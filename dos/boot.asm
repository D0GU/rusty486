; =============================================================================
; TinyDOS boot sector
;
; The kernel lives in the reserved sectors after this one (the disk builder
; sets the BPB's reserved-sector count to cover it), so loading it is a single
; extended INT 13h read. The rest of the disk is a normal FAT12/16 volume.
; =============================================================================

        bits 16
        cpu 486
        org 0x7C00

%include "dos.inc"

        jmp short start
        nop
; BIOS parameter block (filled in by the disk builder)
oem         db 'TINYDOS '
bps         dw 512
spc         db 1
reserved    dw 1
nfats       db 2
rootents    dw 224
total16     dw 2880
media       db 0xF0
fatsecs     dw 9
spt         dw 18
heads       dw 2
hidden      dd 0
total32     dd 0
drive       db 0
            db 0
signature   db 0x29
volid       dd 0
vlabel      db 'NO NAME    '
fstype      db 'FAT12   '

start:
        cli
        xor ax, ax
        mov ds, ax
        mov ss, ax
        mov sp, 0x7C00
        sti
        cld
        mov [drive], dl
        mov cx, [reserved]
        dec cx                          ; sectors of kernel
        jz no_kernel
        mov [dap_count], cx
        mov si, dap
        mov ah, 0x42
        int 0x13
        jc read_error
        mov dl, [drive]
        jmp KSEG:0x0000

no_kernel:
        mov si, msg_nokernel
        jmp fatal
read_error:
        mov si, msg_error
fatal:  lodsb
        test al, al
        jz .halt
        mov ah, 0x0E
        mov bx, 0x0007
        int 0x10
        jmp fatal
.halt:  cli
        hlt
        jmp .halt

dap:        db 16, 0
dap_count:  dw 0
            dw 0x0000, KSEG             ; buffer
            dd 1, 0                     ; LBA 1

msg_nokernel db 'Not a TinyDOS system disk', 13, 10, 0
msg_error    db 'TinyDOS: disk read error', 13, 10, 0

        times 510 - ($ - $$) db 0
        dw 0xAA55
