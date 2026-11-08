; =============================================================================
; TinyDOS kernel
;
; A small MS-DOS 5 compatible kernel for rusty486. Loaded by boot.asm at
; 0800:0000 (linear 8000h). Provides INT 20h/21h/27h/29h/33h, FAT12/16 file
; systems on the BIOS disks, MCB memory management and .COM/.EXE loading,
; then runs \COMMAND.COM.
;
; INT 21h handlers run with DS = kernel segment and BP pointing at the
; caller's saved registers (see F_* in dos.inc); results are written back
; into that frame.
; =============================================================================

        bits 16
        cpu 486
        org 0

%include "dos.inc"

entry:
        cli
        mov ax, cs
        mov ds, ax
        mov es, ax
        mov ss, ax
        mov sp, kstack_top
        sti
        cld
        mov [boot_drive], dl

        ; Interrupt vectors.
        xor ax, ax
        mov es, ax
%macro SETVEC 2
        mov word [es:%1*4], %2
        mov word [es:%1*4+2], KSEG
%endmacro
        SETVEC 0x20, int20
        SETVEC 0x21, int21
        SETVEC 0x22, int22
        SETVEC 0x23, int_iret
        SETVEC 0x24, int24
        SETVEC 0x25, int_retf
        SETVEC 0x26, int_retf
        SETVEC 0x27, int27
        SETVEC 0x28, int_iret
        SETVEC 0x29, int29
        SETVEC 0x2A, int_iret
        SETVEC 0x2F, int_iret
        SETVEC 0x33, int33
        mov eax, [es:0x1C*4]            ; chain the mouse cursor onto the timer hook
        mov [old_1c], eax
        SETVEC 0x1C, mouse_tick
        push ds
        pop es

        call mem_init
        call sft_init

        ; Mount drives: A:, B: (floppies), C:, D: (hard disks).
        mov al, 0
        mov dl, 0x00
        call mount
        mov al, 1
        mov dl, 0x01
        call mount
        mov al, 2
        mov dl, 0x80
        call mount
        mov al, 3
        mov dl, 0x81
        call mount

        ; Boot drive becomes the current drive.
        mov al, [boot_drive]
        test al, 0x80
        jz .floppy
        and al, 0x7F
        add al, 2
.floppy:
        mov [cur_drive], al

        mov si, msg_banner
        call puts

        ; Run the command interpreter forever.
.shell: push cs
        pop ds
        push cs
        pop es
        mov dx, comspec
        mov bx, exec_block
        mov word [exec_block], 0         ; environment: kernel's
        mov word [exec_block+2], cmd_tail_p
        mov word [exec_block+4], KSEG
        mov ax, 0x4B00
        int 0x21
        jnc .shell
        mov si, msg_nocommand
        call puts
.halt:  sti
        hlt
        jmp .halt

int_iret:
        iret
int_retf:
        retf
int24:  mov al, 3                       ; critical error: fail the call
        iret
int22:  ; terminate address: never reached directly (exec handles it)
        iret

; INT 29h: fast console output of AL.
int29:  push ax
        push bx
        mov ah, 0x0E
        mov bx, 0x0007
        int 0x10
        pop bx
        pop ax
        iret

; =============================================================================
; INT 21h dispatcher
; =============================================================================
int21:
        push ds
        push es
        push fs
        push gs
        pushad
        mov bp, sp
        cld
        push cs
        pop ds
        sti
        ; Did the host swap or edit a disk? Then drop cached sectors.
        mov dx, 0x318
        in al, dx
        cmp al, [media_gen]
        je .same_media
        mov [media_gen], al
        call media_changed
.same_media:
        movzx bx, byte [bp+F_AH]
        cmp bx, LAST_FN
        ja .bad
        shl bx, 1
        call [fn_table+bx]
        jmp int21_ret
.bad:   call fn_unsupported
int21_ret:
        popad
        pop gs
        pop fs
        pop es
        pop ds
        iret

; Disks changed under us: discard caches and re-read every drive's layout,
; keeping current directories where possible.
media_changed:
        pusha
        mov byte [sc_dirty], 0
        mov byte [fw_dirty], 0
        mov word [sc_drive], 0
        mov word [fw_drive], 0
        mov byte [remount], 1
        xor cx, cx
.d:     movzx bx, cl
        imul bx, bx, DRV_SIZE
        add bx, drives
        mov al, cl
        mov dl, [bx+D_BIOS]
        call mount
        inc cx
        cmp cx, MAX_DRIVES
        jb .d
        mov byte [remount], 0
        popa
        ret

; Return helpers for INT 21h functions.
ok:     and byte [bp+F_FLAGS], 0xFE
        ret

; Fail with error AX.
fail:   mov [bp+F_AX], ax
        mov [last_error], ax
        or byte [bp+F_FLAGS], 1
        ret

fn_unsupported:
        mov al, [bp+F_AH]
        cmp al, 0x2E                    ; old-style calls just return AL = 0
        jae .new
        mov byte [bp+F_AL], 0
        ret
.new:   mov ax, E_FUNC
        jmp fail

; Caller's DS:DX as FS:SI.
caller_dsdx:
        mov fs, [bp+F_DS]
        mov si, [bp+F_DX]
        ret

; =============================================================================
; Simple functions
; =============================================================================

; AH=00h / INT 20h: terminate.
fn_00:  mov byte [bp+F_AL], 0
        jmp fn_4c

fn_0e:  ; select default drive DL; returns number of drives in AL
        mov al, [bp+F_DL]
        cmp al, MAX_DRIVES
        jae .done
        mov ah, DRV_SIZE
        mul ah
        mov bx, ax
        cmp byte [drives+bx+D_PRESENT], 0
        je .done
        mov al, [bp+F_DL]
        mov [cur_drive], al
.done:  mov byte [bp+F_AL], 26
        ret

fn_19:  mov al, [cur_drive]
        mov [bp+F_AL], al
        ret

fn_1a:  mov ax, [bp+F_DX]               ; set DTA
        mov [dta], ax
        mov ax, [bp+F_DS]
        mov [dta+2], ax
        ret

fn_25:  ; set interrupt vector AL to DS:DX
        movzx bx, byte [bp+F_AL]
        shl bx, 2
        xor ax, ax
        mov es, ax
        cli
        mov ax, [bp+F_DX]
        mov [es:bx], ax
        mov ax, [bp+F_DS]
        mov [es:bx+2], ax
        sti
        ret

fn_2a:  ; get date: CX = year, DH = month, DL = day, AL = weekday
        call rtc_date                   ; CX year, DH month, DL day
        mov [bp+F_CX], cx
        mov [bp+F_DX], dx
        ; Day of week (Zeller-style, 0 = Sunday)
        call day_of_week
        mov [bp+F_AL], al
        ret

fn_2c:  ; get time: CH hour, CL minute, DH second, DL hundredths
        call rtc_time                   ; CH, CL, DH
        mov [bp+F_CX], cx
        mov [bp+F_DH], dh
        mov byte [bp+F_DL], 0
        ret

fn_set_ok:                              ; set date/time etc.: accept, AL = 0
        mov byte [bp+F_AL], 0
        ret

fn_2f:  mov ax, [dta]                   ; get DTA -> ES:BX
        mov [bp+F_BX], ax
        mov ax, [dta+2]
        mov [bp+F_ES], ax
        ret

fn_30:  mov word [bp+F_AX], TINYDOS_VER
        mov word [bp+F_BX], 0
        mov word [bp+F_CX], 0
        ret

fn_33:  ; ctrl-break check: get returns DL = 0
        cmp byte [bp+F_AL], 0
        jne .r
        mov byte [bp+F_DL], 0
.r:     ret

fn_35:  ; get vector AL -> ES:BX
        movzx bx, byte [bp+F_AL]
        shl bx, 2
        xor ax, ax
        mov es, ax
        mov ax, [es:bx]
        mov [bp+F_BX], ax
        mov ax, [es:bx+2]
        mov [bp+F_ES], ax
        ret

fn_37:  ; switch character: DL = '/'
        mov byte [bp+F_DL], '/'
        mov byte [bp+F_AL], 0
        ret

fn_38:  ; country info: fill 34 bytes of zeros at DS:DX, BX = 1 (USA)
        mov es, [bp+F_DS]
        mov di, [bp+F_DX]
        mov cx, 34
        xor al, al
        rep stosb
        mov word [bp+F_BX], 1
        jmp ok

fn_4d:  mov ax, [return_code]
        mov [bp+F_AX], ax
        jmp ok

fn_50:  mov ax, [bp+F_BX]
        mov [cur_psp], ax
        ret

fn_51:
fn_62:  mov ax, [cur_psp]
        mov [bp+F_BX], ax
        jmp ok

fn_54:  mov byte [bp+F_AL], 0           ; verify flag
        ret

fn_58:  ; allocation strategy: get -> 0, set -> ok
        mov word [bp+F_AX], 0
        jmp ok

fn_59:  ; extended error
        mov ax, [last_error]
        mov [bp+F_AX], ax
        mov word [bp+F_BX], 0x0101
        mov byte [bp+F_CH], 1
        ret

fn_ok:  jmp ok                          ; accepted no-ops (67h, 68h...)

; =============================================================================
; Time helpers (RTC via BIOS)
; =============================================================================

; AL (BCD) -> AL (binary)
bcd2bin:
        push cx
        mov cl, al
        shr al, 4
        mov ch, 10
        mul ch
        and cl, 0x0F
        add al, cl
        pop cx
        ret

; CX = year, DH = month, DL = day
rtc_date:
        mov ah, 0x04
        int 0x1A
        mov al, ch
        call bcd2bin
        mov ah, 100
        mul ah
        mov bx, ax
        mov al, cl
        call bcd2bin
        xor ah, ah
        add bx, ax
        mov al, dh
        call bcd2bin
        mov dh, al
        mov al, dl
        call bcd2bin
        mov dl, al
        mov cx, bx
        ret

; CH = hour, CL = minute, DH = second
rtc_time:
        mov ah, 0x02
        int 0x1A
        mov al, ch
        call bcd2bin
        mov ch, al
        mov al, cl
        call bcd2bin
        mov cl, al
        mov al, dh
        call bcd2bin
        mov dh, al
        ret

; Day of week for CX = year, DH = month, DL = day -> AL (0 = Sunday).
; Uses Sakamoto's method.
day_of_week:
        push bx
        push cx
        push dx
        movzx bx, dh
        cmp bx, 3
        jae .noadj
        dec cx
.noadj: movzx ax, byte [dow_table+bx-1]
        movzx dx, dl
        add ax, dx                      ; t[m-1] + d
        add ax, cx                      ; + y
        mov bx, cx
        shr bx, 2
        add ax, bx                      ; + y/4
        push ax
        mov ax, cx
        xor dx, dx
        mov bx, 100
        div bx
        mov bx, ax                      ; y/100
        pop ax
        sub ax, bx
        push ax
        mov ax, cx
        xor dx, dx
        mov bx, 400
        div bx
        mov bx, ax
        pop ax
        add ax, bx                      ; + y/400
        xor dx, dx
        mov bx, 7
        div bx
        mov al, dl
        pop dx
        pop cx
        pop bx
        ret
dow_table db 0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4

; DOS packed date in DX, time in AX (for directory entries).
dos_datetime:
        push cx
        push bx
        call rtc_date                   ; CX year, DH month, DL day
        sub cx, 1980
        shl cx, 9
        movzx ax, dh
        shl ax, 5
        or cx, ax
        movzx ax, dl
        or cx, ax
        push cx
        call rtc_time                   ; CH hour, CL min, DH sec
        movzx ax, ch
        shl ax, 11
        movzx bx, cl
        shl bx, 5
        or ax, bx
        movzx bx, dh
        shr bx, 1
        or ax, bx
        pop dx
        pop bx
        pop cx
        ret

%include "console.asm"
%include "disk.asm"
%include "file.asm"
%include "mem.asm"
%include "exec.asm"
%include "mouse.asm"

; =============================================================================
; Function table
; =============================================================================
fn_table:
        dw fn_00, fn_01, fn_02, fn_unsupported, fn_unsupported, fn_unsupported, fn_06, fn_07  ; 00-07
        dw fn_08, fn_09, fn_0a, fn_0b, fn_0c, fn_0d, fn_0e, fn_unsupported                    ; 08-0F
        dw fn_unsupported, fn_unsupported, fn_unsupported, fn_unsupported                     ; 10-13
        dw fn_unsupported, fn_unsupported, fn_unsupported, fn_unsupported                     ; 14-17
        dw fn_unsupported, fn_19, fn_1a, fn_unsupported, fn_unsupported, fn_unsupported, fn_unsupported, fn_unsupported ; 18-1F
        dw fn_unsupported, fn_unsupported, fn_unsupported, fn_unsupported                     ; 20-23
        dw fn_unsupported, fn_25, fn_unsupported, fn_unsupported                              ; 24-27
        dw fn_unsupported, fn_unsupported, fn_2a, fn_set_ok, fn_2c, fn_set_ok, fn_set_ok, fn_2f ; 28-2F
        dw fn_30, fn_31, fn_unsupported, fn_33, fn_unsupported, fn_35, fn_36, fn_37            ; 30-37
        dw fn_38, fn_39, fn_3a, fn_3b, fn_3c, fn_3d, fn_3e, fn_3f                             ; 38-3F
        dw fn_40, fn_41, fn_42, fn_43, fn_44, fn_45, fn_46, fn_47                             ; 40-47
        dw fn_48, fn_49, fn_4a, fn_4b, fn_4c, fn_4d, fn_4e, fn_4f                             ; 48-4F
        dw fn_50, fn_51, fn_unsupported, fn_unsupported, fn_54, fn_unsupported, fn_56, fn_57  ; 50-57
        dw fn_58, fn_59, fn_unsupported, fn_5b, fn_unsupported, fn_unsupported, fn_unsupported, fn_unsupported ; 58-5F
        dw fn_unsupported, fn_unsupported, fn_62, fn_unsupported, fn_unsupported, fn_unsupported, fn_unsupported, fn_ok ; 60-67
        dw fn_68, fn_unsupported, fn_unsupported, fn_unsupported, fn_6c                       ; 68-6C

; =============================================================================
; Data
; =============================================================================
msg_banner    db 13, 10, 'TinyDOS 1.0 for rusty486', 13, 10, 13, 10, 0
msg_nocommand db 'Bad or missing command interpreter (\COMMAND.COM)', 13, 10, 0
comspec       db '\COMMAND.COM', 0
cmd_tail_p    db 3, ' /P', 13
exec_block    times 14 db 0

boot_drive    db 0
media_gen     db 0
cur_drive     db 0
cur_psp       dw 0
dta           dw 0x80, 0
last_error    dw 0
return_code   dw 0

        align 16
kstack        times 1024 db 0
kstack_top:
