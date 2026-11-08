; =============================================================================
; COMMAND.COM line editor: command history and Tab completion (DOSKEY style)
;
;   Left/Right Home/End      move            Ctrl+Left/Right   move by word
;   Backspace Del            delete          Ins               insert/overwrite
;   Up/Down                  history         F3                last command
;   Tab                      complete a file, directory or command name;
;                            a second Tab lists every match
;   Esc                      clear the line  Ctrl+C            cancel
; =============================================================================

%define HIST_N   32                     ; commands remembered
%define HIST_SZ  128                    ; bytes per command (with the NUL)
%define ED_MAX   120                    ; longest line
%define CP_MAX   80                     ; completion matches kept

; Read a line from the keyboard into linebuf+2 (NUL-terminated).
read_line:
        push es
        push ds
        pop es
        cld
        call ed_home_here
        xor ax, ax
        mov [ed_len], ax
        mov [ed_pos], ax
        mov [ed_old], ax
        mov [ed_back], ax
        mov byte [ed_tabs], 0
        mov byte [linebuf+2], 0
.key:   xor ah, ah
        int 0x16
        cmp ah, 0x0F                    ; Tab (also Shift+Tab)
        je .tab
        mov byte [ed_tabs], 0
        cmp ah, 0x1C
        je .enter
        cmp ah, 0x0E
        je .bksp
        cmp ah, 0x01
        je .esc
        cmp al, 3
        je .ctrlc
        test al, al
        jz .ext
        cmp al, 0xE0
        je .ext
        cmp al, 32
        jb .key
        call ed_type
        jmp .draw
.tab:   call complete
        mov byte [ed_tabs], 1
        jmp .draw
.bksp:  mov bx, [ed_pos]
        test bx, bx
        jz .key
        dec bx
        mov cx, 1
        call ed_delete
        jmp .draw
.esc:   xor ax, ax
        mov [ed_len], ax
        mov [ed_pos], ax
        mov [ed_back], ax
        mov byte [linebuf+2], 0
        jmp .draw
.ctrlc: mov ax, [ed_len]
        mov [ed_pos], ax
        call ed_place
        mov al, '^'
        call tty
        mov al, 'C'
        call tty
        mov byte [linebuf+2], 0
        jmp .done
.ext:   call ctrl_held
        mov dl, al                      ; DL = Ctrl
        cmp ah, 0x4B
        je .left
        cmp ah, 0x4D
        je .right
        cmp ah, 0x47
        je .home
        cmp ah, 0x4F
        je .end
        cmp ah, 0x53
        je .del
        cmp ah, 0x52
        je .ins
        cmp ah, 0x48
        je .up
        cmp ah, 0x50
        je .down
        cmp ah, 0x3D
        je .f3
        jmp .key
.left:  test dl, dl
        jnz .wleft
        cmp word [ed_pos], 0
        je .key
        dec word [ed_pos]
        jmp .draw
.wleft: mov bx, [ed_pos]
.wl1:   test bx, bx
        jz .wl3
        cmp byte [linebuf+2+bx-1], ' '
        jne .wl2
        dec bx
        jmp .wl1
.wl2:   test bx, bx
        jz .wl3
        cmp byte [linebuf+2+bx-1], ' '
        je .wl3
        dec bx
        jmp .wl2
.wl3:   mov [ed_pos], bx
        jmp .draw
.right: test dl, dl
        jnz .wright
        mov ax, [ed_pos]
        cmp ax, [ed_len]
        jae .key
        inc word [ed_pos]
        jmp .draw
.wright:
        mov bx, [ed_pos]
.wr1:   cmp bx, [ed_len]
        jae .wr3
        cmp byte [linebuf+2+bx], ' '
        je .wr2
        inc bx
        jmp .wr1
.wr2:   cmp bx, [ed_len]
        jae .wr3
        cmp byte [linebuf+2+bx], ' '
        jne .wr3
        inc bx
        jmp .wr2
.wr3:   mov [ed_pos], bx
        jmp .draw
.home:  mov word [ed_pos], 0
        jmp .draw
.end:   mov ax, [ed_len]
        mov [ed_pos], ax
        jmp .draw
.del:   mov bx, [ed_pos]
        cmp bx, [ed_len]
        jae .key
        mov cx, 1
        call ed_delete
        jmp .draw
.ins:   xor byte [ed_over], 1
        call ed_cursor_shape
        jmp .key
.up:    mov ax, [ed_back]
        cmp ax, [hist_count]
        jae .key
        inc ax
        mov [ed_back], ax
        call hist_get
        jmp .draw
.down:  mov ax, [ed_back]
        cmp ax, 1
        jb .key
        je .esc                         ; past the newest: an empty line
        dec ax
        mov [ed_back], ax
        call hist_get
        jmp .draw
.f3:    cmp word [hist_count], 0
        je .key
        mov ax, 1
        mov [ed_back], ax
        call hist_get
        jmp .draw
.draw:  call ed_redraw
        jmp .key
.enter: mov ax, [ed_len]
        mov [ed_pos], ax
        call ed_place
        call hist_add
.done:  mov byte [ed_over], 0
        call ed_cursor_shape
        pop es
        ret

; Remember where the line starts on the screen (the cursor position).
ed_home_here:
        mov ah, 0x03
        xor bh, bh
        int 0x10
        mov [ed_row], dh
        mov [ed_col], dl
        mov word [ed_old], 0
        ret

; AL = 1 if Ctrl is held.
ctrl_held:
        push es
        push ax
        mov ax, 0x40
        mov es, ax
        mov al, [es:0x17]
        pop dx
        mov ah, dh
        and al, 4
        shr al, 2
        pop es
        ret

ed_cursor_shape:
        mov cx, 0x0607
        cmp byte [ed_over], 0
        je .set
        mov cx, 0x0007
.set:   mov ah, 0x01
        int 0x10
        ret

; Type AL at the cursor (insert or overwrite).
ed_type:
        mov bx, [ed_pos]
        cmp byte [ed_over], 0
        je ed_insert
        cmp bx, [ed_len]
        jae ed_insert
        mov [linebuf+2+bx], al
        inc word [ed_pos]
        ret

; Insert AL at the cursor.
ed_insert:
        mov cx, [ed_len]
        cmp cx, ED_MAX
        jae .r
        mov bx, [ed_pos]
        ; Shift linebuf[pos..len] right by one (from the end).
        mov si, cx
.sh:    cmp si, bx
        jbe .put
        mov ah, [linebuf+2+si-1]
        mov [linebuf+2+si], ah
        dec si
        jmp .sh
.put:   mov [linebuf+2+bx], al
        inc word [ed_len]
        inc word [ed_pos]
        mov si, [ed_len]
        mov byte [linebuf+2+si], 0
.r:     ret

; Delete CX characters at BX; the cursor goes to BX.
ed_delete:
        mov [ed_pos], bx
        mov si, bx
        add si, cx
        add si, linebuf+2
        lea di, [bx+linebuf+2]
        mov ax, [ed_len]
        sub ax, bx
        sub ax, cx
        inc ax                          ; and the NUL
        xchg ax, cx
        rep movsb
        sub [ed_len], ax
        ret

; Teletype one character (BIOS, so it scrolls at the bottom).
tty:    push ax
        push bx
        mov ah, 0x0E
        mov bx, 0x0007
        int 0x10
        pop bx
        pop ax
        ret

; Redraw the line from its start and put the cursor at ed_pos.
ed_redraw:
        mov dh, [ed_row]
        mov dl, [ed_col]
        call set_cursor
        mov si, linebuf+2
        mov cx, [ed_len]
        jcxz .clear
.p:     lodsb
        call tty
        loop .p
.clear: ; Blank what was left of a longer previous line.
        mov cx, [ed_old]
        sub cx, [ed_len]
        jbe .scroll
.s:     mov al, ' '
        call tty
        loop .s
.scroll:
        ; If drawing ran past the bottom, the screen scrolled up.
        mov ax, [ed_old]
        cmp ax, [ed_len]
        jae .big
        mov ax, [ed_len]
.big:   movzx bx, byte [ed_col]
        add ax, bx
        mov bl, 80
        div bl                          ; AL = rows below the start
        add al, [ed_row]
        cmp al, 24
        jbe .ok
        sub al, 24
        sub [ed_row], al
.ok:    mov ax, [ed_len]
        mov [ed_old], ax
ed_place:
        mov ax, [ed_pos]
        movzx bx, byte [ed_col]
        add ax, bx
        mov bl, 80
        div bl
        add al, [ed_row]
        mov dh, al
        mov dl, ah
set_cursor:
        mov ah, 0x02
        xor bh, bh
        int 0x10
        ret

; ------------------------------------------------------------------ history

; Add the line to the history (not empty lines or repeats of the last one).
hist_add:
        cmp word [ed_len], 0
        je .r
        cmp word [hist_count], 0
        je .add
        mov ax, 1
        call hist_slot
        mov di, si
        mov si, linebuf+2
.cmp:   mov al, [si]
        cmp al, [di]
        jne .add
        inc si
        inc di
        test al, al
        jnz .cmp
        ret                             ; same as the last command
.add:   mov ax, [hist_next]
        mov bx, HIST_SZ
        mul bx
        mov di, hist_buf
        add di, ax
        mov si, linebuf+2
        call strcpy
        mov ax, [hist_next]
        inc ax
        cmp ax, HIST_N
        jb .w
        xor ax, ax
.w:     mov [hist_next], ax
        cmp word [hist_count], HIST_N
        jae .r
        inc word [hist_count]
.r:     ret

; SI = the AX-th most recent history entry (1 = newest).
hist_slot:
        mov bx, [hist_next]
        add bx, HIST_N
        sub bx, ax
        cmp bx, HIST_N
        jb .m
        sub bx, HIST_N
.m:     mov ax, bx
        mov bx, HIST_SZ
        mul bx
        mov si, hist_buf
        add si, ax
        ret

; Load history entry AX into the line.
hist_get:
        call hist_slot
        mov di, linebuf+2
        call strcpy
        sub di, linebuf+3
        mov [ed_len], di
        mov [ed_pos], di
        ret

; HISTORY: list the remembered commands, oldest first.
cmd_history:
        mov cx, [hist_count]
.l:     jcxz .r
        push cx
        mov ax, [hist_count]
        sub ax, cx
        inc ax
        movzx eax, ax
        push cx
        mov cx, 4
        call print_num_w
        pop cx
        mov al, ' '
        call putc
        call putc
        mov ax, cx
        call hist_slot
        call print
        call newline
        pop cx
        dec cx
        jmp .l
.r:     ret

; ------------------------------------------------------------------ completion

; Complete the word before the cursor.
complete:
        ; The word starts after the previous space.
        mov bx, [ed_pos]
.ws:    test bx, bx
        jz .wsd
        cmp byte [linebuf+2+bx-1], ' '
        je .wsd
        dec bx
        jmp .ws
.wsd:   mov [cp_word], bx
        ; The first word is a command; later words are files.
        mov byte [cp_first], 1
        xor si, si
.fw:    cmp si, bx
        jae .fwd
        cmp byte [linebuf+2+si], ' '
        je .fwn
        mov byte [cp_first], 0
.fwn:   inc si
        jmp .fw
.fwd:   ; The name part follows the last '\' or ':'.
        mov [cp_name], bx
        mov byte [cp_dot], 0
        mov si, bx
.ns:    cmp si, [ed_pos]
        jae .nsd
        mov al, [linebuf+2+si]
        cmp al, '.'
        jne .nsdot
        mov byte [cp_dot], 1
.nsdot: cmp al, '\'
        je .sep
        cmp al, ':'
        jne .nsn
.sep:   lea ax, [si+1]
        mov [cp_name], ax
        mov byte [cp_dot], 0
.nsn:   inc si
        jmp .ns
.nsd:   ; Search pattern: the word + "*.*" ("*" if it has a dot already).
        mov di, path2
        mov si, [cp_word]
.cpw:   cmp si, [ed_pos]
        jae .cpd
        mov al, [linebuf+2+si]
        mov [di], al
        inc di
        inc si
        jmp .cpw
.cpd:   mov byte [di], '*'
        inc di
        cmp byte [cp_dot], 0
        jne .pat
        mov word [di], '.*'
        add di, 2
.pat:   mov byte [di], 0
        mov word [cp_count], 0
        mov word [cp_ptr], cp_matches
        ; Internal commands, for the first word without a path.
        cmp byte [cp_first], 0
        je .files
        mov ax, [cp_name]
        cmp ax, [cp_word]
        jne .files
        mov bx, commands
.ic:    mov si, [bx]
        test si, si
        jz .files
        call cp_is_prefix
        jc .icn
        mov si, [bx]
        xor al, al
        call cp_add
.icn:   add bx, 4
        jmp .ic
.files: mov dx, path2
        mov cx, 0x16
        mov ah, 0x4E
        int 0x21
        jc .got
.f:     mov si, dta+30
        cmp byte [si], '.'
        je .fn
        cmp byte [cp_first], 0
        je .fa
        test byte [dta+21], 0x10
        jnz .fa
        call is_program
        jc .fn
.fa:    mov si, dta+30
        mov al, [dta+21]
        call cp_add
.fn:    mov ah, 0x4F
        int 0x21
        jnc .f
.got:   mov cx, [cp_count]
        test cx, cx
        jnz .some
        ret                             ; nothing matches
.some:  ; CX = length of the common prefix of all matches.
        mov si, cp_matches
        call strlen
        mov dx, [cp_count]
        mov di, cp_matches
.cm:    dec dx
        jz .cmd
        add di, 14
        xor bx, bx
.cc:    cmp bx, cx
        jae .cm
        mov al, [cp_matches+bx]
        call upcase
        mov ah, al
        mov al, [di+bx]
        call upcase
        cmp al, ah
        jne .cut
        inc bx
        jmp .cc
.cut:   mov cx, bx
        jmp .cm
.cmd:   mov ax, [ed_pos]
        sub ax, [cp_name]               ; typed length of the name
        cmp cx, ax
        ja .extend
        cmp word [cp_count], 1
        je .suffix
        cmp byte [ed_tabs], 0
        je .r
        jmp cp_list                     ; second Tab: show the choices
.extend:
        ; Replace the typed name with the common prefix.
        push cx
        mov bx, [cp_name]
        mov cx, ax
        call ed_delete
        pop cx
        xor si, si
.ins:   cmp si, cx
        jae .ext_done
        mov al, [cp_matches+si]
        push si
        push cx
        call ed_insert
        pop cx
        pop si
        inc si
        jmp .ins
.ext_done:
        cmp word [cp_count], 1
        jne .r
.suffix:
        ; A single match: "\" after a directory, a space after anything else.
        mov al, '\'
        test byte [cp_matches+13], 0x10
        jnz .add
        mov bx, [ed_pos]
        cmp bx, [ed_len]
        jne .r
        mov al, ' '
.add:   jmp ed_insert
.r:     ret

; CF clear if the typed name (cp_name..ed_pos) is a prefix of the name at SI.
cp_is_prefix:
        push bx
        mov bx, [cp_name]
.l:     cmp bx, [ed_pos]
        jae .yes
        mov al, [linebuf+2+bx]
        call upcase
        mov ah, al
        lodsb
        call upcase
        cmp al, ah
        jne .no
        inc bx
        jmp .l
.yes:   pop bx
        clc
        ret
.no:    pop bx
        stc
        ret

; Add the name at SI (attribute AL) to the matches.
cp_add:
        cmp word [cp_count], CP_MAX
        jae .r
        mov di, [cp_ptr]
        mov [di+13], al
        mov cx, 13
.c:     lodsb
        stosb
        test al, al
        jz .z
        loop .c
        mov byte [di-1], 0
.z:     add word [cp_ptr], 14
        inc word [cp_count]
.r:     ret

; CF clear if the name at SI ends in .COM, .EXE or .BAT.
is_program:
.d:     lodsb
        test al, al
        jz .no
        cmp al, '.'
        jne .d
        mov eax, [si-1]
        cmp eax, '.COM'
        je .yes
        cmp eax, '.EXE'
        je .yes
        cmp eax, '.BAT'
        je .yes
.no:    stc
        ret
.yes:   clc
        ret

; CX = length of the NUL-terminated string at SI.
strlen: xor cx, cx
.l:     cmp byte [si], 0
        je .r
        inc si
        inc cx
        jmp .l
.r:     sub si, cx
        ret

; List the matches in columns, then show the prompt and line again.
cp_list:
        mov ax, [ed_len]
        mov [ed_pos], ax
        call ed_place
        call newline
        mov si, cp_matches
        mov cx, [cp_count]
        xor dx, dx                      ; column
.l:     push cx
        push si
        call strlen
        call print
        test byte [si+13], 0x10
        jz .pad
        mov al, '\'
        call putc
        inc cx
.pad:   inc dx
        cmp dx, 5
        jb .sp
        xor dx, dx
        call newline
        jmp .next
.sp:    mov al, ' '
.spl:   call putc
        inc cx
        cmp cx, 16
        jb .spl
.next:  pop si
        add si, 14
        pop cx
        loop .l
        test dx, dx
        jz .p
        call newline
.p:     call prompt
        jmp ed_home_here
