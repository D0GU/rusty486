; =============================================================================
; rusty486 BIOS
;
; A small PC-compatible BIOS for the rusty486 fantasy 486. Assembled as a flat
; 64 KB ROM mapped at F000:0000. Services so far:
;   INT 10h  video (80x25 colour text)       INT 13h  disk (CHS + LBA extensions)
;   INT 11h  equipment list                   INT 15h  system services (partial)
;   INT 12h  conventional memory size         INT 16h  keyboard (stub until M3)
;   INT 18h  no-boot handler                  INT 19h  bootstrap loader
;   INT 1Ah  time of day (stub)
;
; Every character printed through INT 10h is mirrored to debug port E9h, so
; headless tests and the host log can see screen output.
; =============================================================================

        bits 16
        cpu 486
        org 0

%define RDC          0x300          ; rusty486 disk controller base port
%define RDC_CMD      RDC+0x00
%define RDC_DRIVE    RDC+0x01
%define RDC_LBA      RDC+0x02
%define RDC_COUNT    RDC+0x06
%define RDC_ADDR     RDC+0x08
%define RDC_ERROR    RDC+0x0C
%define RDC_TOTAL    RDC+0x10
%define RDC_CYLS     RDC+0x14
%define RDC_HEADS    RDC+0x16
%define RDC_SPT      RDC+0x17
%define CMD_READ     1
%define CMD_WRITE    2

%define INPUT        0x320          ; rusty486 input device
%define INPUT_COUNT  INPUT+0
%define INPUT_TYPE   INPUT+1        ; read: head event type, write: pop
%define INPUT_A      INPUT+2
%define INPUT_B      INPUT+3
%define INPUT_C      INPUT+4
%define INPUT_D      INPUT+5
%define INPUT_IRQ    INPUT+6

%define DEBUG_PORT   0xE9
%define CRTC         0x3D4
%define TEXT_SEG     0xB800
%define BDA          0x40           ; BIOS data area segment

; BIOS data area offsets
%define BDA_EQUIP    0x10
%define BDA_MEMKB    0x13
%define BDA_SHIFT    0x17           ; keyboard shift flags
%define BDA_KBHEAD   0x1A           ; keyboard buffer head/tail (offsets in BDA)
%define BDA_KBTAIL   0x1C
%define BDA_MIDNIGHT 0x70
%define BDA_KBSTART  0x80
%define BDA_KBEND    0x82
%define BDA_DISKST   0x41           ; last floppy status
%define BDA_MODE     0x49
%define BDA_COLS     0x4A
%define BDA_PAGESZ   0x4C
%define BDA_CURPOS   0x50           ; page 0: column, row
%define BDA_CURSHAPE 0x60
%define BDA_CRTC     0x63
%define BDA_TICKS    0x6C
%define BDA_HDST     0x74           ; last hard disk status
%define BDA_NHD      0x75           ; number of hard disks
%define BDA_ROWS     0x84
%define BDA_CHARH    0x85

; =============================================================================
; Power-on self test
; =============================================================================
start:
        cli
        cld
        xor ax, ax
        mov ss, ax
        mov sp, 0x7C00

        ; Clear the IVT and BIOS data area (0000:0000 - 0000:04FF).
        mov es, ax
        xor di, di
        mov cx, 0x500 / 2
        rep stosw

        ; Point every vector at a bare IRET, then install our handlers.
        xor di, di
        mov cx, 256
.ivt:   mov word [es:di], dummy_iret
        mov word [es:di+2], 0xF000
        add di, 4
        loop .ivt

%macro VECTOR 2
        mov word [es:%1*4], %2
%endmacro
        ; Hardware IRQs: unexpected ones just acknowledge the PIC.
        mov di, 0x0A * 4
        mov cx, 6
.irqm:  mov word [es:di], irq_master
        add di, 4
        loop .irqm
        mov di, 0x70 * 4
        mov cx, 8
.irqs:  mov word [es:di], irq_slave
        add di, 4
        loop .irqs

        VECTOR 0x08, int08
        VECTOR 0x09, int09
        VECTOR 0x10, int10
        VECTOR 0x11, int11
        VECTOR 0x12, int12
        VECTOR 0x13, int13
        VECTOR 0x15, int15
        VECTOR 0x16, int16
        VECTOR 0x18, int18
        VECTOR 0x19, int19
        VECTOR 0x1A, int1a
        VECTOR 0x1E, floppy_dpt

        ; BIOS data area.
        mov ax, BDA
        mov ds, ax
        mov word [BDA_EQUIP], 0x0021    ; 1 floppy drive, 80x25 colour
        mov word [BDA_MEMKB], 640
        mov word [BDA_CRTC], CRTC
        mov byte [BDA_CHARH], 16

        ; Count hard disks attached to the disk controller.
        xor bl, bl
        mov dx, RDC_DRIVE
        mov al, 0x80
        out dx, al
        mov dx, RDC_CMD
        in al, dx
        test al, 0x80
        jz .nohd
        inc bl
        mov dx, RDC_DRIVE
        mov al, 0x81
        out dx, al
        mov dx, RDC_CMD
        in al, dx
        test al, 0x80
        jz .nohd
        inc bl
.nohd:  mov [BDA_NHD], bl

        ; Keyboard buffer: 16 words at 40:1E.
        mov word [BDA_KBSTART], 0x1E
        mov word [BDA_KBEND], 0x3E
        mov word [BDA_KBHEAD], 0x1E
        mov word [BDA_KBTAIL], 0x1E

        ; Interrupt controllers: master vectors 08h-0Fh, slave 70h-77h.
        mov al, 0x11
        out 0x20, al
        out 0xA0, al
        mov al, 0x08
        out 0x21, al
        mov al, 0x70
        out 0xA1, al
        mov al, 0x04
        out 0x21, al
        mov al, 0x02
        out 0xA1, al
        mov al, 0x01
        out 0x21, al
        out 0xA1, al
        mov al, 0xF8                    ; enable IRQ0 (timer), IRQ1 (input), IRQ2 (cascade)
        out 0x21, al
        mov al, 0xFF
        out 0xA1, al

        ; Timer channel 0: square wave, divisor 65536 -> 18.2 Hz.
        mov al, 0x36
        out 0x43, al
        xor al, al
        out 0x40, al
        out 0x40, al

        ; Input device: interrupt on events.
        mov dx, INPUT_IRQ
        mov al, 1
        out dx, al

        ; Time of day from the real-time clock.
        call rtc_to_ticks

        ; 486SX: no FPU, so make FPU instructions trap (#NM) for emulators.
        mov eax, cr0
        or eax, 0x04                    ; CR0.EM
        and eax, ~0x02                  ; CR0.MP
        mov cr0, eax

        ; Video mode 3 (also clears the screen and homes the cursor).
        mov ax, 0x0003
        int 0x10

        mov si, banner
        call print
        mov si, msg_mem
        call print

        sti
        int 0x19                        ; never returns

; Print the NUL-terminated string at CS:SI via INT 10h teletype.
print:
        push ax
        push bx
        push si
.next:  cs lodsb
        test al, al
        jz .done
        mov ah, 0x0E
        mov bx, 0x0007
        int 0x10
        jmp .next
.done:  pop si
        pop bx
        pop ax
        ret

banner  db 'rusty486 BIOS v0.1', 13, 10
        db 'Intel 486SX-compatible CPU @ 33 MHz', 13, 10, 0
msg_mem db '16384 KB RAM OK', 13, 10, 13, 10, 0

dummy_iret:
        iret

; Unexpected hardware interrupts: acknowledge and return.
irq_master:
        push ax
        mov al, 0x20
        out 0x20, al
        pop ax
        iret

irq_slave:
        push ax
        mov al, 0x20
        out 0xA0, al
        out 0x20, al
        pop ax
        iret

; Set the BDA tick count from the RTC's time of day. DS = BDA.
rtc_to_ticks:
        pushad
        mov al, 4                       ; hours
        call cmos_bin
        imul ebx, eax, 3600
        mov al, 2                       ; minutes
        call cmos_bin
        imul eax, eax, 60
        add ebx, eax
        mov al, 0                       ; seconds
        call cmos_bin
        add eax, ebx
        mov ecx, 1193182                ; ticks = seconds * 1193182 / 65536
        mul ecx
        shrd eax, edx, 16
        mov [BDA_TICKS], eax
        mov byte [BDA_MIDNIGHT], 0
        popad
        ret

; Read CMOS register AL as BCD -> binary in EAX.
cmos_bin:
        out 0x70, al
        in al, 0x71
        mov ah, al
        shr ah, 4
        and al, 0x0F
        aad
        movzx eax, al
        ret

; Read CMOS register AL -> AL (raw BCD).
cmos_read:
        out 0x70, al
        in al, 0x71
        ret

; =============================================================================
; INT 08h - timer tick (IRQ0, 18.2 Hz)
; =============================================================================
int08:
        push ds
        push eax
        mov ax, BDA
        mov ds, ax
        inc dword [BDA_TICKS]
        cmp dword [BDA_TICKS], 0x1800B0 ; ticks per day
        jb .same_day
        mov dword [BDA_TICKS], 0
        mov byte [BDA_MIDNIGHT], 1
.same_day:
        int 0x1C                        ; user timer hook
        mov al, 0x20
        out 0x20, al
        pop eax
        pop ds
        iret

; =============================================================================
; INT 09h - input device (IRQ1): move key presses into the BDA buffer
; =============================================================================
int09:
        push ds
        pusha
        mov ax, BDA
        mov ds, ax
.next:  mov dx, INPUT_COUNT
        in al, dx
        test al, al
        jz .done
        mov dx, INPUT_TYPE
        in al, dx
        cmp al, 1
        je .down
        cmp al, 2
        jne .pop                        ; mouse events are for INT 33h (TinyDOS)
        mov dx, INPUT_C                 ; key up: just track the shift flags
        in al, dx
        mov [BDA_SHIFT], al
        jmp .pop
.down:  mov dx, INPUT_C
        in al, dx
        mov [BDA_SHIFT], al
        mov dx, INPUT_D
        in al, dx
        test al, 2                      ; modifier keys don't produce keystrokes
        jnz .pop
        mov bl, al
        mov dx, INPUT_A
        in al, dx
        mov ah, al                      ; AH = scancode
        mov dx, INPUT_B
        in al, dx                       ; AL = ASCII
        test bl, 1
        jz .store
        test al, al
        jnz .store
        mov al, 0xE0                    ; enhanced-keyboard extended key
.store: call kb_put
.pop:   mov dx, INPUT_TYPE
        out dx, al
        jmp .next
.done:  mov al, 0x20
        out 0x20, al
        popa
        pop ds
        iret

; Append key word AX to the BDA keyboard buffer (dropped if full).
; DS = BDA. Clobbers BX, SI.
kb_put:
        mov bx, [BDA_KBTAIL]
        mov si, bx
        add si, 2
        cmp si, [BDA_KBEND]
        jb .nowrap
        mov si, [BDA_KBSTART]
.nowrap:
        cmp si, [BDA_KBHEAD]
        je .full
        mov [bx], ax
        mov [BDA_KBTAIL], si
.full:  ret

; =============================================================================
; INT 10h - video services
; =============================================================================
int10:
        sti
        cmp ah, 0x00
        je v_setmode
        cmp ah, 0x01
        je v_cursorshape
        cmp ah, 0x02
        je v_setcursor
        cmp ah, 0x03
        je v_getcursor
        cmp ah, 0x06
        je v_scrollup
        cmp ah, 0x07
        je v_scrolldown
        cmp ah, 0x08
        je v_readchar
        cmp ah, 0x09
        je v_writecharattr
        cmp ah, 0x0A
        je v_writechar
        cmp ah, 0x0E
        je v_teletype
        cmp ah, 0x0F
        je v_getmode
        cmp ah, 0x13
        je v_writestring
        iret

; AH=00h set video mode (AL bit 7 = don't clear). Only text mode 3 for now.
v_setmode:
        push ds
        push ax
        push bx
        push cx
        push dx
        mov bx, BDA
        mov ds, bx
        mov ah, al
        and al, 0x7F
        mov [BDA_MODE], al
        mov word [BDA_COLS], 80
        mov word [BDA_PAGESZ], 4000
        mov byte [BDA_ROWS], 24
        mov word [BDA_CURPOS], 0
        mov word [BDA_CURSHAPE], 0x0E0F
        test ah, 0x80
        jnz .noclear
        mov ax, 0x0600                  ; scroll whole screen = clear
        mov bh, 0x07
        xor cx, cx
        mov dx, 0x184F
        call scroll_up
.noclear:
        mov cx, 0x0E0F
        call set_cursor_shape
        call update_cursor
        pop dx
        pop cx
        pop bx
        pop ax
        pop ds
        iret

; AH=01h set cursor shape: CH = start line (bit 5 = hide), CL = end line.
v_cursorshape:
        push ds
        push ax
        mov ax, BDA
        mov ds, ax
        mov [BDA_CURSHAPE], cx
        call set_cursor_shape
        pop ax
        pop ds
        iret

; AH=02h set cursor position: DH = row, DL = column.
v_setcursor:
        push ds
        push ax
        mov ax, BDA
        mov ds, ax
        mov [BDA_CURPOS], dl
        mov [BDA_CURPOS+1], dh
        call update_cursor
        pop ax
        pop ds
        iret

; AH=03h get cursor: DH,DL = row,col; CH,CL = shape.
v_getcursor:
        push ds
        push ax
        mov ax, BDA
        mov ds, ax
        mov dl, [BDA_CURPOS]
        mov dh, [BDA_CURPOS+1]
        mov cx, [BDA_CURSHAPE]
        pop ax
        pop ds
        iret

; AH=06h scroll up / AH=07h scroll down a window.
v_scrollup:
        call scroll_up
        iret
v_scrolldown:
        call scroll_down
        iret

; AH=08h read character and attribute at cursor -> AL, AH.
v_readchar:
        push ds
        push bx
        call cursor_addr                ; DS:BX -> cell
        mov ax, [bx]
        pop bx
        pop ds
        iret

; AH=09h write char AL with attribute BL, CX times, at cursor (no move).
v_writecharattr:
        push ds
        push es
        push di
        push cx
        push ax
        push bx
        call cursor_addr
        mov di, bx
        pop bx
        push bx
        push ds
        pop es
        mov ah, bl
        rep stosw
        pop bx
        pop ax
        pop cx
        pop di
        pop es
        pop ds
        iret

; AH=0Ah write char AL, CX times, at cursor, keeping attributes.
v_writechar:
        push ds
        push bx
        push cx
        call cursor_addr
        jcxz .done
.loop:  mov [bx], al
        add bx, 2
        loop .loop
.done:  pop cx
        pop bx
        pop ds
        iret

; AH=0Eh teletype output of AL (keeps the cell's attribute).
v_teletype:
        push ds
        push bx
        call cursor_addr
        mov bl, [bx+1]                  ; existing attribute
        call put_char
        pop bx
        pop ds
        iret

; AH=0Fh get video mode: AL = mode, AH = columns, BH = page.
v_getmode:
        push ds
        mov ax, BDA
        mov ds, ax
        mov al, [BDA_MODE]
        mov ah, 80
        xor bh, bh
        pop ds
        iret

; AH=13h write string ES:BP, CX chars, at DH,DL. AL bit 0 = move cursor,
; bit 1 = string holds char/attr pairs (else attribute is BL).
v_writestring:
        pusha
        push ds
        mov si, BDA
        mov ds, si
        push word [BDA_CURPOS]          ; saved cursor
        mov [BDA_CURPOS], dl
        mov [BDA_CURPOS+1], dh
        mov si, bp
        mov ah, al                      ; AH = mode
        jcxz .end
.loop:  mov al, [es:si]
        inc si
        test ah, 2
        jz .attr_ok
        mov bl, [es:si]
        inc si
.attr_ok:
        call put_char
        loop .loop
.end:   pop dx                          ; saved cursor
        test ah, 1
        jnz .keep
        mov [BDA_CURPOS], dx
        call update_cursor
.keep:  pop ds
        popa
        iret

; -----------------------------------------------------------------------------
; Video helpers
; -----------------------------------------------------------------------------

; DS:BX -> text cell under the cursor. Clobbers DS, BX.
cursor_addr:
        push ax
        mov bx, BDA
        mov ds, bx
        mov al, [BDA_CURPOS+1]          ; row
        mov bl, 80
        mul bl
        add al, [BDA_CURPOS]            ; + column
        adc ah, 0
        shl ax, 1
        mov bx, ax
        mov ax, TEXT_SEG
        mov ds, ax
        pop ax
        ret

; Write AL with attribute BL at the cursor and advance, handling BEL, BS,
; LF and CR, wrapping and scrolling. Mirrors the character to the debug port.
put_char:
        push ds
        push ax
        push bx
        push cx
        push dx
        out DEBUG_PORT, al
        mov dx, BDA
        mov ds, dx
        mov dl, [BDA_CURPOS]            ; column
        mov dh, [BDA_CURPOS+1]          ; row
        cmp al, 7
        je .done
        cmp al, 8
        je .bs
        cmp al, 10
        je .lf
        cmp al, 13
        je .cr
        ; printable: store char + attribute
        mov cl, bl                      ; attribute (CX is saved above)
        call cursor_addr                ; DS:BX -> cell
        mov ah, cl
        mov [bx], ax
        mov ax, BDA
        mov ds, ax
        inc dl
        cmp dl, 80
        jb .save
        xor dl, dl
        jmp .lf
.bs:    test dl, dl
        jz .save
        dec dl
        jmp .save
.cr:    xor dl, dl
        jmp .save
.lf:    inc dh
        cmp dh, 25
        jb .save
        mov dh, 24
        push dx
        mov ax, 0x0601                  ; scroll up one line
        mov bh, 0x07
        xor cx, cx
        mov dx, 0x184F
        call scroll_up
        pop dx
.save:  mov [BDA_CURPOS], dl
        mov [BDA_CURPOS+1], dh
        call update_cursor
.done:  pop dx
        pop cx
        pop bx
        pop ax
        pop ds
        ret

; Program the CRTC cursor location from the BDA cursor position.
update_cursor:
        push ds
        push ax
        push bx
        push dx
        mov ax, BDA
        mov ds, ax
        mov al, [BDA_CURPOS+1]
        mov bl, 80
        mul bl
        add al, [BDA_CURPOS]
        adc ah, 0
        mov bx, ax
        mov dx, CRTC
        mov al, 0x0E
        out dx, al
        inc dx
        mov al, bh
        out dx, al
        dec dx
        mov al, 0x0F
        out dx, al
        inc dx
        mov al, bl
        out dx, al
        pop dx
        pop bx
        pop ax
        pop ds
        ret

; Program the CRTC cursor shape from CX (CH = start, CL = end).
set_cursor_shape:
        push ax
        push dx
        mov dx, CRTC
        mov al, 0x0A
        out dx, al
        inc dx
        mov al, ch
        out dx, al
        dec dx
        mov al, 0x0B
        out dx, al
        inc dx
        mov al, cl
        out dx, al
        pop dx
        pop ax
        ret

; Scroll a window up. AL = lines (0 = clear), BH = fill attribute,
; CH,CL = top row,col; DH,DL = bottom row,col.
scroll_up:
        pusha
        push ds
        push es
        mov si, TEXT_SEG
        mov ds, si
        mov es, si
        cld
        mov bl, dh
        sub bl, ch
        inc bl                          ; BL = window height
        test al, al
        jz .all
        cmp al, bl
        jbe .ok
.all:   mov al, bl
.ok:    xor ah, ah
        mov si, ax                      ; save n
        mov al, dl
        sub al, cl
        inc al
        mov bp, ax                      ; BP = width (AH is 0)
        mov ax, si                      ; AL = n
        mov dl, ch                      ; DL = current row
.row:   cmp dl, dh
        ja .done
        push ax
        mov al, dl
        call cell_offset                ; AX = offset of (row DL, col CL)
        mov di, ax
        pop ax
        push ax
        add al, dl                      ; source row
        cmp al, dh
        ja .blank
        call cell_offset
        mov si, ax
        push cx
        mov cx, bp
        rep movsw
        pop cx
        pop ax
        jmp .next
.blank: push cx
        mov cx, bp
        mov ah, bh
        mov al, ' '
        rep stosw
        pop cx
        pop ax
.next:  inc dl
        jmp .row
.done:  pop es
        pop ds
        popa
        ret

; Scroll a window down. Same registers as scroll_up.
scroll_down:
        pusha
        push ds
        push es
        mov si, TEXT_SEG
        mov ds, si
        mov es, si
        cld
        mov bl, dh
        sub bl, ch
        inc bl                          ; BL = height = rows to process
        test al, al
        jz .all
        cmp al, bl
        jbe .ok
.all:   mov al, bl
.ok:    xor ah, ah
        mov si, ax
        mov al, dl
        sub al, cl
        inc al
        mov bp, ax                      ; BP = width
        mov ax, si                      ; AL = n
        mov dl, dh                      ; DL = current row (bottom up)
.row:   test bl, bl
        jz .done
        push ax
        mov al, dl
        call cell_offset
        mov di, ax
        pop ax
        push ax
        mov ah, dl
        sub ah, al                      ; source row = row - n
        jb .blank
        cmp ah, ch
        jb .blank
        mov al, ah
        call cell_offset
        mov si, ax
        push cx
        mov cx, bp
        rep movsw
        pop cx
        pop ax
        jmp .next
.blank: push cx
        mov cx, bp
        mov ah, bh
        mov al, ' '
        rep stosw
        pop cx
        pop ax
.next:  dec dl
        dec bl
        jmp .row
.done:  pop es
        pop ds
        popa
        ret

; AX = byte offset of the cell at row AL, column CL.
cell_offset:
        push bx
        mov ah, 80
        mul ah
        xor bh, bh
        mov bl, cl
        add ax, bx
        shl ax, 1
        pop bx
        ret

; =============================================================================
; INT 11h / 12h - equipment and memory size
; =============================================================================
int11:
        push ds
        mov ax, BDA
        mov ds, ax
        mov ax, [BDA_EQUIP]
        pop ds
        iret

int12:
        push ds
        mov ax, BDA
        mov ds, ax
        mov ax, [BDA_MEMKB]
        pop ds
        iret

; =============================================================================
; INT 13h - disk services, backed by the rusty486 disk controller
; =============================================================================
int13:
        sti
        cmp ah, 0x00
        je d_ok                         ; reset
        cmp ah, 0x01
        je d_status
        cmp ah, 0x02
        je d_read
        cmp ah, 0x03
        je d_write
        cmp ah, 0x04
        je d_ok                         ; verify
        cmp ah, 0x08
        je d_params
        cmp ah, 0x15
        je d_type
        cmp ah, 0x41
        je d_extcheck
        cmp ah, 0x42
        je d_extread
        cmp ah, 0x43
        je d_extwrite
        mov ah, 0x01                    ; invalid function
        jmp d_fail

d_ok:   xor ah, ah
d_done: ; Store status in the BDA, set CF from AH, return keeping flags.
        push ds
        push bx
        mov bx, BDA
        mov ds, bx
        mov bx, BDA_DISKST
        test dl, 0x80
        jz .fd
        mov bx, BDA_HDST
.fd:    mov [bx], ah
        pop bx
        pop ds
        test ah, ah
        jnz .err
        clc
        retf 2
.err:   stc
        retf 2

d_fail: jmp d_done

d_status:
        push ds
        push bx
        mov bx, BDA
        mov ds, bx
        mov al, [BDA_DISKST]
        test dl, 0x80
        jz .fd
        mov al, [BDA_HDST]
.fd:    pop bx
        pop ds
        xor ah, ah
        clc
        retf 2

; Select drive DL on the controller. Returns ZF=1 if no disk is present.
select_drive:
        push ax
        push dx
        mov al, dl
        mov dx, RDC_DRIVE
        out dx, al
        mov dx, RDC_CMD
        in al, dx
        test al, 0x80
        pop dx
        pop ax
        ret

; Issue a transfer. EAX = LBA, CX = sector count, EDI = physical address,
; BL = command, DL = drive. Returns AH = status (0 = ok).
disk_xfer:
        push dx
        push eax
        mov al, dl
        mov dx, RDC_DRIVE
        out dx, al
        pop eax
        mov dx, RDC_LBA
        out dx, eax
        mov dx, RDC_COUNT
        mov ax, cx
        out dx, ax
        mov dx, RDC_ADDR
        mov eax, edi
        out dx, eax
        mov dx, RDC_CMD
        mov al, bl
        out dx, al
        mov dx, RDC_ERROR
        in al, dx
        mov ah, al
        pop dx
        ret

; Convert CHS (CH, CL, DH) on drive DL to an LBA in EAX.
chs_to_lba:
        push ebx
        push ecx
        push edx
        call select_drive
        movzx ebx, cl
        and bl, 0x3F
        dec bx                          ; EBX = sector - 1
        push ebx
        movzx eax, cl
        shl ax, 2
        and ax, 0x300
        mov al, ch                      ; EAX = cylinder
        movzx ebx, dh                   ; EBX = head
        push eax
        mov dx, RDC_HEADS
        in al, dx
        movzx ecx, al                   ; ECX = heads
        pop eax
        mul ecx                         ; EAX = cyl * heads (EDX clobbered)
        add eax, ebx
        push eax
        mov dx, RDC_SPT
        in al, dx
        movzx ecx, al                   ; ECX = sectors per track
        pop eax
        mul ecx
        pop ebx
        add eax, ebx
        pop edx
        pop ecx
        pop ebx
        ret

; AH=02h/03h CHS read/write: AL = count, CH/CL/DH = CHS, ES:BX = buffer.
d_read:
        push bx
        mov bl, CMD_READ
        jmp d_chs
d_write:
        push bx
        mov bl, CMD_WRITE
d_chs:  call select_drive
        jz .nodrive
        push eax
        push ecx
        push edi
        movzx edi, word [esp+12]        ; caller's BX (buffer offset)
        push eax
        xor eax, eax
        mov ax, es
        shl eax, 4
        add edi, eax                    ; EDI = physical buffer address
        pop eax
        push ax
        call chs_to_lba
        pop cx
        xor ch, ch                      ; CX = sector count (from AL)
        push cx
        call disk_xfer
        pop cx
        mov al, cl                      ; AL = sectors transferred
        test ah, ah
        jz .ok
        xor al, al
.ok:    pop edi
        pop ecx
        mov [esp], ax                   ; patch AL (count) and AH (status) of the saved EAX
        pop eax
        pop bx
        jmp d_done
.nodrive:
        pop bx
        mov ah, 0x80
        jmp d_done

; AH=08h get drive parameters.
d_params:
        call select_drive
        jz .nodrive
        push ax
        push dx
        mov dx, RDC_CYLS
        in ax, dx
        dec ax                          ; max cylinder
        mov ch, al
        mov cl, ah
        shl cl, 6
        mov dx, RDC_SPT
        in al, dx
        or cl, al
        mov dx, RDC_HEADS
        in al, dx
        dec al
        mov bh, al                      ; temp: max head
        pop dx
        pop ax
        mov dh, bh
        test dl, 0x80
        jnz .hd
        mov bl, 0x04                    ; 1.44 MB drive type
        mov dl, 1                       ; one floppy drive
        mov di, floppy_dpt
        push cs
        pop es
        xor bh, bh
        jmp d_ok
.hd:    push ds
        mov ax, BDA
        mov ds, ax
        mov dl, [BDA_NHD]
        pop ds
        xor bx, bx
        jmp d_ok
.nodrive:
        mov ah, 0x07
        jmp d_done

; AH=15h get disk type.
d_type:
        call select_drive
        jz .none
        test dl, 0x80
        jnz .hd
        mov ah, 0x01                    ; floppy without change-line
        clc
        retf 2
.hd:    push ax
        push dx
        mov dx, RDC_TOTAL
        in ax, dx
        mov bx, ax
        add dx, 2
        in ax, dx
        mov cx, ax
        pop dx
        pop ax
        mov dx, bx                      ; CX:DX = sector count
        mov ah, 0x03
        clc
        retf 2
.none:  xor ah, ah
        clc
        retf 2

; AH=41h check for LBA extensions.
d_extcheck:
        cmp bx, 0x55AA
        jne .no
        mov bx, 0xAA55
        mov ah, 0x21                    ; EDD 1.1
        mov cx, 0x0001                  ; fixed disk access subset
        clc
        retf 2
.no:    mov ah, 0x01
        jmp d_done

; AH=42h/43h extended read/write: DS:SI -> disk address packet.
d_extread:
        push bx
        mov bl, CMD_READ
        jmp d_ext
d_extwrite:
        push bx
        mov bl, CMD_WRITE
d_ext:  call select_drive
        jz .nodrive
        push eax
        push ecx
        push edi
        movzx edi, word [si+4]          ; buffer offset
        movzx eax, word [si+6]          ; buffer segment
        shl eax, 4
        add edi, eax
        mov cx, [si+2]                  ; count
        mov eax, [si+8]                 ; LBA (low 32 bits)
        call disk_xfer
        pop edi
        pop ecx
        mov [esp], ah                   ; keep status out of the restore
        pop eax
        mov ah, al                      ; AH = status (AL was clobbered anyway)
        pop bx
        jmp d_done
.nodrive:
        pop bx
        mov ah, 0x80
        jmp d_done

; =============================================================================
; INT 15h - system services
; =============================================================================
int15:
        sti
        cmp ah, 0x88
        je .extmem
        cmp ah, 0x86
        je .wait
        cmp ax, 0x2400
        je .a20off
        cmp ax, 0x2401
        je .a20on
        cmp ax, 0x2402
        je .a20status
        cmp ax, 0x2403
        je .a20support
        mov ah, 0x86                    ; unsupported
        stc
        retf 2
.extmem:
        mov ax, 15360                   ; KB above 1 MB (16 MB total)
        clc
        retf 2
; AH=86h: wait CX:DX microseconds (to the nearest 55 ms timer tick).
.wait:  push ds
        push eax
        push ebx
        push edx
        movzx eax, cx
        shl eax, 16
        mov ax, dx
        xor edx, edx
        mov ebx, 54925
        div ebx
        inc eax
        mov bx, BDA
        mov ds, bx
        add eax, [BDA_TICKS]
.tick:  hlt
        cmp [BDA_TICKS], eax
        jb .tick
        pop edx
        pop ebx
        pop eax
        pop ds
        clc
        retf 2
.a20off:
        in al, 0x92
        and al, ~0x02
        out 0x92, al
        xor ah, ah
        clc
        retf 2
.a20on:
        in al, 0x92
        or al, 0x02
        out 0x92, al
        xor ah, ah
        clc
        retf 2
.a20status:
        in al, 0x92
        shr al, 1
        and al, 1
        xor ah, ah
        clc
        retf 2
.a20support:
        mov bx, 0x0002                  ; fast A20 via port 92h
        xor ah, ah
        clc
        retf 2

; =============================================================================
; INT 16h - keyboard services (buffer filled by INT 09h)
; =============================================================================
int16:
        sti
        push ds
        push si
        mov si, BDA
        mov ds, si
        cmp ah, 0x00
        je .read
        cmp ah, 0x10
        je .read
        cmp ah, 0x01
        je .peek
        cmp ah, 0x11
        je .peek
        cmp ah, 0x02
        je .shift
        cmp ah, 0x12
        je .shift12
        cmp ah, 0x05
        je .push
.ret:   pop si
        pop ds
        iret

; AH=00h/10h: wait for a key, return AH = scancode, AL = ASCII.
.read:  push bx
        mov bl, ah
.wait:  cli
        mov si, [BDA_KBHEAD]
        cmp si, [BDA_KBTAIL]
        jne .got
        sti
        hlt
        jmp .wait
.got:   mov ax, [si]
        add si, 2
        cmp si, [BDA_KBEND]
        jb .nowrap
        mov si, [BDA_KBSTART]
.nowrap:
        mov [BDA_KBHEAD], si
        sti
        cmp bl, 0x10                    ; the old call hides E0 extended codes
        je .rdone
        cmp al, 0xE0
        jne .rdone
        xor al, al
.rdone: pop bx
        jmp .ret

; AH=01h/11h: check for a key: ZF=1 if none, else AX = key (not removed).
.peek:  cli
        mov si, [BDA_KBHEAD]
        cmp si, [BDA_KBTAIL]
        sti
        je .empty
        push bx
        mov bl, ah
        mov ax, [si]
        cmp bl, 0x11
        je .pok
        cmp al, 0xE0
        jne .pok
        xor al, al
.pok:   pop bx
        push bp
        mov bp, sp
        and word [bp+10], ~0x0040       ; caller's ZF = 0
        pop bp
        jmp .ret
.empty: push bp
        mov bp, sp
        or word [bp+10], 0x0040         ; caller's ZF = 1
        pop bp
        jmp .ret

; AH=02h/12h: shift flags.
.shift: mov al, [BDA_SHIFT]
        jmp .ret
.shift12:
        mov al, [BDA_SHIFT]
        mov ah, [BDA_SHIFT+1]
        jmp .ret

; AH=05h: push key CX into the buffer. AL = 0 ok, 1 full.
.push:  push bx
        mov ax, cx
        cli
        mov bx, [BDA_KBTAIL]
        call kb_put
        cmp bx, [BDA_KBTAIL]
        sti
        mov al, 0
        jne .pushed
        mov al, 1
.pushed:
        pop bx
        jmp .ret

; =============================================================================
; INT 18h / 19h - boot
; =============================================================================
int18:
        mov si, msg_noboot
        call print
.halt:  cli
        hlt
        jmp .halt

; Try A: then C:. Load the boot sector to 0000:7C00 and jump to it.
int19:
        sti
        xor ax, ax
        mov ds, ax
        mov es, ax
        mov dl, 0x00
        call try_boot
        mov dl, 0x80
        call try_boot
        int 0x18

; Attempt to boot from drive DL. Returns only on failure.
try_boot:
        mov ax, 0x0201
        mov bx, 0x7C00
        mov cx, 0x0001
        xor dh, dh
        int 0x13
        jc .fail
        cmp word [es:0x7DFE], 0xAA55
        jne .fail
        mov si, msg_boot_a
        test dl, 0x80
        jz .msg
        mov si, msg_boot_c
.msg:   call print
        xor ax, ax
        mov ss, ax
        mov sp, 0x7C00
        jmp 0x0000:0x7C00
.fail:  ret

msg_boot_a  db 'Booting from floppy A:...', 13, 10, 0
msg_boot_c  db 'Booting from hard disk C:...', 13, 10, 0
msg_noboot  db 'No bootable disk found. System halted.', 13, 10, 0

; =============================================================================
; INT 1Ah - time of day and real-time clock
; =============================================================================
int1a:
        sti
        push ds
        push si
        mov si, BDA
        mov ds, si
        cmp ah, 0x00
        je .get
        cmp ah, 0x01
        je .set
        cmp ah, 0x02
        je .time
        cmp ah, 0x04
        je .date
        pop si
        pop ds
        stc
        retf 2
; AH=00h: CX:DX = ticks since midnight, AL = midnight flag (then cleared).
.get:   cli
        mov dx, [BDA_TICKS]
        mov cx, [BDA_TICKS+2]
        mov al, [BDA_MIDNIGHT]
        mov byte [BDA_MIDNIGHT], 0
        sti
        jmp .ok
; AH=01h: set ticks from CX:DX.
.set:   cli
        mov [BDA_TICKS], dx
        mov [BDA_TICKS+2], cx
        mov byte [BDA_MIDNIGHT], 0
        sti
        jmp .ok
; AH=02h: CH:CL:DH = hours:minutes:seconds (BCD), DL = 0 (no DST).
.time:  mov al, 4
        call cmos_read
        mov ch, al
        mov al, 2
        call cmos_read
        mov cl, al
        mov al, 0
        call cmos_read
        mov dh, al
        xor dl, dl
        jmp .ok
; AH=04h: CH = century, CL = year, DH = month, DL = day (BCD).
.date:  mov al, 0x32
        call cmos_read
        mov ch, al
        mov al, 9
        call cmos_read
        mov cl, al
        mov al, 8
        call cmos_read
        mov dh, al
        mov al, 7
        call cmos_read
        mov dl, al
.ok:    pop si
        pop ds
        clc
        retf 2

; =============================================================================
; Data
; =============================================================================

; 1.44 MB floppy disk parameter table (pointed to by INT 1Eh).
floppy_dpt:
        db 0xAF, 0x02, 0x25, 0x02, 18, 0x1B, 0xFF, 0x6C, 0xF6, 0x0F, 0x08

; =============================================================================
; Reset vector and ROM identification
; =============================================================================
        times 0xFFF0 - ($ - $$) db 0xFF
reset_vector:
        jmp 0xF000:start
        db '11/07/26'                   ; BIOS date
        db 0
        db 0xFC                         ; model byte: AT-class
        db 0                            ; checksum (unused)
