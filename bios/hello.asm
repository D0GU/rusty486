; Milestone 1 demo boot sector: prints a greeting through the BIOS and halts.
; Assembled to a 512-byte boot sector and placed on a 1.44 MB floppy image.

        bits 16
        cpu 486
        org 0x7C00

start:
        jmp 0x0000:main                 ; normalise CS:IP to 0000:7C00

main:
        xor ax, ax
        mov ds, ax
        mov es, ax
        mov ss, ax
        mov sp, 0x7C00
        cld

        ; Title bar: white on blue across the top row.
        mov ax, 0x1300 | 0x01           ; write string, move cursor
        mov bx, 0x001F                  ; page 0, attribute 1Fh
        mov cx, title_len
        xor dx, dx
        mov bp, title
        int 0x10

        ; Greeting below the BIOS messages, via teletype.
        mov ah, 0x02
        xor bh, bh
        mov dx, 0x0800                  ; row 8, column 0
        int 0x10
        mov si, hello
.next:  lodsb
        test al, al
        jz .done
        mov ah, 0x0E
        mov bx, 0x0007
        int 0x10
        jmp .next

.done:  cli
        hlt
        jmp .done

title   db ' rusty486                                      fantasy 486 PC  -  milestone 1 '
title_len equ $ - title
hello   db 'Hello from rusty486!', 13, 10, 0

        times 510 - ($ - $$) db 0
        dw 0xAA55
