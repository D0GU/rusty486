; Instruction coverage for rusty486-asm. Assembled by both NASM and
; rusty486-asm; the outputs must match byte for byte.

%macro FORMS 0
        ; ALU in all forms
        add al, bl
        add ax, bx
        add eax, ebx
        add [bx], al
        add [bx+si+4], ax
        add [ebx+ecx*4+100h], eax
        add cl, [di]
        add dx, [bp-2]
        add esi, [esp+8]
        add al, 5
        add ax, 5
        add ax, 1234h
        add ax, 0FFFFh
        add bx, 5
        add bx, 1234h
        add ebx, 12345678h
        add eax, -1
        add byte [bx], 7
        add word [bx], 7
        add word [bx], 300
        add dword [bx], 300
        or cx, dx
        adc si, 3
        sbb di, [si]
        and al, 0Fh
        sub sp, 8
        xor ax, ax
        cmp byte [es:di], 0
        cmp word [fwd], 1
        cmp ax, fwd

        ; mov
        mov al, bh
        mov ax, bx
        mov eax, ecx
        mov al, [1234h]
        mov ax, [fwd]
        mov [1234h], al
        mov [fwd], ax
        mov bl, [bx]
        mov cx, [bp]
        mov dx, [bp+di+12h]
        mov [si-1], dx
        mov ebx, [ebp]
        mov ecx, [esp]
        mov edx, [eax*2]
        mov edx, [eax*4]
        mov edx, [eax*4+10h]
        mov edx, [ebx+eax*8-4]
        mov edx, [ecx*3]
        mov al, 12h
        mov ax, 1234h
        mov eax, 12345678h
        mov si, fwd
        mov byte [bx], 1
        mov word [bx+2], 1234h
        mov dword [bx], 1
        mov ax, ds
        mov eax, ds
        mov [bx], es
        mov ds, ax
        mov es, [bx]
        mov fs, ax
        mov gs, ax
        mov ss, ax
        mov eax, cr0
        mov cr0, eax
        mov cr3, ebx
        mov ebx, dr7
        mov dr0, eax
        mov ax, [cs:bx]
        mov ax, [es:1234h]
        mov ax, es:[di]

        ; test / xchg
        test al, bl
        test ax, bx
        test [bx], cx
        test cx, [bx]
        test al, 1
        test ax, 8000h
        test bl, 80h
        test word [bx], 1
        xchg ax, bx
        xchg bx, ax
        xchg bx, cx
        xchg al, bl
        xchg [bx], ax
        xchg ax, [bx]
        xchg eax, edx

        ; inc / dec / unary
        inc ax
        inc eax
        inc al
        inc byte [bx]
        inc word [bx]
        dec di
        dec dword [bx]
        not ax
        neg byte [si]
        mul bl
        mul cx
        div word [bx]
        idiv ecx
        imul bx
        imul ax, bx
        imul ax, [bx]
        imul ax, 10
        imul ax, 1000
        imul ax, bx, 10
        imul eax, [ebx], 100000

        ; shifts
        shl ax, 1
        shl ax, 4
        shl ax, cl
        shr byte [bx], 1
        sar dx, 3
        sal bx, 1
        rol al, cl
        ror eax, 8
        rcl word [bx], 1
        rcr si, 2

        ; stack
        push ax
        push eax
        push es
        push cs
        push ss
        push ds
        push fs
        push gs
        push 5
        push 1234h
        push -1
        push word [bx]
        push dword [bx]
        push fwd
        pop bx
        pop ebx
        pop es
        pop ss
        pop ds
        pop fs
        pop gs
        pop word [bx]
        pusha
        popa
        pushad
        popad
        pushf
        popf
        pushfd
        popfd
        enter 16, 0
        enter 100h, 2
        leave

        ; jumps and calls
.top:   jmp .top
        jmp fwd
        jmp near .top
        jmp short .top
        jmp bx
        jmp [bx]
        jmp word [bx]
        jmp far [bx]
        jmp 1234h:5678h
        call .top
        call fwd
        call bx
        call [bx+2]
        call far [bx]
        call 1234h:5678h
        je .top
        jne fwd
        jc .top
        jnc .top
        jz .top
        jnz .top
        ja .top
        jbe .top
        jg .top
        jge .top
        jl .top
        jle .top
        js .top
        jns .top
        jo .top
        jno .top
        jp .top
        jnp .top
        loop .top
        loope .top
        loopne .top
        jcxz .top
        jecxz .top
        ret
        ret 4
        retf
        retf 6
        int 10h
        int 3
        int3
        into
        iret
        iretd

        ; I/O
        in al, 60h
        in ax, 40h
        in eax, 40h
        in al, dx
        in ax, dx
        in eax, dx
        out 20h, al
        out 40h, ax
        out dx, al
        out dx, ax
        out dx, eax

        ; strings and prefixes
        movsb
        movsw
        movsd
        rep movsb
        rep stosw
        repe cmpsb
        repne scasb
        lodsb
        cs lodsb
        es movsb
        lock add [bx], ax
        lock inc word [bx]
        insb
        outsw
        xlatb

        ; misc
        lea ax, [bx+si+4]
        lea eax, [ebx+ecx*2+8]
        lea si, [fwd]
        lds si, [bx]
        les di, [bp+4]
        lss sp, [bx]
        lfs ax, [bx]
        lgs eax, [bx]
        bound ax, [bx]
        cbw
        cwde
        cwd
        cdq
        sahf
        lahf
        clc
        stc
        cmc
        cli
        sti
        cld
        std
        hlt
        nop
        aaa
        aas
        daa
        das
        aam
        aad
        aam 16
        wait

        ; 386 / 486
        movzx ax, bl
        movzx eax, bl
        movzx eax, word [bx]
        movzx ecx, byte [bx]
        movsx dx, al
        movsx edx, cx
        sete al
        setne byte [bx]
        setg cl
        bt ax, bx
        bt ax, 3
        bts word [bx], 5
        btr eax, ecx
        btc dword [bx], 31
        bsf ax, bx
        bsr ecx, [bx]
        shld ax, bx, 4
        shld ax, bx, cl
        shrd [bx], dx, 2
        shrd eax, edx, cl
        bswap eax
        bswap edi
        xadd [bx], ax
        xadd eax, ebx
        cmpxchg [bx], cl
        cmpxchg ecx, edx
        cpuid
        invd
        wbinvd
        invlpg [bx]
        lgdt [bx]
        lidt [bx]
        sgdt [bx]
        sidt [bx]
        lmsw ax
        smsw ax
        clts
%endmacro

        org 100h
        bits 16
start:
        FORMS
        bits 32
s32:
        FORMS
fwd:
        db 'data', 0, 13, 10
        dw 1234h, 'ab', 'abc'
        dd 12345678h, fwd, $
        times 4 db 0AAh
        align 4
        resb 3
        db $ - start, ($ - $$) & 0FFh
value   equ 1 << 4 | 3
        dw value, value * 2 + (3 - 1) / 2, ~value & 0FFh
        db -1, 255, 'x' + 1
