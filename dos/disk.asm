; =============================================================================
; TinyDOS disk layer: drive table, sector I/O, caches, FAT access
; =============================================================================

; Mount drive index AL from BIOS drive DL. Marks it present if it holds a
; FAT volume with 512-byte sectors.
mount:
        pusha
        movzx bx, al
        imul bx, bx, DRV_SIZE
        add bx, drives
        cmp byte [remount], 0
        je .fresh
        cmp byte [bx+D_PRESENT], 0
        jne .keepcwd
.fresh: mov word [bx+D_CWD], 0
        mov byte [bx+D_PATH], 0
.keepcwd:
        mov byte [bx+D_PRESENT], 0
        mov [bx+D_BIOS], dl
        mov word [bx+D_FREEHINT], 2
        ; Read the boot sector into the scratch buffer.
        push bx
        xor eax, eax
        mov di, bx
        push ds
        pop es
        mov bx, scratch
        mov ah, 0x42
        call disk_io_lba0
        pop bx
        jc .done
        cmp word [scratch+11], 512
        jne .done
        mov al, [scratch+13]
        test al, al
        jz .done
        mov [bx+D_SPC], al
        xor cl, cl
.shift: shr al, 1
        jz .gotshift
        inc cl
        jmp .shift
.gotshift:
        mov [bx+D_CLSHIFT], cl
        mov ax, [scratch+14]
        mov [bx+D_RESERVED], ax
        mov al, [scratch+16]
        mov [bx+D_NFATS], al
        mov ax, [scratch+17]
        mov [bx+D_ROOTENTS], ax
        mov ax, [scratch+22]
        mov [bx+D_FATSECS], ax
        ; root_start = reserved + nfats * fatsecs
        movzx eax, word [scratch+22]
        movzx ecx, byte [scratch+16]
        imul eax, ecx
        movzx ecx, word [scratch+14]
        add eax, ecx
        mov [bx+D_ROOTSTART], eax
        ; data_start = root_start + ceil(rootents * 32 / 512)
        movzx ecx, word [scratch+17]
        add ecx, 15
        shr ecx, 4
        add eax, ecx
        mov [bx+D_DATASTART], eax
        ; total sectors
        movzx ecx, word [scratch+19]
        test ecx, ecx
        jnz .t16
        mov ecx, [scratch+32]
.t16:   sub ecx, eax
        jbe .done
        mov al, [bx+D_CLSHIFT]
        push cx
        mov cl, al
        pop ax                          ; (low word of data sectors, unused)
        mov eax, ecx
        mov cl, [bx+D_CLSHIFT]
        shr eax, cl
        cmp eax, 65524
        jbe .ncl
        mov eax, 65524
.ncl:   mov [bx+D_CLUSTERS], ax
        cmp ax, 4085
        setae al
        mov [bx+D_FAT16], al
        mov byte [bx+D_PRESENT], 1
.done:  popa
        ret

; Read sector 0 of BIOS drive DL into ES:BX (used by mount). CF on error.
disk_io_lba0:
        xor eax, eax
        mov [dap_lba], eax
        mov [dap_off], bx
        mov [dap_seg], es
        mov word [dap_count], 1
        mov si, dap
        mov ah, 0x42
        int 0x13
        ret

; Sector I/O on the current drive [dp]. EAX = LBA, ES:BX = buffer,
; CL = 0x42 read / 0x43 write. CF on error.
disk_io:
        push si
        push dx
        push ax
        mov [dap_lba], eax
        mov [dap_off], bx
        mov [dap_seg], es
        mov word [dap_count], 1
        mov si, [dp]
        mov dl, [si+D_BIOS]
        mov si, dap
        mov ah, cl
        xor al, al
        int 0x13
        pop ax
        pop dx
        pop si
        ret

dap:        db 16, 0
dap_count:  dw 1
dap_off:    dw 0
dap_seg:    dw 0
dap_lba:    dd 0, 0

; ---------------------------------------------------------------- sector cache

; Make sure sector EAX of drive [dp] is in secbuf. Returns BX = secbuf.
get_sector:
        push cx
        push es
        mov cx, [dp]
        cmp cx, [sc_drive]
        jne .miss
        cmp eax, [sc_lba]
        je .hit
.miss:  call flush_sector
        push ds
        pop es
        mov bx, secbuf
        mov cl, 0x42
        call disk_io
        jc .err
        mov cx, [dp]
        mov [sc_drive], cx
        mov [sc_lba], eax
.hit:   mov bx, secbuf
        clc
.err:   pop es
        pop cx
        ret

; Point the cache at sector EAX without reading it (the caller will
; overwrite the whole sector). Returns BX = secbuf.
claim_sector:
        call flush_sector
        push cx
        mov cx, [dp]
        mov [sc_drive], cx
        mov [sc_lba], eax
        pop cx
        mov bx, secbuf
        ret

mark_dirty:
        mov byte [sc_dirty], 1
        ret

flush_sector:
        cmp byte [sc_dirty], 0
        je .r
        push eax
        push bx
        push cx
        push es
        push word [dp]
        mov ax, [sc_drive]
        mov [dp], ax
        push ds
        pop es
        mov eax, [sc_lba]
        mov bx, secbuf
        mov cl, 0x43
        call disk_io
        pop word [dp]
        mov byte [sc_dirty], 0
        pop es
        pop cx
        pop bx
        pop eax
.r:     ret

; Forget cached sectors (after writes, before a disk might change).
flush_all:
        call flush_sector
        call fat_flush
        mov word [sc_drive], 0
        mov word [fw_drive], 0
        ret

; ---------------------------------------------------------------- FAT

; Load the FAT window starting at FAT sector index AX of drive [dp].
fat_window:
        push bx
        mov bx, [dp]
        cmp bx, [fw_drive]
        jne .load
        cmp ax, [fw_index]
        je .ok
.load:  call fat_flush
        mov [fw_index], ax
        mov [fw_drive], bx
        push eax
        push cx
        push es
        push ds
        pop es
        movzx eax, ax
        movzx ecx, word [bx+D_RESERVED]
        add eax, ecx
        push bx
        mov bx, fatbuf
        mov cl, 0x42
        call disk_io
        pop bx
        mov cx, [fw_index]
        inc cx
        cmp cx, [bx+D_FATSECS]
        jae .one
        inc eax
        mov bx, fatbuf+512
        mov cl, 0x42
        call disk_io
.one:   pop es
        pop cx
        pop eax
.ok:    pop bx
        ret

; Write the FAT window back to every FAT copy if it changed.
fat_flush:
        cmp byte [fw_dirty], 0
        je .r
        pushad
        push es
        push word [dp]
        mov bx, [fw_drive]
        mov [dp], bx
        push ds
        pop es
        movzx edx, byte [bx+D_NFATS]
        movzx eax, word [bx+D_RESERVED]
        movzx ecx, word [fw_index]
        add eax, ecx                    ; first copy
.copy:  push eax
        push bx
        mov bx, fatbuf
        mov cl, 0x43
        call disk_io
        pop bx
        mov cx, [fw_index]
        inc cx
        cmp cx, [bx+D_FATSECS]
        jae .next
        inc eax
        push bx
        mov bx, fatbuf+512
        mov cl, 0x43
        call disk_io
        pop bx
.next:  pop eax
        movzx ecx, word [bx+D_FATSECS]
        add eax, ecx
        dec edx
        jnz .copy
        mov byte [fw_dirty], 0
        pop word [dp]
        pop es
        popad
.r:     ret

; Byte offset of cluster AX's FAT entry -> AX = window index, BX = offset
; in fatbuf (the window is loaded).
fat_locate:
        push dx
        push si
        mov si, [dp]
        movzx edx, ax
        cmp byte [si+D_FAT16], 0
        je .f12
        shl edx, 1
        jmp .have
.f12:   mov ebx, edx
        shr ebx, 1
        add edx, ebx
.have:  mov bx, dx
        and bx, 511
        shr edx, 9
        mov ax, dx
        call fat_window
        pop si
        pop dx
        ret

; AX = cluster -> AX = next cluster (FFFFh for end of chain).
fat_get:
        push bx
        push cx
        mov cx, ax
        call fat_locate
        mov ax, [fatbuf+bx]
        push si
        mov si, [dp]
        cmp byte [si+D_FAT16], 0
        pop si
        jne .f16
        test cl, 1
        jz .even
        shr ax, 4
        jmp .f12
.even:  and ax, 0x0FFF
.f12:   cmp ax, 0x0FF7
        jb .r
        mov ax, 0xFFFF
        jmp .r
.f16:   cmp ax, 0xFFF7
        jb .r
        mov ax, 0xFFFF
.r:     pop cx
        pop bx
        ret

; Set cluster AX's FAT entry to DX.
fat_set:
        push ax
        push bx
        push cx
        push dx
        mov cx, ax
        call fat_locate
        push si
        mov si, [dp]
        cmp byte [si+D_FAT16], 0
        pop si
        jne .f16
        and dx, 0x0FFF
        mov ax, [fatbuf+bx]
        test cl, 1
        jz .even
        and ax, 0x000F
        shl dx, 4
        or ax, dx
        jmp .store
.even:  and ax, 0xF000
        or ax, dx
.store: mov [fatbuf+bx], ax
        jmp .done
.f16:   mov [fatbuf+bx], dx
.done:  mov byte [fw_dirty], 1
        pop dx
        pop cx
        pop bx
        pop ax
        ret

; Allocate a free cluster, mark it end-of-chain, zero it, and link it after
; cluster DX (if DX != 0). Returns AX = cluster, CF if the disk is full.
alloc_cluster:
        push bx
        push cx
        push si
        mov si, [dp]
        mov cx, [si+D_CLUSTERS]         ; clusters to scan
        mov bx, [si+D_FREEHINT]
.scan:  cmp bx, 2
        jb .wrap
        mov ax, [si+D_CLUSTERS]
        inc ax
        cmp bx, ax
        jbe .test
.wrap:  mov bx, 2
.test:  mov ax, bx
        call fat_get
        test ax, ax
        jz .found
        inc bx
        loop .scan
        stc
        jmp .r
.found: mov ax, bx
        inc bx
        mov [si+D_FREEHINT], bx
        push dx
        mov dx, 0xFFFF
        call fat_set
        pop dx
        test dx, dx
        jz .nolink
        xchg ax, dx
        call fat_set                    ; prev -> new
        xchg ax, dx
.nolink:
        call zero_cluster
        clc
.r:     pop si
        pop cx
        pop bx
        ret

; Fill cluster AX with zeros on disk.
zero_cluster:
        pushad
        mov si, [dp]
        call cluster_lba
        movzx cx, byte [si+D_SPC]
.s:     push cx
        call claim_sector
        push di
        push es
        push ds
        pop es
        mov di, secbuf
        mov cx, 256
        xor ax, ax
        rep stosw
        pop es
        pop di
        call mark_dirty
        call flush_sector
        inc eax
        pop cx
        loop .s
        popad
        ret

; Free the chain starting at cluster AX.
free_chain:
        push ax
        push dx
.l:     cmp ax, 2
        jb .d
        cmp ax, 0xFFFF
        je .d
        push ax
        call fat_get
        mov dx, ax
        pop ax
        push dx
        xor dx, dx
        call fat_set
        pop ax
        jmp .l
.d:     pop dx
        pop ax
        ret

; AX = cluster -> EAX = first sector of the cluster.
cluster_lba:
        push cx
        push si
        mov si, [dp]
        movzx eax, ax
        sub eax, 2
        mov cl, [si+D_CLSHIFT]
        shl eax, cl
        add eax, [si+D_DATASTART]
        pop si
        pop cx
        ret

; Select drive index AL as [dp]. CF (AX = E_DRIVE) if not present.
select_drive:
        cmp al, MAX_DRIVES
        jae .bad
        push bx
        movzx bx, al
        imul bx, bx, DRV_SIZE
        add bx, drives
        cmp byte [bx+D_PRESENT], 0
        je .badpop
        mov [dp], bx
        mov [dp_index], al
        pop bx
        clc
        ret
.badpop:
        pop bx
.bad:   mov ax, E_DRIVE
        stc
        ret

; AH=36h: disk free space for drive DL (0 = default).
fn_36:  mov al, [bp+F_DL]
        dec al
        jns .have
        mov al, [cur_drive]
.have:  call select_drive
        jc .bad
        mov si, [dp]
        xor dx, dx                      ; free count
        mov cx, [si+D_CLUSTERS]
        mov bx, 2
.l:     mov ax, bx
        call fat_get
        test ax, ax
        jnz .used
        inc dx
.used:  inc bx
        loop .l
        movzx ax, byte [si+D_SPC]
        mov [bp+F_AX], ax
        mov [bp+F_BX], dx
        mov word [bp+F_CX], 512
        mov ax, [si+D_CLUSTERS]
        mov [bp+F_DX], ax
        ret
.bad:   mov word [bp+F_AX], 0xFFFF
        ret

remount     db 0                        ; mount keeps the current directory
dp          dw drives                   ; current drive entry
dp_index    db 0
drives      times MAX_DRIVES * DRV_SIZE db 0

sc_drive    dw 0
sc_lba      dd 0
sc_dirty    db 0
fw_drive    dw 0
fw_index    dw 0
fw_dirty    db 0

        align 16
secbuf      times 512 db 0
fatbuf      times 1024 db 0
scratch     times 512 db 0
