; =============================================================================
; rusty486 BIOS: graphics video services (included by bios.asm)
;
;   AH=00h AL=13h     320x200x256 (mode 13h)
;   AH=0Ch / 0Dh      write / read pixel (mode 13h and 8-bit VBE modes)
;   AH=10h            DAC palette (AL = 10h, 12h, 15h, 17h)
;   AX=1130h          font information (8x8 ROM font)
;   AH=4Fh            VESA BIOS Extensions 2.0 (00h-09h)
;
; The card is driven through Bochs-style DISPI registers (1CEh/1CFh).
; =============================================================================

%define DISPI_INDEX  0x1CE
%define DISPI_DATA   0x1CF


; Write DISPI register AX with value BX.
dispi_write:
        push dx
        mov dx, DISPI_INDEX
        out dx, ax
        mov dx, DISPI_DATA
        xchg ax, bx
        out dx, ax
        xchg ax, bx
        pop dx
        ret

; Read DISPI register AX -> AX.
dispi_read:
        push dx
        mov dx, DISPI_INDEX
        out dx, ax
        mov dx, DISPI_DATA
        in ax, dx
        pop dx
        ret

; Program a graphics mode: CX = width, DX = height, SI = bits per pixel,
; DI = enable flags (1 = on, 40h = linear framebuffer, 80h = don't clear).
dispi_mode:
        push ax
        push bx
        mov ax, 4
        xor bx, bx
        call dispi_write                ; disable while reprogramming
        mov ax, 1
        mov bx, cx
        call dispi_write
        mov ax, 2
        mov bx, dx
        call dispi_write
        mov ax, 3
        mov bx, si
        call dispi_write
        mov ax, 4
        mov bx, di
        call dispi_write
        pop bx
        pop ax
        ret

dispi_off:
        push ax
        push bx
        mov ax, 4
        xor bx, bx
        call dispi_write
        pop bx
        pop ax
        ret

load_default_palette:
        push ax
        push cx
        push dx
        push si
        mov dx, 0x3C8
        xor al, al
        out dx, al
        inc dx
        mov si, default_palette
        mov cx, 768
.l:     cs lodsb
        out dx, al
        loop .l
        pop si
        pop dx
        pop cx
        pop ax
        ret

; Enter mode 13h (DS = BDA). AH bit 7 = don't clear.
set_mode13:
        push cx
        push dx
        push si
        push di
        mov word [BDA_COLS], 40
        mov byte [BDA_ROWS], 24
        mov word [BDA_PAGESZ], 64000
        mov word [VBE_MODE], 0x13
        mov cx, 320
        mov dx, 200
        mov si, 8
        mov di, 0x41
        test ah, 0x80
        jz .clr
        or di, 0x80
.clr:   call dispi_mode
        call load_default_palette
        pop di
        pop si
        pop dx
        pop cx
        ret

; ---------------------------------------------------------------- mode 13h text

; Draw character AL in colour BL at the BDA cursor (mode 13h, 40x25 cells).
gfx_char:
        pusha
        push es
        movzx si, al
        shl si, 3
        add si, font8x8
        movzx ax, byte [BDA_CURPOS+1]   ; row
        mov cx, 2560
        mul cx
        movzx cx, byte [BDA_CURPOS]     ; column
        shl cx, 3
        add ax, cx
        mov di, ax
        mov ax, 0xA000
        mov es, ax
        mov dh, 8
.row:   mov dl, [cs:si]
        inc si
        mov cx, 8
.col:   xor al, al
        test dl, 1
        jz .bg
        mov al, bl
.bg:    mov [es:di], al
        shr dl, 1
        inc di
        loop .col
        add di, 320 - 8
        dec dh
        jnz .row
        pop es
        popa
        ret

; Scroll the mode 13h screen up one text row (8 pixels).
gfx_scroll:
        pusha
        push ds
        push es
        mov ax, 0xA000
        mov ds, ax
        mov es, ax
        cld
        xor di, di
        mov si, 2560
        mov cx, (64000 - 2560) / 4
        rep movsd
        xor eax, eax
        mov cx, 2560 / 4
        rep stosd
        pop es
        pop ds
        popa
        ret

; ---------------------------------------------------------------- pixels

; Offset and bank of pixel (CX, DX) in an 8-bit mode -> ES:DI, bank set.
pixel_addr:
        push eax
        push ebx
        push edx
        mov ax, 6                       ; virtual width = bytes per line (8 bpp)
        call dispi_read
        movzx eax, ax
        movzx edx, dx
        mul edx
        movzx ebx, cx
        add eax, ebx
        mov di, ax
        shr eax, 16
        mov bx, ax
        mov ax, 5
        call dispi_write                ; bank
        mov ax, 0xA000
        mov es, ax
        pop edx
        pop ebx
        pop eax
        ret

v_putpixel:
        push es
        push di
        push ax
        call pixel_addr
        pop ax
        mov [es:di], al
        pop di
        pop es
        iret

v_getpixel:
        push es
        push di
        call pixel_addr
        mov al, [es:di]
        pop di
        pop es
        iret

; ---------------------------------------------------------------- palette

v_palette:
        push dx
        push cx
        push bx
        cmp al, 0x10
        je .set1
        cmp al, 0x12
        je .setblock
        cmp al, 0x15
        je .get1
        cmp al, 0x17
        je .getblock
        jmp .r
.set1:  mov ah, dh                      ; BX = index, DH = R, CH = G, CL = B
        mov al, bl
        mov dx, 0x3C8
        out dx, al
        inc dx
        mov al, ah
        out dx, al
        mov al, ch
        out dx, al
        mov al, cl
        out dx, al
        jmp .r
.setblock:                              ; BX = first, CX = count, ES:DX = RGB data
        push si
        mov si, dx
        mov al, bl
        mov dx, 0x3C8
        out dx, al
        inc dx
        mov bx, cx
        add cx, cx
        add cx, bx
.sb:    mov al, [es:si]
        out dx, al
        inc si
        loop .sb
        pop si
        jmp .r
.get1:  pop bx                          ; -> DH = R, CH = G, CL = B
        pop cx
        pop dx
        push ax
        push dx                         ; keep DL
        mov al, bl
        mov dx, 0x3C7
        out dx, al
        mov dx, 0x3C9
        in al, dx
        mov ah, al
        in al, dx
        mov ch, al
        in al, dx
        mov cl, al
        pop dx
        mov dh, ah
        pop ax
        iret
.getblock:                              ; BX = first, CX = count, ES:DX = buffer
        push di
        mov di, dx
        mov al, bl
        mov dx, 0x3C7
        out dx, al
        mov dx, 0x3C9
        mov bx, cx
        add cx, cx
        add cx, bx
.gb:    in al, dx
        mov [es:di], al
        inc di
        loop .gb
        pop di
.r:     pop bx
        pop cx
        pop dx
        iret

; AX=1130h: ES:BP -> 8x8 font, CX = bytes per character, DL = rows - 1.
v_fontinfo:
        cmp al, 0x30
        jne .r
        push cs
        pop es
        mov bp, font8x8
        mov cx, 8
        mov dl, 24
.r:     iret

; ---------------------------------------------------------------- VBE

vbe_ok:
        mov ax, 0x004F
        iret
vbe_fail:
        mov ax, 0x014F
        iret

v_vbe:  cmp al, 0x00
        je vbe_info
        cmp al, 0x01
        je vbe_modeinfo
        cmp al, 0x02
        je vbe_setmode
        cmp al, 0x03
        je vbe_getmode
        cmp al, 0x05
        je vbe_bank
        cmp al, 0x06
        je vbe_scanline
        cmp al, 0x07
        je vbe_start
        cmp al, 0x08
        je vbe_dacwidth
        cmp al, 0x09
        je vbe_setpal
        jmp vbe_fail

; 4F00h: controller information at ES:DI.
vbe_info:
        mov dword [es:di], 'VESA'
        mov word [es:di+4], 0x0200
        mov word [es:di+6], vbe_oem
        mov word [es:di+8], 0xF000
        mov dword [es:di+10], 0
        mov word [es:di+14], vbe_modes
        mov word [es:di+16], 0xF000
        mov word [es:di+18], 32         ; 2 MB in 64 KB blocks
        jmp vbe_ok

; Find mode BX in the table -> SI (CS-relative). CF if unknown.
vbe_find:
        push bx
        and bx, 0x1FF
        mov si, vbe_table
.l:     cmp word [cs:si], 0xFFFF
        je .no
        cmp [cs:si], bx
        je .yes
        add si, 8
        jmp .l
.yes:   pop bx
        clc
        ret
.no:    pop bx
        stc
        ret

; 4F01h: mode CX information at ES:DI.
vbe_modeinfo:
        push bx
        push cx
        push si
        push di
        mov bx, cx
        call vbe_find
        jc .fail
        push di
        push cx
        mov cx, 128
        xor ax, ax
        rep stosw
        pop cx
        pop di
        mov word [es:di+0x00], 0x009B   ; supported, colour, graphics, LFB
        mov byte [es:di+0x02], 0x07     ; window A: exists, readable, writable
        mov word [es:di+0x04], 64       ; granularity (KB)
        mov word [es:di+0x06], 64       ; window size (KB)
        mov word [es:di+0x08], 0xA000
        mov ax, [cs:si+2]               ; width
        mov [es:di+0x12], ax
        mov bx, [cs:si+4]               ; height
        mov [es:di+0x14], bx
        mov byte [es:di+0x16], 8
        mov byte [es:di+0x17], 16
        mov byte [es:di+0x18], 1        ; planes
        mov cl, [cs:si+6]               ; bits per pixel
        mov [es:di+0x19], cl
        mov byte [es:di+0x1A], 1        ; banks
        mov byte [es:di+0x1D], 0        ; image pages - 1
        mov byte [es:di+0x1E], 1
        ; bytes per scanline = width * bytes per pixel
        mov bx, ax
        cmp cl, 8
        je .bpl
        shl bx, 1
        cmp cl, 16
        jbe .bpl
        shl bx, 1
.bpl:   mov [es:di+0x10], bx
        mov [es:di+0x32], bx            ; linear bytes per scanline (VBE 3)
        mov byte [es:di+0x1B], 4        ; packed pixel
        cmp cl, 8
        je .masks_done
        mov byte [es:di+0x1B], 6        ; direct colour
        cmp cl, 15
        je .m15
        cmp cl, 16
        je .m16
        mov dword [es:di+0x1F], 0x08081008  ; R 8@16, G 8@8 (size, position)
        mov dword [es:di+0x23], 0x18080008  ; B 8@0, reserved 8@24
        jmp .masks_done
.m15:   mov dword [es:di+0x1F], 0x05050A05  ; R 5@10, G 5@5
        mov dword [es:di+0x23], 0x0F010005  ; B 5@0, reserved 1@15
        jmp .masks_done
.m16:   mov dword [es:di+0x1F], 0x05060B05  ; R 5@11, G 6@5 (size, position)
        mov dword [es:di+0x23], 0x00000005  ; B 5@0
.masks_done:
        mov dword [es:di+0x28], 0xE0000000 ; linear framebuffer
        pop di
        pop si
        pop cx
        pop bx
        jmp vbe_ok
.fail:  pop di
        pop si
        pop cx
        pop bx
        jmp vbe_fail

; 4F02h: set mode BX (bit 14 = linear framebuffer, bit 15 = don't clear).
vbe_setmode:
        push ds
        push cx
        push dx
        push si
        push di
        push bx
        mov ax, BDA
        mov ds, ax
        mov ax, bx
        and ax, 0x1FF
        cmp ax, 0x100
        jae .vesa
        ; A standard mode via AH=00h.
        test bh, 0x80
        jz .std
        or al, 0x80
.std:   xor ah, ah
        int 0x10
        pop bx
        jmp .ok
.vesa:  call vbe_find
        jc .bad
        mov cx, [cs:si+2]
        mov dx, [cs:si+4]
        movzx si, byte [cs:si+6]
        mov di, 0x01
        test bh, 0x40
        jz .nolfb
        or di, 0x40
.nolfb: test bh, 0x80
        jz .clr
        or di, 0x80
.clr:   call dispi_mode
        cmp si, 8
        jne .nopal
        call load_default_palette
.nopal: pop bx
        mov [VBE_MODE], bx
        mov byte [BDA_MODE], 0xFF       ; no BIOS text output in VBE modes
.ok:    pop di
        pop si
        pop dx
        pop cx
        pop ds
        jmp vbe_ok
.bad:   pop bx
        pop di
        pop si
        pop dx
        pop cx
        pop ds
        jmp vbe_fail

vbe_getmode:
        push ds
        mov bx, BDA
        mov ds, bx
        mov bx, [VBE_MODE]
        pop ds
        jmp vbe_ok

; 4F05h: BH = 0 set bank DX, BH = 1 get bank -> DX.
vbe_bank:
        push bx
        test bh, bh
        jnz .get
        mov ax, 5
        mov bx, dx
        call dispi_write
        pop bx
        jmp vbe_ok
.get:   mov ax, 5
        call dispi_read
        mov dx, ax
        pop bx
        jmp vbe_ok

; 4F06h: BL = 0 set line length CX pixels, 1 get -> BX bytes, CX pixels, DX lines.
vbe_scanline:
        cmp bl, 0
        jne .get
        push bx
        mov ax, 6
        mov bx, cx
        call dispi_write
        pop bx
.get:   mov ax, 6
        call dispi_read
        mov cx, ax
        mov bx, ax
        push cx
        mov ax, 3
        call dispi_read
        pop cx
        cmp ax, 8
        je .b
        shl bx, 1
        cmp ax, 16
        jbe .b
        shl bx, 1
.b:     mov dx, 480
        jmp vbe_ok

; 4F07h: BL = 0/80h set display start CX, DX; 1 get.
vbe_start:
        test bl, 0x7F
        jnz .get
        push bx
        mov ax, 8
        mov bx, cx
        call dispi_write
        mov ax, 9
        mov bx, dx
        call dispi_write
        pop bx
        jmp vbe_ok
.get:   mov ax, 8
        call dispi_read
        mov cx, ax
        mov ax, 9
        call dispi_read
        mov dx, ax
        xor bh, bh
        jmp vbe_ok

; 4F08h: DAC width is fixed at 6 bits.
vbe_dacwidth:
        mov bh, 6
        jmp vbe_ok

; 4F09h: BL = 0 set palette: CX entries from DX, ES:DI = B, G, R, pad.
vbe_setpal:
        test bl, 0x7F
        jnz vbe_fail
        push cx
        push dx
        push di
        mov al, dl
        mov dx, 0x3C8
        out dx, al
        inc dx
.p:     mov al, [es:di+2]
        out dx, al
        mov al, [es:di+1]
        out dx, al
        mov al, [es:di]
        out dx, al
        add di, 4
        loop .p
        pop di
        pop dx
        pop cx
        jmp vbe_ok

; ---------------------------------------------------------------- data

vbe_oem db 'rusty486 VBE', 0
; Mode list for 4F00h.
vbe_modes:
        dw 0x100, 0x101, 0x10D, 0x10E, 0x10F, 0x110, 0x111, 0x112, 0xFFFF
; mode, width, height, bits per pixel
vbe_table:
        dw 0x013, 320, 200
        db 8, 0
        dw 0x100, 640, 400
        db 8, 0
        dw 0x101, 640, 480
        db 8, 0
        dw 0x10D, 320, 200
        db 15, 0
        dw 0x10E, 320, 200
        db 16, 0
        dw 0x10F, 320, 200
        db 32, 0
        dw 0x110, 640, 480
        db 15, 0
        dw 0x111, 640, 480
        db 16, 0
        dw 0x112, 640, 480
        db 32, 0
        dw 0xFFFF

font8x8:
%include "font8x8.inc"
default_palette:
%include "palette.inc"
