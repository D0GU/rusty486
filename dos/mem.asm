; =============================================================================
; TinyDOS memory management: DOS-style memory control blocks (MCBs)
;
; Each block is preceded by a 16-byte MCB: 'M' (or 'Z' for the last),
; owner PSP (0 = free), size in paragraphs, and an 8-byte name at offset 8.
; =============================================================================

mem_init:
        push es
        mov ax, FIRST_MCB
        mov es, ax
        mov byte [es:0], 'Z'
        mov word [es:1], 0
        mov word [es:3], MEM_TOP - FIRST_MCB - 1
        mov dword [es:8], 0
        mov dword [es:12], 0
        pop es
        ret

; Merge free blocks following the MCB at ES into it.
mcb_merge:
        push ax
        push dx
.l:     cmp byte [es:0], 'M'
        jne .d
        mov dx, es
        add dx, [es:3]
        inc dx
        push es
        mov es, dx
        cmp word [es:1], 0
        mov al, [es:0]
        mov dx, [es:3]
        pop es
        jne .d
        mov [es:0], al
        inc dx
        add [es:3], dx
        jmp .l
.d:     pop dx
        pop ax
        ret

; Split the block at ES (MCB) to BX paragraphs; the rest becomes free.
mcb_split:
        push ax
        push dx
        mov dx, [es:3]
        cmp dx, bx
        jbe .d
        sub dx, bx
        dec dx                          ; size of the new free block
        mov ax, es
        add ax, bx
        inc ax
        push es
        mov al, [es:0]
        push ax
        mov ax, es
        add ax, bx
        inc ax
        mov es, ax
        pop ax
        mov [es:0], al
        mov word [es:1], 0
        mov [es:3], dx
        mov dword [es:8], 0
        mov dword [es:12], 0
        pop es
        mov byte [es:0], 'M'
        mov [es:3], bx
.d:     pop dx
        pop ax
        ret

; Allocate BX paragraphs owned by AX. Returns AX = segment.
; CF: AX = error, BX = largest free block.
mem_alloc:
        push cx
        push dx
        push es
        mov [ma_owner], ax
        mov [ma_want], bx
        mov word [ma_largest], 0
        mov ax, FIRST_MCB
.l:     mov es, ax
        mov cl, [es:0]
        cmp cl, 'M'
        je .valid
        cmp cl, 'Z'
        jne .corrupt
.valid: cmp word [es:1], 0
        jne .next
        call mcb_merge
        mov cx, [es:3]
        cmp cx, [ma_largest]
        jbe .nl
        mov [ma_largest], cx
.nl:    cmp cx, [ma_want]
        jb .next
        mov bx, [ma_want]
        call mcb_split
        mov ax, [ma_owner]
        mov [es:1], ax
        mov ax, es
        inc ax
        clc
        jmp .r
.next:  cmp byte [es:0], 'Z'
        je .full
        mov ax, es
        add ax, [es:3]
        inc ax
        jmp .l
.corrupt:
        mov ax, E_MCB
        stc
        jmp .r
.full:  mov ax, E_NOMEM
        mov bx, [ma_largest]
        stc
.r:     pop es
        pop dx
        pop cx
        ret

; Free the block at segment ES. CF/AX on a bad block.
mem_free:
        push es
        mov ax, es
        dec ax
        mov es, ax
        mov al, [es:0]
        cmp al, 'M'
        je .ok
        cmp al, 'Z'
        jne .bad
.ok:    mov word [es:1], 0
        pop es
        clc
        ret
.bad:   pop es
        mov ax, E_BLOCK
        stc
        ret

; Resize the block at segment ES to BX paragraphs.
; CF: AX = error, BX = the most it can grow to.
mem_resize:
        push cx
        push es
        mov ax, es
        dec ax
        mov es, ax
        mov al, [es:0]
        cmp al, 'M'
        je .ok
        cmp al, 'Z'
        jne .bad
.ok:    mov cx, [es:3]                  ; original size
        cmp bx, cx
        jbe .split
        call mcb_merge
        cmp bx, [es:3]
        jbe .split
        ; Not enough: give back what was merged.
        push bx
        mov bx, [es:3]
        mov [ma_largest], bx
        mov bx, cx
        call mcb_split
        pop bx
        mov bx, [ma_largest]
        mov ax, E_NOMEM
        stc
        jmp .r
.split: call mcb_split
        clc
        jmp .r
.bad:   mov ax, E_BLOCK
        stc
.r:     pop es
        pop cx
        ret

; Free every block owned by PSP AX.
free_owner:
        push bx
        push es
        mov bx, FIRST_MCB
.l:     mov es, bx
        cmp [es:1], ax
        jne .n
        mov word [es:1], 0
.n:     cmp byte [es:0], 'Z'
        je .d
        cmp byte [es:0], 'M'
        jne .d
        add bx, [es:3]
        inc bx
        jmp .l
.d:     pop es
        pop bx
        ret

; Change the owner of the block at segment AX to DX.
set_owner:
        push es
        dec ax
        mov es, ax
        mov [es:1], dx
        inc ax
        pop es
        ret

fn_48:  mov bx, [bp+F_BX]
        mov ax, [cur_psp]
        call mem_alloc
        jc .f
        mov [bp+F_AX], ax
        jmp ok
.f:     mov [bp+F_BX], bx
        jmp fail

fn_49:  mov es, [bp+F_ES]
        call mem_free
        jc fail
        jmp ok

fn_4a:  mov es, [bp+F_ES]
        mov bx, [bp+F_BX]
        call mem_resize
        jc .f
        jmp ok
.f:     mov [bp+F_BX], bx
        jmp fail

ma_owner    dw 0
ma_want     dw 0
ma_largest  dw 0
