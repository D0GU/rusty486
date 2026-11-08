; =============================================================================
; rusty486 DOS extender stub ("PM stub")
;
; The real-mode part of a 32-bit TinyDOS program. The .EXE file is:
;
;   MZ header | this stub | payload header (32 bytes) | flat 32-bit image
;
; The stub loads the image to its link address above 1 MB (using unreal
; mode), switches to 32-bit flat protected mode (ring 0, interrupts off) and
; calls the entry point with a pointer to the system table:
;
;   +0  rm_int(intno, regs)   call DOS/BIOS: switches to real mode and back
;   +4  PSP segment           +8  transfer buffer (linear)
;   +12 transfer buffer seg   +16 transfer buffer size
;   +20 command tail (linear) +24 heap start (after BSS)  +28 heap end
;
; regs (36 bytes): eax ebx ecx edx esi edi ebp (dword each), ds es flags pad (word).
; Returning from the entry point exits to DOS with AL = return value.
; CPU exceptions print a message and exit with code 255.
;
; Payload header: 'R486', load address, image size, BSS size, entry, stack top,
; heap end (0 = the top of RAM), reserved.
; =============================================================================

        bits 16
        cpu 486
        org 0

%define SEL_CODE32 0x08
%define SEL_DATA32 0x10
%define SEL_CODE16 0x18
%define SEL_DATA16 0x20
%define XFER_SIZE  32768
%define HEAP_END   0x00F00000

start:
        mov ax, cs
        mov ds, ax
        mov [psp], es
        cli
        mov ss, ax
        mov sp, rm_stack_top
        sti
        cld
        ; Keep only the stub (code, data and buffers) in conventional memory.
        mov bx, cs
        sub bx, [psp]
        add bx, (stub_end + 15) / 16
        mov ah, 0x4A
        int 0x21

        ; Our own path: after the environment strings and a word count.
        mov es, [psp]
        mov es, [es:0x2C]
        xor di, di
.env:   cmp word [es:di], 0
        je .envend
        inc di
        jmp .env
.envend:
        add di, 4                       ; "\0\0" + count word
        mov si, path
.cp:    mov al, [es:di]
        mov [si], al
        inc di
        inc si
        test al, al
        jnz .cp

        mov dx, path
        mov ax, 0x3D00
        int 0x21
        jc fail_open
        mov [handle], ax

        ; MZ header -> payload offset (end of the MZ load image).
        mov dx, hdrbuf
        mov cx, 28
        call read_file
        movzx eax, word [hdrbuf+4]      ; pages
        shl eax, 9
        movzx ecx, word [hdrbuf+2]      ; bytes in the last page
        jecxz .full
        sub eax, 512
        add eax, ecx
.full:  call seek
        mov dx, payload_hdr
        mov cx, 32
        call read_file
        cmp dword [payload_hdr], 'R486'
        jne fail_format

        ; Linear base of this segment.
        xor eax, eax
        mov ax, cs
        shl eax, 4
        mov [base], eax
        call setup_tables

        ; A20 on, then unreal mode so ES can reach all memory.
        in al, 0x92
        or al, 2
        out 0x92, al
        call enter_unreal

        ; Copy the image above 1 MB in transfer-buffer sized chunks.
        mov edi, [payload_hdr+4]        ; load address
        mov eax, [payload_hdr+8]
        mov [left], eax                 ; bytes left (read_file clobbers BX)
.chunk: mov ecx, [left]
        test ecx, ecx
        jz .loaded
        cmp ecx, XFER_SIZE
        jbe .rd
        mov ecx, XFER_SIZE
.rd:    sub [left], ecx
        push ecx
        push edi
        mov dx, xfer
        call read_file
        pop edi
        pop ecx
        xor ax, ax
        mov es, ax
        mov esi, xfer
        a32 rep movsb
        jmp .chunk
.loaded:
        ; Clear the BSS.
        mov ecx, [payload_hdr+12]
        xor ax, ax
        mov es, ax
        a32 rep stosb
        mov bx, [handle]
        mov ah, 0x3E
        int 0x21

        ; System table.
        mov eax, [base]
        add eax, rm_int32
        mov [systab+0], eax
        movzx eax, word [psp]
        mov [systab+4], eax
        mov eax, [base]
        add eax, xfer
        mov [systab+8], eax
        shr eax, 4
        mov [systab+12], eax
        mov dword [systab+16], XFER_SIZE
        movzx eax, word [psp]
        shl eax, 4
        add eax, 0x80
        mov [systab+20], eax
        mov eax, [payload_hdr+4]
        add eax, [payload_hdr+8]
        add eax, [payload_hdr+12]
        add eax, 15
        and eax, ~15
        mov [systab+24], eax
        mov eax, [payload_hdr+24]
        test eax, eax
        jnz .heap
        call memory_top
.heap:  mov [systab+28], eax

        ; Into 32-bit protected mode.
        cli
        lgdt [gdtr]
        lidt [pm_idtr]
        mov eax, cr0
        or al, 1
        mov cr0, eax
        db 0x66, 0xEA                   ; jmp dword SEL_CODE32:pm_start (linear)
pm_start_ptr:
        dd 0
        dw SEL_CODE32

; EAX = the end of RAM: INT 15h E801h (or 88h), else HEAP_END.
memory_top:
        mov ax, 0xE801
        int 0x15
        jc .old
        movzx eax, bx
        test eax, eax
        jz .low
        shl eax, 16
        add eax, 0x1000000
        ret
.low:   movzx eax, ax
        jmp .kb
.old:   mov ah, 0x88
        int 0x15
        jc .default
        movzx eax, ax
.kb:    shl eax, 10
        add eax, 0x100000
        ret
.default:
        mov eax, HEAP_END
        ret

; Read CX bytes into DS:DX from the program file. CF on error.
read_file:
        mov bx, [handle]
        mov ah, 0x3F
        int 0x21
        ret

; Seek the program file to EAX.
seek:   mov dx, ax
        shr eax, 16
        mov cx, ax
        mov bx, [handle]
        mov ax, 0x4200
        int 0x21
        ret

; Fill in the GDT bases, IDT entries and far-jump targets for this segment.
setup_tables:
        mov eax, [base]
        ; 16-bit code and data descriptors based at this segment.
        mov [gdt_code16+2], ax
        mov [gdt_data16+2], ax
        shr eax, 16
        mov [gdt_code16+4], al
        mov [gdt_data16+4], al
        mov [gdt_code16+7], ah
        mov [gdt_data16+7], ah
        mov eax, [base]
        add eax, gdt
        mov [gdtr+2], eax
        mov eax, [base]
        add eax, idt
        mov [pm_idtr+2], eax
        mov eax, [base]
        add eax, pm_start
        mov [pm_start_ptr], eax
        mov eax, [base]
        add eax, back32
        mov [back32_ptr], eax
        mov [real_cs], cs
        ; 32 exception gates -> exc_N stubs (each 8 bytes apart).
        mov cx, 32
        xor si, si
        mov di, idt
.g:     mov eax, [base]
        add eax, exc_stubs
        movzx edx, si
        add eax, edx
        mov [di], ax
        mov word [di+2], SEL_CODE32
        mov word [di+4], 0x8E00         ; present, DPL 0, 32-bit interrupt gate
        shr eax, 16
        mov [di+6], ax
        add di, 8
        add si, 8
        loop .g
        ret

enter_unreal:
        cli
        lgdt [gdtr]
        mov eax, cr0
        or al, 1
        mov cr0, eax
        mov ax, SEL_DATA32
        mov es, ax
        mov fs, ax
        mov eax, cr0
        and al, 0xFE
        mov cr0, eax
        xor ax, ax
        mov es, ax
        mov fs, ax
        sti
        ret

fail_open:
        mov dx, msg_open
        jmp fail
fail_format:
        mov dx, msg_format
fail:   mov ah, 0x09
        int 0x21
        mov ax, 0x4CFF
        int 0x21

; ---------------------------------------------------------------- 32-bit code

        bits 32
pm_start:
        mov ax, SEL_DATA32
        mov ds, ax
        mov es, ax
        mov fs, ax
        mov gs, ax
        mov ss, ax
        call .b
.b:     pop ebp
        sub ebp, .b                     ; EBP = linear base of the stub
        mov esp, [ebp+payload_hdr+20]
        lea eax, [ebp+systab]
        push eax
        call [ebp+payload_hdr+16]
        ; Returned: exit to DOS with AL.
        movzx eax, al
        or eax, 0x4C00
        sub esp, 36
        mov [esp], eax
        push esp
        push dword 0x21
        call rm_int32_local
.hang:  jmp .hang

rm_int32_local:
        call .b
.b:     pop eax
        sub eax, .b
        add eax, rm_int32
        jmp eax

; rm_int(intno, regs): run software interrupt `intno` in real mode with the
; register image at `regs`, then copy the results back. cdecl.
rm_int32:
        pushad
        call .b
.b:     pop ebp
        sub ebp, .b
        mov [ebp+saved_esp], esp
        mov eax, [esp+36]
        mov [ebp+int_no], al
        mov esi, [esp+40]
        mov [ebp+regs_ptr], esi
        lea edi, [ebp+rr]
        mov ecx, 9
        rep movsd
        jmp SEL_CODE16:to16

        bits 16
to16:   mov ax, SEL_DATA16
        mov ds, ax
        mov es, ax
        mov ss, ax
        mov esp, rm_stack_top           ; clear ESP's upper half for real mode
        mov eax, cr0
        and al, 0xFE
        mov cr0, eax
        db 0xEA                         ; jmp real_cs:in_real
        dw in_real
real_cs dw 0
in_real:
        mov ax, cs
        mov ds, ax
        mov ss, ax
        mov esp, rm_stack_top
        lidt [rm_idtr]
        mov es, [rr_es]
        mov eax, [rr_eax]
        mov ebx, [rr_ebx]
        mov ecx, [rr_ecx]
        mov edx, [rr_edx]
        mov esi, [rr_esi]
        mov edi, [rr_edi]
        mov ebp, [rr_ebp]
        push word [rr_ds]
        pop ds
        sti
        db 0xCD                         ; int n
int_no  db 0x21
        cli
        mov [cs:rr_eax], eax
        mov [cs:rr_ebx], ebx
        mov [cs:rr_ecx], ecx
        mov [cs:rr_edx], edx
        mov [cs:rr_esi], esi
        mov [cs:rr_edi], edi
        mov [cs:rr_ebp], ebp
        mov [cs:rr_ds], ds
        mov [cs:rr_es], es
        pushf
        pop word [cs:rr_flags]
        mov ax, cs
        mov ds, ax
        lgdt [gdtr]
        lidt [pm_idtr]
        mov eax, cr0
        or al, 1
        mov cr0, eax
        db 0x66, 0xEA                   ; jmp dword SEL_CODE32:back32 (linear)
back32_ptr:
        dd 0
        dw SEL_CODE32

        bits 32
back32: mov ax, SEL_DATA32
        mov ds, ax
        mov es, ax
        mov fs, ax
        mov gs, ax
        mov ss, ax
        call .b
.b:     pop ebp
        sub ebp, .b
        mov esp, [ebp+saved_esp]
        lea esi, [ebp+rr]
        mov edi, [ebp+regs_ptr]
        mov ecx, 9
        rep movsd
        popad
        ret

; Exceptions: print "CPU exception NNh at EIP xxxxxxxx" through DOS and
; exit with code 255.
exc_stubs:
%assign n 0
%rep 32
        push byte n                     ; 2 bytes
        jmp strict near exc_common      ; 5 bytes
        nop                             ; pad to 8
%assign n n+1
%endrep

exc_common:
        call .b
.b:     pop ebp
        sub ebp, .b
        pop eax                         ; vector
        ; Error-code exceptions push one more dword before EIP.
        mov ebx, [esp]
        cmp al, 8
        je .err
        cmp al, 10
        jb .noerr
        cmp al, 14
        ja .noerr
.err:   mov ebx, [esp+4]
.noerr: lea esp, [ebp+exc_stack_top]    ; a known-good stack
        lea edi, [ebp+xfer]
        lea esi, [ebp+msg_exc]
        call .str
        mov edx, eax
        shl edx, 24
        mov ecx, 2
        call .hex
        lea esi, [ebp+msg_at]
        call .str
        mov edx, ebx
        mov ecx, 8
        call .hex
        lea esi, [ebp+msg_crlf]
        call .str
        ; DOS AH=09h with DS = transfer buffer segment, DX = 0.
        lea edi, [ebp+rr]
        xor eax, eax
        mov ecx, 9
        rep stosd
        mov byte [ebp+rr_eax+1], 0x09
        mov eax, [ebp+systab+12]
        mov [ebp+rr_ds], ax
        lea eax, [ebp+rr]
        push eax
        push dword 0x21
        call rm_int32_local
        add esp, 8
        mov dword [ebp+rr_eax], 0x4CFF
        lea eax, [ebp+rr]
        push eax
        push dword 0x21
        call rm_int32_local
.h:     jmp .h
.str:   lodsb
        test al, al
        jz .sd
        stosb
        jmp .str
.sd:    ret
.hex:   rol edx, 4
        mov al, dl
        and al, 15
        add al, '0'
        cmp al, '9'
        jbe .d
        add al, 7
.d:     stosb
        loop .hex
        ret

        bits 16
; ---------------------------------------------------------------- data

msg_open    db 'Cannot open program file$'
msg_format  db 'Not a rusty486 32-bit program$'
msg_exc     db 13, 10, 'CPU exception ', 0
msg_at      db 'h at EIP ', 0
msg_crlf    db 13, 10, '$', 0

        align 8
gdt:        dq 0
            dw 0xFFFF, 0x0000           ; 08: code32 flat
            db 0x00, 0x9A, 0xCF, 0x00
            dw 0xFFFF, 0x0000           ; 10: data32 flat
            db 0x00, 0x92, 0xCF, 0x00
gdt_code16: dw 0xFFFF, 0x0000           ; 18: code16, base = this segment
            db 0x00, 0x9A, 0x00, 0x00
gdt_data16: dw 0xFFFF, 0x0000           ; 20: data16, base = this segment
            db 0x00, 0x92, 0x00, 0x00
gdt_end:
gdtr:       dw gdt_end - gdt - 1
            dd 0
pm_idtr:    dw 32 * 8 - 1
            dd 0
rm_idtr:    dw 0x3FF
            dd 0

psp         dw 0
handle      dw 0
base        dd 0
left        dd 0
saved_esp   dd 0
regs_ptr    dd 0
systab      times 8 dd 0
; Register image for rm_int.
rr:
rr_eax      dd 0
rr_ebx      dd 0
rr_ecx      dd 0
rr_edx      dd 0
rr_esi      dd 0
rr_edi      dd 0
rr_ebp      dd 0
rr_ds       dw 0
rr_es       dw 0
rr_flags    dw 0
            dw 0
hdrbuf      times 28 db 0
payload_hdr times 32 db 0
path        times 128 db 0

        align 8
idt         times 32 * 8 db 0
            times 512 db 0
exc_stack_top:
            times 1024 db 0
rm_stack_top:
        align 16
xfer:
stub_end    equ xfer + XFER_SIZE
