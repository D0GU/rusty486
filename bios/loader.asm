; =============================================================================
; rusty486 program loader (boot sector)
;
; The editor's build writes this to sector 0 of a 1.44 MB floppy, followed by
; the assembled program. It loads the program to 1000:0100 and runs it like a
; DOS .COM file: CS=DS=ES=SS=1000h, SP=FFFEh, DL=boot drive. A near RET from
; the program lands on a halt stub at 1000:0000.
;
; The build patches the word at offset 1FCh with the program's sector count.
; =============================================================================

        bits 16
        cpu 486
        org 0x7C00

%define LOAD_SEG  0x1000
%define SPT       18                    ; 1.44 MB floppy geometry
%define HEADS     2

start:
        jmp 0x0000:main

main:
        cli
        xor ax, ax
        mov ds, ax
        mov ss, ax
        mov sp, 0x7C00
        sti
        cld
        mov [drive], dl

        mov ax, LOAD_SEG
        mov es, ax
        mov word [es:0], 0xF4FA         ; halt stub: cli / hlt
        mov word [es:2], 0xFDEB         ; jmp $-1
        mov bx, 0x0100                  ; ES:BX = load address
        mov si, 1                       ; first program sector (LBA)
        mov cx, [sectors]
        jcxz run

.next:  push cx
        mov ax, si                      ; LBA -> CHS
        xor dx, dx
        mov di, SPT
        div di                          ; AX = track, DX = sector - 1
        mov cl, dl
        inc cl                          ; CL = sector (1-based)
        mov dh, al
        and dh, HEADS - 1               ; DH = head
        shr ax, 1
        mov ch, al                      ; CH = cylinder
        mov dl, [drive]
        mov ax, 0x0201                  ; read one sector
        int 0x13
        jc error
        add bx, 512
        inc si
        pop cx
        loop .next

run:    mov dl, [drive]
        mov ax, LOAD_SEG
        mov ds, ax
        mov es, ax
        cli
        mov ss, ax
        mov sp, 0xFFFE
        sti
        push word 0                     ; near RET -> 1000:0000 halt stub
        jmp LOAD_SEG:0x0100

error:  mov si, msg_error
.print: lodsb
        test al, al
        jz .halt
        mov ah, 0x0E
        mov bx, 0x0007
        int 0x10
        jmp .print
.halt:  cli
        hlt
        jmp .halt

drive     db 0
msg_error db 'Loader: disk read error', 13, 10, 0

        times 0x1FC - ($ - $$) db 0
sectors dw 0                            ; patched by the build
        dw 0xAA55
