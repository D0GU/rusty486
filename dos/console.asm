; =============================================================================
; TinyDOS console I/O
; =============================================================================

; Write AL to the screen (expanding tabs to 8-column stops).
putc:
        pusha
        cmp al, 9
        jne .plain
        mov ah, 0x03
        xor bh, bh
        int 0x10                        ; DL = column
        mov cl, dl
        and cl, 7
        mov ch, 8
        sub ch, cl
.tab:   mov ax, 0x0E20
        mov bx, 0x0007
        int 0x10
        dec ch
        jnz .tab
        popa
        ret
.plain: mov ah, 0x0E
        mov bx, 0x0007
        int 0x10
        popa
        ret

; Print the NUL-terminated string at DS:SI.
puts:
        push ax
        push si
.l:     lodsb
        test al, al
        jz .d
        call putc
        jmp .l
.d:     pop si
        pop ax
        ret

; Read a key: AL = ASCII. Extended keys return 0 first and the scancode on
; the next call, as DOS does.
readkey:
        mov al, [pending_scan]
        test al, al
        jz .read
        mov byte [pending_scan], 0
        ret
.read:  xor ah, ah
        int 0x16
        cmp al, 0xE0
        jne .not_e0
        test ah, ah
        jz .done                        ; real E0 character
        xor al, al
.not_e0:
        test al, al
        jnz .done
        mov [pending_scan], ah
.done:  ret

; ZF = 0 if a key is waiting.
key_ready:
        cmp byte [pending_scan], 0
        jne .yes
        mov ah, 0x01
        int 0x16
        ret                             ; ZF from BIOS
.yes:   ret                             ; ZF = 0 from the cmp

fn_01:  call readkey
        mov [bp+F_AL], al
        test al, al
        jz .r
        call putc
.r:     ret

fn_02:  mov al, [bp+F_DL]
        call putc
        mov [bp+F_AL], al
        ret

fn_06:  mov al, [bp+F_DL]
        cmp al, 0xFF
        je .input
        call putc
        mov [bp+F_AL], al
        ret
.input: call key_ready
        jnz .key
        mov byte [bp+F_AL], 0
        or word [bp+F_FLAGS], 0x0040    ; ZF = 1: no key
        ret
.key:   call readkey
        mov [bp+F_AL], al
        and word [bp+F_FLAGS], ~0x0040
        ret

fn_07:
fn_08:  call readkey
        mov [bp+F_AL], al
        ret

fn_09:  call caller_dsdx
.l:     mov al, [fs:si]
        cmp al, '$'
        je .d
        call putc
        inc si
        jmp .l
.d:     mov byte [bp+F_AL], '$'
        ret

; Buffered input into DS:DX: [max][count][chars...]
fn_0a:  mov es, [bp+F_DS]
        mov di, [bp+F_DX]
        mov cl, [es:di]
        test cl, cl
        jz .r
        dec cl                          ; leave room for CR
        push di
        add di, 2
        call line_input
        pop di
        mov [es:di+1], ch
.r:     ret

fn_0b:  call key_ready
        mov al, 0
        jz .n
        mov al, 0xFF
.n:     mov [bp+F_AL], al
        ret

fn_0c:  ; flush input, then run function AL
.flush: call key_ready
        jz .go
        call readkey
        jmp .flush
.go:    mov al, [bp+F_AL]
        mov [bp+F_AH], al
        cmp al, 0x01
        je fn_01
        cmp al, 0x06
        je fn_06
        cmp al, 0x07
        je fn_07
        cmp al, 0x08
        je fn_08
        cmp al, 0x0A
        je fn_0a
        ret

fn_0d:  call flush_all
        ret

; Read a line with echo and editing into ES:DI (max CL chars). A CR is
; stored after the text. Returns CH = length.
line_input:
        xor ch, ch
.key:   call readkey
        test al, al
        jnz .ascii
        call readkey                    ; discard extended scancode
        jmp .key
.ascii: cmp al, 13
        je .enter
        cmp al, 8
        je .bs
        cmp al, 27
        je .esc
        cmp al, 32
        jb .key
        cmp ch, cl
        jae .key                        ; full
        mov [es:di], al
        inc di
        inc ch
        call putc
        jmp .key
.bs:    test ch, ch
        jz .key
        dec di
        dec ch
        call erase_char
        jmp .key
.esc:   test ch, ch
        jz .key
        dec di
        dec ch
        call erase_char
        jmp .esc
.enter: mov byte [es:di], 13
        mov al, 13
        call putc
        ret

erase_char:
        push ax
        mov al, 8
        call putc
        mov al, ' '
        call putc
        mov al, 8
        call putc
        pop ax
        ret

; Read up to CX bytes from the console into ES:DI (handle 0 semantics:
; whole lines, ending in CR LF). Returns AX = bytes read.
con_read:
        push cx
        mov ax, [con_len]
        cmp [con_pos], ax
        jb .have
        ; Read a fresh line.
        push es
        push di
        push cx
        push ds
        pop es
        mov di, con_line
        mov cl, 125
        call line_input
        movzx bx, ch
        mov byte [con_line+bx], 13
        mov byte [con_line+bx+1], 10
        add bx, 2
        mov [con_len], bx
        mov word [con_pos], 0
        mov al, 10
        call putc
        pop cx
        pop di
        pop es
.have:  mov ax, [con_len]
        sub ax, [con_pos]
        cmp ax, cx
        jbe .n
        mov ax, cx
.n:     mov cx, ax
        mov si, con_line
        add si, [con_pos]
        add [con_pos], ax
        rep movsb
        pop cx
        ret

pending_scan db 0
con_line     times 128 db 0
con_pos      dw 0
con_len      dw 0
