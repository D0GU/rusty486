; =============================================================================
; TinyDOS program loading (.COM and MZ .EXE) and termination
; =============================================================================

%define MAX_DEPTH 8

; AH=4Bh, AL=00h: load and execute DS:DX with parameter block ES:BX.
fn_4b:  cmp byte [bp+F_AL], 0
        je .go
        mov ax, E_FUNC
        jmp fail
.go:    cmp byte [exec_depth], MAX_DEPTH
        jb .room
        mov ax, E_NOMEM
        jmp fail
.room:  call caller_dsdx
        call resolve
        jc fail
        call lookup_name
        jc .nofile
        test byte [bx+DE_ATTR], A_DIR | A_VOLUME
        jnz .nofile
        ; Private file control block for reading the program.
        push ds
        pop es
        mov di, exec_file
        mov cx, SFT_SIZE
        xor al, al
        rep stosb
        mov byte [exec_file+S_FLAGS], 1
        mov al, [dp_index]
        mov [exec_file+S_DRIVE], al
        mov ax, [bx+DE_CLUSTER]
        mov [exec_file+S_CLUSTER], ax
        mov [exec_file+S_CURCL], ax
        mov eax, [bx+DE_SIZE]
        mov [exec_file+S_SIZE], eax
        ; Header.
        mov dword [exe_hdr], 0
        mov si, exec_file
        mov di, exe_hdr
        mov cx, 28
        call file_read
        mov ax, [exe_hdr]
        cmp ax, 'MZ'
        je .exe
        cmp ax, 'ZM'
        je .exe
        mov byte [is_exe], 0
        mov eax, [exec_file+S_SIZE]
        cmp eax, 0xFF00
        ja .format
        jmp .env
.exe:   mov byte [is_exe], 1
        ; Load image size in paragraphs.
        movzx eax, word [exe_hdr+4]     ; pages
        shl eax, 9
        movzx ecx, word [exe_hdr+2]     ; bytes in last page
        test ecx, ecx
        jz .full
        sub eax, 512
        add eax, ecx
.full:  movzx ecx, word [exe_hdr+8]     ; header paragraphs
        shl ecx, 4
        sub eax, ecx
        jbe .format
        mov [image_bytes], eax
        add eax, 15
        shr eax, 4
        mov [image_paras], ax
.env:
        ; ------------------------------------------------ environment
        mov es, [bp+F_ES]
        mov bx, [bp+F_BX]
        mov ax, [es:bx]                 ; environment segment
        test ax, ax
        jnz .haveenv
        mov ax, [cur_psp]
        test ax, ax
        jz .kenv
        mov es, ax
        mov ax, [es:0x2C]
        test ax, ax
        jnz .haveenv
.kenv:  mov ax, ds
        mov word [env_src_off], default_env
        jmp .envlen
.haveenv:
        mov word [env_src_off], 0
.envlen:
        mov [env_src_seg], ax
        mov es, ax
        mov di, [env_src_off]
        xor cx, cx                      ; length up to and including "\0\0"
.scan:  cmp word [es:di], 0
        je .endenv
        inc di
        inc cx
        cmp cx, 0x7FF0
        jb .scan
.endenv:
        add cx, 2
        mov [env_len], cx
        ; + count word + path
        call caller_dsdx
        xor dx, dx
.pl:    cmp byte [fs:si], 0
        je .pe
        inc si
        inc dx
        jmp .pl
.pe:    add dx, 2 + 1 + 2               ; count word, NUL, "X:" prefix
        add cx, dx
        add cx, 15
        shr cx, 4
        mov bx, cx
        mov ax, 0xFFFF                  ; temporary owner
        call mem_alloc
        jc .nomem
        mov [env_seg], ax
        ; Copy the environment.
        push ds
        mov es, ax
        xor di, di
        mov cx, [env_len]
        mov si, [env_src_off]
        mov ds, [env_src_seg]
        rep movsb
        pop ds
        mov word [es:di], 1
        add di, 2
        ; Program path, with a drive prefix.
        call caller_dsdx
        cmp byte [fs:si+1], ':'
        je .cpath
        mov al, [dp_index]
        add al, 'A'
        stosb
        mov al, ':'
        stosb
.cpath: mov al, [fs:si]
        cmp al, 'a'
        jb .cu
        cmp al, 'z'
        ja .cu
        sub al, 32
.cu:    stosb
        inc si
        test al, al
        jnz .cpath

        ; ------------------------------------------------ program memory
        mov bx, 0xFFFF
        mov ax, 0xFFFF
        call mem_alloc                  ; fails, giving the largest block in BX
        mov [mem_largest], bx
        cmp byte [is_exe], 0
        jne .exemem
        ; .COM: the largest block; must hold PSP + file + a small stack.
        mov eax, [exec_file+S_SIZE]
        add eax, 0x100 + 0x200
        shr eax, 4
        cmp ax, bx
        ja .nomem_env
        jmp .alloc
.exemem:
        ; need = PSP + image + minalloc; want = PSP + image + maxalloc
        mov ax, [image_paras]
        add ax, 0x10
        jc .nomem_env
        mov cx, ax
        add ax, [exe_hdr+0x0A]
        jc .nomem_env
        cmp ax, bx
        ja .nomem_env
        add cx, [exe_hdr+0x0C]
        jnc .wantok
        mov cx, 0xFFFF
.wantok:
        cmp cx, bx
        jae .alloc
        mov bx, cx
.alloc: mov ax, 0xFFFF
        call mem_alloc
        jc .nomem_env
        mov [child_psp], ax
        mov dx, ax
        call set_owner                  ; program block owned by its PSP
        mov ax, [env_seg]
        call set_owner
        ; Memory top for the PSP.
        mov es, [child_psp]
        mov ax, es
        dec ax
        push es
        mov es, ax
        mov ax, [es:3]
        pop es
        add ax, [child_psp]
        mov [child_top], ax

        ; ------------------------------------------------ PSP
        xor di, di
        mov cx, 128
        xor ax, ax
        rep stosw
        mov word [es:0], 0x20CD         ; INT 20h
        mov ax, [child_top]
        mov [es:2], ax
        push es
        xor ax, ax
        mov es, ax
        mov eax, [es:0x22*4]
        mov ecx, [es:0x23*4]
        mov edx, [es:0x24*4]
        pop es
        mov [es:0x0A], eax
        mov [es:0x0E], ecx
        mov [es:0x12], edx
        mov ax, [cur_psp]
        test ax, ax
        jnz .par
        mov ax, [child_psp]             ; the first process is its own parent
.par:   mov [es:0x16], ax
        ; Inherit handles 0-19 from the parent (or the console for the first).
        mov cx, 20
        xor bx, bx
.jft:   mov al, 0xFF
        cmp word [cur_psp], 0
        jne .inh
        cmp bx, 5
        jae .setj
        xor al, al
        jmp .setj
.inh:   push es
        mov es, [cur_psp]
        mov al, [es:0x18+bx]
        pop es
.setj:  mov [es:0x18+bx], al
        cmp al, NSFT
        jae .nj
        movzx si, al
        imul si, si, SFT_SIZE
        add si, sft
        cmp byte [si+S_FLAGS], 0
        je .nj
        inc byte [si+S_REFS]
.nj:    inc bx
        loop .jft
        mov ax, [env_seg]
        mov [es:0x2C], ax
        mov word [es:0x32], 20
        mov word [es:0x34], 0x18
        mov [es:0x36], es
        mov dword [es:0x38], 0xFFFFFFFF
        mov word [es:0x50], 0x21CD      ; INT 21h / RETF
        mov byte [es:0x52], 0xCB
        ; FCBs (blank) and the command tail.
        mov byte [es:0x5C], 0
        mov di, 0x5D
        mov cx, 11
        mov al, ' '
        rep stosb
        mov byte [es:0x6C], 0
        mov di, 0x6D
        mov cx, 11
        rep stosb
        push ds
        mov fs, [bp+F_ES]
        mov bx, [bp+F_BX]
        lds si, [fs:bx+2]               ; command tail
        mov di, 0x80
        mov cl, [si]
        cmp cl, 126
        jbe .tl
        mov cl, 126
.tl:    movzx cx, cl
        mov [es:di], cl
        inc si
        inc di
        rep movsb
        mov byte [es:di], 13
        pop ds

        ; ------------------------------------------------ load the image
        mov si, exec_file
        cmp byte [is_exe], 0
        jne .loadexe
        mov dword [si+S_POS], 0
        mov word [si+S_CURIDX], 0
        mov ax, [si+S_CLUSTER]
        mov [si+S_CURCL], ax
        mov es, [child_psp]
        mov di, 0x100
        mov cx, [si+S_SIZE]
        call file_read
        ; Registers for a .COM program.
        mov ax, [child_psp]
        mov [start_cs], ax
        mov [start_ss], ax
        mov word [start_ip], 0x100
        mov ax, [child_top]
        sub ax, [child_psp]
        cmp ax, 0x1000
        mov ax, 0xFFFE
        jae .comsp
        mov ax, [child_top]
        sub ax, [child_psp]
        shl ax, 4
        sub ax, 2
.comsp: mov [start_sp], ax
        jmp .launch

.loadexe:
        movzx eax, word [exe_hdr+8]
        shl eax, 4
        mov [si+S_POS], eax
        mov ax, [child_psp]
        add ax, 0x10
        mov [load_seg], ax
        mov [cur_load], ax
        mov ecx, [image_bytes]
.chunk: test ecx, ecx
        jz .relocs
        push ecx
        cmp ecx, 0x8000
        jbe .last
        mov ecx, 0x8000
.last:  mov es, [cur_load]
        xor di, di
        push cx
        call file_read
        pop cx
        add word [cur_load], 0x800
        movzx ecx, cx
        mov eax, ecx
        pop ecx
        sub ecx, eax
        jmp .chunk
.relocs:
        movzx eax, word [exe_hdr+0x18]
        mov [si+S_POS], eax
        mov cx, [exe_hdr+6]
.rbatch:
        test cx, cx
        jz .regs
        push cx
        cmp cx, 64
        jbe .rb
        mov cx, 64
.rb:    push cx
        push ds
        pop es
        mov di, reloc_buf
        shl cx, 2
        call file_read
        pop cx
        mov bx, reloc_buf
.rl:    mov di, [bx]
        mov ax, [bx+2]
        add ax, [load_seg]
        mov es, ax
        mov ax, [load_seg]
        add [es:di], ax
        add bx, 4
        loop .rl
        pop cx
        sub cx, 64
        jae .rbatch
        xor cx, cx
        jmp .rbatch
.regs:  mov ax, [load_seg]
        add ax, [exe_hdr+0x16]
        mov [start_cs], ax
        mov ax, [exe_hdr+0x14]
        mov [start_ip], ax
        mov ax, [load_seg]
        add ax, [exe_hdr+0x0E]
        mov [start_ss], ax
        mov ax, [exe_hdr+0x10]
        mov [start_sp], ax

.launch:
        ; Save the parent's state and switch to the child.
        movzx bx, byte [exec_depth]
        imul bx, bx, 10
        mov ax, [cur_psp]
        mov [exec_stack+bx], ax
        mov eax, [dta]
        mov [exec_stack+bx+2], eax
        mov [exec_stack+bx+6], ss
        mov [exec_stack+bx+8], bp
        inc byte [exec_depth]
        mov ax, [child_psp]
        mov [cur_psp], ax
        mov word [dta], 0x80
        mov [dta+2], ax
        call flush_all
        cli
        mov ss, [start_ss]
        mov sp, [start_sp]
        cmp byte [is_exe], 0
        jne .noret
        push word 0                     ; .COM: RET goes to PSP:0000 (INT 20h)
.noret: sti
        push word 0x0202                ; flags (IF set)
        push word [start_cs]
        push word [start_ip]
        mov ax, [child_psp]
        mov es, ax
        mov ds, ax
        xor ax, ax
        xor bx, bx
        mov cx, 0xFF
        xor si, si
        xor di, di
        xor bp, bp
        iret

.nofile:
        mov ax, E_NOFILE
        jmp fail
.format:
        mov ax, E_FORMAT
        jmp fail
.nomem_env:
        push es
        mov es, [env_seg]
        call mem_free
        pop es
.nomem: mov ax, E_NOMEM
        jmp fail

; AH=4Ch terminate with return code AL.
fn_4c:  mov al, [bp+F_AL]
        xor ah, ah
        mov [return_code], ax
        mov word [keep_paras], 0
        jmp terminate

; AH=31h terminate and stay resident, keeping DX paragraphs.
fn_31:  mov al, [bp+F_AL]
        mov ah, 3
        mov [return_code], ax
        mov ax, [bp+F_DX]
        cmp ax, 6
        jae .k
        mov ax, 6
.k:     mov [keep_paras], ax
        jmp terminate

terminate:
        cmp byte [exec_depth], 0
        je .nowhere
        ; Close the child's files.
        xor bx, bx
.cl:    push bx
        call close_handle
        pop bx
        inc bx
        cmp bx, 20
        jb .cl
        mov ax, [cur_psp]
        cmp word [keep_paras], 0
        jne .tsr
        call free_owner
        jmp .freed
.tsr:   mov es, ax
        mov bx, [keep_paras]
        call mem_resize
.freed: call flush_all
        dec byte [exec_depth]
        movzx bx, byte [exec_depth]
        imul bx, bx, 10
        mov ax, [exec_stack+bx]
        mov [cur_psp], ax
        mov eax, [exec_stack+bx+2]
        mov [dta], eax
        cli
        mov ss, [exec_stack+bx+6]
        mov sp, [exec_stack+bx+8]
        sti
        mov bp, sp
        and byte [bp+F_FLAGS], 0xFE     ; the EXEC call succeeded
        jmp int21_ret
.nowhere:
        mov si, msg_noparent
        call puts
.h:     hlt
        jmp .h

; INT 20h: terminate.
int20:  mov ax, 0x4C00
        int 0x21

; INT 27h: terminate and stay resident; DX = bytes to keep (from PSP).
int27:  add dx, 15
        shr dx, 4
        mov ax, 0x3100
        int 0x21

msg_noparent db 'TinyDOS: no process to return to', 13, 10, 0
default_env  db 'PATH=\', 0, 'COMSPEC=\COMMAND.COM', 0, 0

exec_depth   db 0
exec_stack   times MAX_DEPTH * 10 db 0
exec_file    times SFT_SIZE db 0
exe_hdr      times 28 db 0
is_exe       db 0
image_bytes  dd 0
image_paras  dw 0
env_src_seg  dw 0
env_src_off  dw 0
env_len      dw 0
env_seg      dw 0
mem_largest  dw 0
child_psp    dw 0
child_top    dw 0
load_seg     dw 0
cur_load     dw 0
start_cs     dw 0
start_ip     dw 0
start_ss     dw 0
start_sp     dw 0
keep_paras   dw 0
reloc_buf    times 256 db 0
