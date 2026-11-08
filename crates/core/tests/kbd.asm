; Test boot sector: echo keys via INT 16h until '!', then print the number of
; timer ticks (INT 1Ah) that elapsed since boot and halt.
        bits 16
        org 0x7C00
start:  jmp 0:main
main:   xor ax, ax
        mov ds, ax
        mov ss, ax
        mov sp, 0x7C00
        sti
        mov ah, 0
        int 0x1A
        mov [t0], dx
.loop:  mov ah, 0
        int 0x16
        cmp al, '!'
        je .done
        mov ah, 0x0E
        mov bx, 7
        int 0x10
        jmp .loop
.done:  mov ah, 0
        int 0x1A
        sub dx, [t0]
        mov ax, dx
        mov si, msg
.p:     lodsb
        test al, al
        jz .num
        mov ah, 0x0E
        int 0x10
        jmp .p
.num:   mov ax, dx
        mov cx, 0
        mov bx, 10
.div:   xor dx, dx
        div bx
        push dx
        inc cx
        test ax, ax
        jnz .div
.out:   pop ax
        add al, '0'
        mov ah, 0x0E
        int 0x10
        loop .out
        cli
        hlt
t0      dw 0
msg     db ' T=', 0
        times 510 - ($ - $$) db 0
        dw 0xAA55
