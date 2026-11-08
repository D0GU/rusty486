; =============================================================================
; TinyDOS mouse driver (INT 33h) on the rusty486 input device.
; Coordinates use the classic 640x200 virtual screen in text mode.
; =============================================================================

%define MOUSE_X   0x32A
%define MOUSE_Y   0x32C
%define MOUSE_BTN 0x32E

int33:
        push ds
        push cs
        pop ds
        cmp ax, 0x00
        je .reset
        cmp ax, 0x21
        je .reset
        cmp ax, 0x01
        je .show
        cmp ax, 0x02
        je .hide
        cmp ax, 0x03
        je .get
        cmp ax, 0x05
        je .press
        cmp ax, 0x06
        je .press
        cmp ax, 0x07
        je .xrange
        cmp ax, 0x08
        je .yrange
        cmp ax, 0x0B
        je .motion
        pop ds
        iret
.reset: mov word [m_xmin], 0
        mov word [m_xmax], 639
        mov word [m_ymin], 0
        mov word [m_ymax], 199
        mov ax, 0xFFFF
        mov bx, 2
        pop ds
        iret
.show:  mov byte [m_visible], 1
        call mouse_draw
        pop ds
        iret
.hide:  call mouse_undraw
        mov byte [m_visible], 0
        pop ds
        iret
.get:   call mouse_pos
        pop ds
        iret
.press: call mouse_pos                  ; no press counting: report current state
        xor bx, bx
        pop ds
        iret
.xrange:
        mov [m_xmin], cx
        mov [m_xmax], dx
        pop ds
        iret
.yrange:
        mov [m_ymin], cx
        mov [m_ymax], dx
        pop ds
        iret
.motion:
        xor cx, cx
        xor dx, dx
        pop ds
        iret

; BX = buttons, CX = x, DX = y (clamped to the ranges).
mouse_pos:
        push ax
        push dx
        mov dx, MOUSE_BTN
        in al, dx
        movzx bx, al
        mov dx, MOUSE_X
        in ax, dx
        mov cx, ax
        mov dx, MOUSE_Y
        in ax, dx
        shr ax, 1                       ; 400 lines -> 200
        mov dx, ax
        cmp cx, [m_xmin]
        jge .x1
        mov cx, [m_xmin]
.x1:    cmp cx, [m_xmax]
        jle .x2
        mov cx, [m_xmax]
.x2:    cmp dx, [m_ymin]
        jge .y1
        mov dx, [m_ymin]
.y1:    cmp dx, [m_ymax]
        jle .y2
        mov dx, [m_ymax]
.y2:    pop ax                          ; discard saved DX
        pop ax
        ret

; Text-mode cursor: invert the attribute of the cell under the mouse.
mouse_draw:
        cmp byte [m_visible], 0
        je .r
        pusha
        push es
        push bx
        call mouse_pos
        pop bx
        shr cx, 3                       ; column
        shr dx, 3                       ; row (200 / 8 = 25)
        mov ax, dx
        mov bl, 80
        mul bl
        add ax, cx
        shl ax, 1
        inc ax
        mov di, ax
        mov ax, 0xB800
        mov es, ax
        mov [m_cell], di
        mov al, [es:di]
        mov [m_saved], al
        xor al, 0x77
        mov [es:di], al
        mov byte [m_drawn], 1
        pop es
        popa
.r:     ret

mouse_undraw:
        cmp byte [m_drawn], 0
        je .r
        push ax
        push di
        push es
        mov ax, 0xB800
        mov es, ax
        mov di, [m_cell]
        mov al, [m_saved]
        mov [es:di], al
        mov byte [m_drawn], 0
        pop es
        pop di
        pop ax
.r:     ret

; Timer hook (INT 1Ch): keep the text cursor under the mouse.
mouse_tick:
        push ds
        push cs
        pop ds
        cmp byte [m_visible], 0
        je .chain
        call mouse_undraw
        call mouse_draw
.chain: pop ds
        jmp far [cs:old_1c]

m_visible db 0
m_drawn   db 0
m_cell    dw 0
m_saved   db 0
m_xmin    dw 0
m_xmax    dw 639
m_ymin    dw 0
m_ymax    dw 199
old_1c    dd 0
