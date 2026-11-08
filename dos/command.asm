; =============================================================================
; TinyDOS COMMAND.COM - the command interpreter
;
; Internal commands: CD CLS COPY DATE DEL DIR ECHO EXIT HELP HISTORY MD MEM
; PAUSE RD REM REN TIME TYPE VER, drive changes (A:), and .COM/.EXE/.BAT
; programs from the current directory or the root. Runs AUTOEXEC.BAT when
; started with /P. The command line has history and Tab completion
; (lineedit.inc).
; =============================================================================

        bits 16
        cpu 486
        org 0x100

start:  mov sp, stack_top
        mov bx, (prog_end - $$ + 0x100 + 15) / 16
        mov ah, 0x4A                    ; give unused memory back to DOS
        int 0x21
        mov dx, dta
        mov ah, 0x1A
        int 0x21
        ; "/P" (permanent) on the command line: we're the primary shell.
        mov si, 0x81
.opt:   lodsb
        cmp al, 13
        je .opts_done
        cmp al, '/'
        jne .opt
        lodsb
        or al, 0x20
        cmp al, 'p'
        jne .opt
        mov byte [permanent], 1
        jmp .opt
.opts_done:
        cmp byte [permanent], 0
        je main_loop
        mov si, autoexec_name
        call file_exists
        jc main_loop
        mov si, autoexec_name
        call run_batch

main_loop:
        mov sp, stack_top
        call prompt
        call read_line
        call newline
        mov si, linebuf+2
        call execute
        jmp main_loop

; Print "A:\PATH>".
prompt: mov ah, 0x19
        int 0x21
        add al, 'A'
        mov [cwdbuf], al
        mov word [cwdbuf+1], ':\'
        mov si, cwdbuf+3
        xor dl, dl
        mov ah, 0x47
        int 0x21
        mov si, cwdbuf
        call print
        mov al, '>'
        jmp putc

; ------------------------------------------------------------------ execution

; Execute the command line at DS:SI (NUL-terminated).
execute:
        call skip_spaces
        cmp byte [si], '@'
        jne .noat
        inc si
        call skip_spaces
.noat:  cmp byte [si], 0
        je .r
        ; Command word -> cmdname (upper case).
        mov di, cmdname
        xor cx, cx
.w:     mov al, [si]
        cmp al, ' '
        jbe .wend
        cmp al, '/'
        je .wend
        cmp al, '.'
        jne .notdot
        cmp cx, 4                       ; "ECHO." prints an empty line
        jne .notdot
        cmp dword [cmdname], 'ECHO'
        jne .notdot
        jmp .wend
.notdot:
        call upcase
        stosb
        inc si
        inc cx
        cmp cx, 60
        jb .w
.wend:  mov byte [di], 0
        mov [args], si
        call skip_spaces
        mov [args_trim], si
        ; Drive change "X:"
        cmp cx, 2
        jne .internal
        cmp byte [cmdname+1], ':'
        jne .internal
        mov dl, [cmdname]
        sub dl, 'A'
        mov ah, 0x0E
        int 0x21
        mov ah, 0x19
        int 0x21
        cmp al, dl
        je .r
        mov si, msg_baddrive
        jmp print
.internal:
        mov bx, commands
.find:  mov di, [bx]
        test di, di
        jz .external
        mov si, cmdname
.cmp:   mov al, [si]
        cmp al, [di]
        jne .nextcmd
        test al, al
        jz .found
        inc si
        inc di
        jmp .cmp
.nextcmd:
        add bx, 4
        jmp .find
.found: mov si, [args_trim]
        jmp [bx+2]
.external:
        jmp run_external
.r:     ret

commands:
        dw s_cd, cmd_cd, s_chdir, cmd_cd, s_cls, cmd_cls, s_copy, cmd_copy
        dw s_date, cmd_date, s_del, cmd_del, s_erase, cmd_del, s_dir, cmd_dir
        dw s_echo, cmd_echo, s_exit, cmd_exit, s_help, cmd_help, s_history, cmd_history
        dw s_md, cmd_md
        dw s_mkdir, cmd_md, s_mem, cmd_mem, s_pause, cmd_pause, s_rd, cmd_rd
        dw s_rmdir, cmd_rd, s_rem, cmd_rem, s_ren, cmd_ren, s_rename, cmd_ren
        dw s_time, cmd_time, s_type, cmd_type, s_ver, cmd_ver
        dw 0, 0

s_cd     db 'CD', 0
s_chdir  db 'CHDIR', 0
s_cls    db 'CLS', 0
s_copy   db 'COPY', 0
s_date   db 'DATE', 0
s_del    db 'DEL', 0
s_erase  db 'ERASE', 0
s_dir    db 'DIR', 0
s_echo   db 'ECHO', 0
s_exit   db 'EXIT', 0
s_help   db 'HELP', 0
s_history db 'HISTORY', 0
s_md     db 'MD', 0
s_mkdir  db 'MKDIR', 0
s_mem    db 'MEM', 0
s_pause  db 'PAUSE', 0
s_rd     db 'RD', 0
s_rmdir  db 'RMDIR', 0
s_rem    db 'REM', 0
s_ren    db 'REN', 0
s_rename db 'RENAME', 0
s_time   db 'TIME', 0
s_type   db 'TYPE', 0
s_ver    db 'VER', 0

; Run a program: cmdname[.COM|.EXE|.BAT] in the current directory or root.
run_external:
        mov si, cmdname
        call has_dot
        jc .tryexts                     ; no extension given: try .COM/.EXE/.BAT
        ; Only programs run: anything else (README.TXT) would be executed as
        ; machine code and hang the machine.
        mov si, cmdname
        call is_program
        jnc .program
        mov si, cmdname
        call file_exists
        jc .bad
        mov si, msg_notprog1
        call print
        mov si, cmdname
        call print
        mov si, msg_notprog2
        call print
        mov si, cmdname
        call print
        jmp newline
.program:
        mov si, cmdname
        mov di, progpath
        call strcpy
        call try_run
        jnc .done
        mov di, progpath                ; try the root
        mov byte [di], '\'
        inc di
        mov si, cmdname
        call strcpy
        call try_run
        jnc .done
        jmp .bad
.tryexts:
        mov bx, exts
.ext:   mov si, cmdname
        mov di, progpath
        call strcpy
        dec di
        mov si, [bx]
        test si, si
        jz .root
        call strcpy
        call try_run
        jnc .done
        add bx, 2
        jmp .ext
.root:  mov bx, exts
.rext:  mov di, progpath
        mov byte [di], '\'
        inc di
        mov si, cmdname
        call strcpy
        dec di
        mov si, [bx]
        test si, si
        jz .bad
        call strcpy
        call try_run
        jnc .done
        add bx, 2
        jmp .rext
.bad:   mov si, msg_badcmd
        call print
.done:  ret

exts    dw ext_com, ext_exe, ext_bat, 0
ext_com db '.COM', 0
ext_exe db '.EXE', 0
ext_bat db '.BAT', 0

; Run progpath if it exists. CF if not found.
try_run:
        push bx
        mov si, progpath
        call file_exists
        jc .r
        ; Batch file?
        mov si, progpath
.e:     lodsb
        test al, al
        jnz .e
        cmp dword [si-5], '.BAT'
        jne .prog
        mov si, progpath
        call run_batch
        clc
        jmp .r
.prog:  ; Command tail: length + text after the command name + CR.
        mov si, [args]
        mov di, cmdtail+1
        xor cx, cx
.t:     lodsb
        test al, al
        jz .te
        stosb
        inc cx
        cmp cx, 125
        jb .t
.te:    mov byte [di], 13
        mov [cmdtail], cl
        mov word [epb], 0
        mov word [epb+2], cmdtail
        mov [epb+4], cs
        mov word [epb+6], 0x5C
        mov [epb+8], cs
        mov word [epb+10], 0x6C
        mov [epb+12], cs
        mov dx, progpath
        mov bx, epb
        mov ax, 0x4B00
        int 0x21
        jnc .ran
        call print_error
.ran:   push cs
        pop ds
        push cs
        pop es
        mov dx, dta                     ; the child may have moved the DTA
        mov ah, 0x1A
        int 0x21
        clc
.r:     pop bx
        ret

; ------------------------------------------------------------------ batch

; Run the batch file named at DS:SI.
run_batch:
        mov dx, si
        mov ax, 0x3D00
        int 0x21
        jc .r
        mov bx, ax
        mov dx, batchbuf
        mov cx, 4095
        mov ah, 0x3F
        int 0x21
        push ax
        mov ah, 0x3E
        int 0x21
        pop bx
        mov byte [batchbuf+bx], 0
        mov si, batchbuf
.line:  cmp byte [si], 0
        je .r
        ; Copy one line into linebuf+2.
        mov di, linebuf+2
.c:     lodsb
        test al, al
        jz .eol0
        cmp al, 10
        je .eol
        cmp al, 13
        je .c
        cmp al, 26
        je .eol0
        stosb
        jmp .c
.eol0:  dec si
.eol:   mov byte [di], 0
        push si
        cmp byte [echo_on], 0
        je .quiet
        cmp byte [linebuf+2], '@'
        je .quiet
        cmp byte [linebuf+2], 0
        je .quiet
        call prompt
        mov si, linebuf+2
        call print
        call newline
.quiet: mov si, linebuf+2
        call execute
        pop si
        jmp .line
.r:     ret

; ------------------------------------------------------------------ commands

cmd_rem:
        ret

cmd_exit:
        cmp byte [permanent], 0
        jne .r
        mov ax, 0x4C00
        int 0x21
.r:     ret

cmd_ver:
        mov si, msg_ver
        jmp print

cmd_cls:
        mov ah, 0x0F
        int 0x10
        xor ah, ah
        int 0x10
        ret

cmd_echo:
        cmp byte [si], 0
        jne .text
        mov si, [args]
        cmp byte [si], '.'
        je newline
        mov si, msg_echo_on
        cmp byte [echo_on], 0
        jne print
        mov si, msg_echo_off
        jmp print
.text:  mov eax, [si]
        and eax, 0x00DFDFDF
        cmp eax, 'OFF'
        jne .on
        mov byte [echo_on], 0
        ret
.on:    and eax, 0x0000DFDF
        cmp eax, 'ON'
        jne .say
        cmp byte [si+2], ' '
        ja .say
        mov byte [echo_on], 1
        ret
.say:   call print
        jmp newline

cmd_pause:
        mov si, msg_pause
        call print
        mov ah, 0x08
        int 0x21
        jmp newline

cmd_help:
        mov si, msg_help
        jmp print

cmd_date:
        mov ah, 0x2A
        int 0x21
        push dx
        push cx
        mov si, msg_date
        call print
        pop ax                          ; year
        pop dx
        push ax
        movzx eax, dh
        call print_2
        mov al, '-'
        call putc
        movzx eax, dl
        call print_2
        mov al, '-'
        call putc
        pop ax
        movzx eax, ax
        call print_num
        jmp newline

cmd_time:
        mov ah, 0x2C
        int 0x21
        push dx
        push cx
        mov si, msg_time
        call print
        pop cx
        movzx eax, ch
        call print_2
        mov al, ':'
        call putc
        movzx eax, cl
        call print_2
        mov al, ':'
        call putc
        pop dx
        movzx eax, dh
        call print_2
        jmp newline

cmd_cd: cmp byte [si], 0
        jne .chdir
        call prompt_path
        jmp newline
.chdir: call arg_to_path
        mov dx, path1
        mov ah, 0x3B
        int 0x21
        jnc .r
        mov si, msg_baddir
        call print
.r:     ret

; Print "A:\PATH" (no '>').
prompt_path:
        mov ah, 0x19
        int 0x21
        add al, 'A'
        mov [cwdbuf], al
        mov word [cwdbuf+1], ':\'
        mov si, cwdbuf+3
        xor dl, dl
        mov ah, 0x47
        int 0x21
        mov si, cwdbuf
        jmp print

cmd_md: call arg_to_path
        mov dx, path1
        mov ah, 0x39
        int 0x21
        jnc .r
        mov si, msg_mdfail
        call print
.r:     ret

cmd_rd: call arg_to_path
        mov dx, path1
        mov ah, 0x3A
        int 0x21
        jnc .r
        mov si, msg_rdfail
        call print
.r:     ret

cmd_ren:
        call arg_to_path                ; path1 = old
        call skip_word
        call skip_spaces
        mov di, path2
        call copy_word
        mov dx, path1
        mov di, path2
        push ds
        pop es
        mov ah, 0x56
        int 0x21
        jnc .r
        call print_error
.r:     ret

cmd_type:
        call arg_to_path
        mov dx, path1
        mov ax, 0x3D00
        int 0x21
        jc .err
        mov bx, ax
.l:     mov dx, iobuf
        mov cx, 512
        mov ah, 0x3F
        int 0x21
        jc .close
        test ax, ax
        jz .close
        mov cx, ax
        push bx
        mov bx, 1
        mov ah, 0x40
        int 0x21
        pop bx
        jmp .l
.close: mov ah, 0x3E
        int 0x21
        ret
.err:   jmp print_error

cmd_del:
        call arg_to_path
        ; Directory part of the pattern (to build each file's path).
        call split_dir
        mov dx, path1
        mov cx, 0
        mov ah, 0x4E
        int 0x21
        jc .none
.each:  mov si, dirpart
        mov di, path2
        call strcpy
        dec di
        mov si, dta+30
        call strcpy
        mov dx, path2
        mov ah, 0x41
        int 0x21
        mov ah, 0x4F
        int 0x21
        jnc .each
        ret
.none:  mov si, msg_notfound
        jmp print

cmd_copy:
        call arg_to_path                ; source
        call skip_word
        call skip_spaces
        mov di, path2
        call copy_word
        cmp byte [path2], 0
        jne .havedst
        mov si, msg_copyusage
        jmp print
.havedst:
        ; Destination "." or ending in '\' or ':' -> append the source name.
        mov si, path2
.e:     lodsb
        test al, al
        jnz .e
        mov al, [si-2]
        cmp al, '\'
        je .append
        cmp al, ':'
        je .append
        cmp word [path2], '.'
        je .dot
        jmp .open
.dot:   mov byte [path2], 0
        lea si, [path2]
        inc si
.append:
        dec si
        mov di, si
        mov si, path1
        call base_name
        call strcpy
.open:  mov dx, path1
        mov ax, 0x3D00
        int 0x21
        jc .err
        mov [h_src], ax
        mov dx, path2
        xor cx, cx
        mov ah, 0x3C
        int 0x21
        jc .errclose
        mov [h_dst], ax
.l:     mov bx, [h_src]
        mov dx, iobuf
        mov cx, 4096
        mov ah, 0x3F
        int 0x21
        jc .done
        test ax, ax
        jz .done
        mov cx, ax
        mov bx, [h_dst]
        mov ah, 0x40
        int 0x21
        jmp .l
.done:  mov bx, [h_dst]
        mov ah, 0x3E
        int 0x21
        mov bx, [h_src]
        mov ah, 0x3E
        int 0x21
        mov si, msg_copied
        jmp print
.errclose:
        push ax
        mov bx, [h_src]
        mov ah, 0x3E
        int 0x21
        pop ax
.err:   jmp print_error

cmd_mem:
        mov bx, 0xFFFF
        mov ah, 0x48
        int 0x21                        ; fails: BX = largest free block
        movzx eax, bx
        shl eax, 4
        call print_num
        mov si, msg_mem
        jmp print

; DIR [path] [/W] [/P]: /W lists names in columns, /P pauses each screen.
cmd_dir:
        mov byte [dir_wide], 0
        mov byte [dir_page], 0
        mov byte [dir_lines], 0
        mov byte [dir_col], 0
        mov byte [path1], 0
        mov si, [args_trim]
.arg:   call skip_spaces
        cmp byte [si], 0
        je .args_done
        cmp byte [si], '/'
        jne .path
        mov al, [si+1]
        call upcase
        cmp al, 'W'
        jne .notw
        mov byte [dir_wide], 1
.notw:  cmp al, 'P'
        jne .skip
        mov byte [dir_page], 1
        jmp .skip
.path:  cmp byte [path1], 0
        jne .skip
        mov di, path1
        call copy_word
.skip:  call skip_word
        jmp .arg
.args_done:
        cmp byte [path1], 0
        jne .given
        mov dword [path1], '*.*'
        jmp .search
.given: ; A directory name (no wildcards) lists its contents.
        mov si, path1
        call has_wild
        jnc .search
        mov si, path1
.end:   lodsb
        test al, al
        jnz .end
        cmp byte [si-2], '\'
        je .star
        cmp byte [si-2], ':'
        je .star
        mov dx, path1
        mov ax, 0x4300
        int 0x21
        jc .search
        test cl, 0x10
        jz .search
        mov byte [si-1], '\'
        inc si
.star:  dec si
        mov dword [si], '*.*'
.search:
        call split_dir
        mov si, msg_dirof
        call print
        cmp byte [dirpart], 0
        je .cwd
        cmp byte [dirpart+1], ':'
        je .named
        cmp byte [dirpart], '\'
        je .named
.cwd:   call prompt_path
        cmp byte [dirpart], 0
        je .nl
        mov si, cwdbuf
.cwdend:
        lodsb
        test al, al
        jnz .cwdend
        cmp byte [si-2], '\'
        je .rel
        mov al, '\'
        call putc
.rel:   mov si, dirpart
        call print
        jmp .nl
.named: mov si, dirpart
        call print
.nl:    call newline
        call newline
        mov dword [dir_files], 0
        mov dword [dir_bytes], 0
        mov dx, path1
        mov cx, 0x16                    ; include hidden, system, directories
        mov ah, 0x4E
        int 0x21
        jc .summary
.entry: cmp byte [dir_wide], 0
        jne .wide
        call dir_line
        call dir_more
        jmp .nextent
.wide:  call dir_wide_entry
.nextent:
        mov ah, 0x4F
        int 0x21
        jnc .entry
        cmp byte [dir_col], 0
        je .summary
        call newline
.summary:
        mov eax, [dir_files]
        call print_num
        mov si, msg_files
        call print
        mov eax, [dir_bytes]
        call print_num
        mov si, msg_bytes
        call print
        ; Free space on the drive.
        xor dl, dl
        mov si, path1
        cmp byte [si+1], ':'
        jne .df
        mov dl, [si]
        or dl, 0x20
        sub dl, 'a' - 1
.df:    mov ah, 0x36
        int 0x21
        cmp ax, 0xFFFF
        je .r
        movzx eax, ax
        movzx ebx, bx
        imul eax, ebx
        movzx ecx, cx
        imul eax, ecx
        call print_num
        mov si, msg_free
        call print
.r:     ret

; DIR /W: the name from the DTA in a 16-column cell, five per line.
dir_wide_entry:
        mov si, dta+30
        call strlen
        test byte [dta+21], 0x10
        jz .file
        mov al, '['
        call putc
        call print
        mov al, ']'
        call putc
        add cx, 2
        jmp .count
.file:  call print
        mov eax, [dta+26]
        add [dir_bytes], eax
.count: inc dword [dir_files]
        inc byte [dir_col]
        cmp byte [dir_col], 5
        jb .pad
        mov byte [dir_col], 0
        call newline
        jmp dir_more
.pad:   mov al, ' '
.p:     call putc
        inc cx
        cmp cx, 16
        jb .p
        ret

; DIR /P: after a screenful of lines, wait for a key.
dir_more:
        cmp byte [dir_page], 0
        je .r
        inc byte [dir_lines]
        cmp byte [dir_lines], 22
        jb .r
        mov byte [dir_lines], 0
        push si
        mov si, msg_pause
        call print
        mov ah, 0x08
        int 0x21
        call newline
        pop si
.r:     ret

; One DIR line from the DTA: NAME     EXT    <DIR>/size  date  time
dir_line:
        mov si, dta+30
        mov di, outline
        mov cx, 9
        mov al, ' '
        rep stosb
        mov cx, 4
        rep stosb
        mov di, outline
.n:     lodsb
        test al, al
        jz .nd
        cmp al, '.'
        je .ext
        stosb
        jmp .n
.ext:   mov di, outline+9
.x:     lodsb
        test al, al
        jz .nd
        stosb
        jmp .x
.nd:    cmp byte [dta+30], '.'
        jne .print
        mov dword [outline+9], '    '   ; "." / ".." entries: no extension
        mov word [outline], '. '
        cmp byte [dta+31], '.'
        jne .print
        mov word [outline], '..'
.print: mov byte [outline+13], 0
        mov si, outline
        call print
        test byte [dta+21], 0x10
        jz .file
        mov si, msg_dirtag
        call print
        jmp .date
.file:  mov eax, [dta+26]
        add [dir_bytes], eax
        mov cx, 10
        call print_num_w
.date:  inc dword [dir_files]
        mov al, ' '
        call putc
        call putc
        mov ax, [dta+24]                ; date
        push ax
        shr ax, 5
        and ax, 15
        movzx eax, ax
        call print_2
        mov al, '-'
        call putc
        pop ax
        push ax
        and ax, 31
        movzx eax, ax
        call print_2
        mov al, '-'
        call putc
        pop ax
        shr ax, 9
        add ax, 1980
        movzx eax, ax
        call print_num
        mov al, ' '
        call putc
        call putc
        mov ax, [dta+22]                ; time
        push ax
        shr ax, 11
        movzx eax, ax
        call print_2
        mov al, ':'
        call putc
        pop ax
        shr ax, 5
        and ax, 63
        movzx eax, ax
        call print_2
        jmp newline

; ------------------------------------------------------------------ helpers

; Copy the first argument word at SI into path1.
arg_to_path:
        mov di, path1
copy_word:
        push si
.c:     lodsb
        cmp al, ' '
        jbe .e
        stosb
        jmp .c
.e:     mov byte [di], 0
        pop si
        ret

skip_word:
.l:     cmp byte [si], ' '
        jbe .r
        inc si
        jmp .l
.r:     ret

skip_spaces:
.l:     cmp byte [si], ' '
        jne .r
        inc si
        jmp .l
.r:     ret

; Split path1 into dirpart (everything up to the last '\' or ':').
split_dir:
        mov si, path1
        mov di, dirpart
        xor bx, bx                      ; length of the directory part
        xor cx, cx
.l:     lodsb
        test al, al
        jz .e
        inc cx
        cmp al, '\'
        je .mark
        cmp al, ':'
        jne .l
.mark:  mov bx, cx
        jmp .l
.e:     mov si, path1
        mov cx, bx
        rep movsb
        mov byte [di], 0
        ret

; SI -> pointer to the file name part of the path at SI.
base_name:
        mov bx, si
.l:     lodsb
        test al, al
        jz .e
        cmp al, '\'
        je .m
        cmp al, ':'
        jne .l
.m:     mov bx, si
        jmp .l
.e:     mov si, bx
        ret

; CF clear if the string at SI contains '.', set if not.
has_dot:
.l:     lodsb
        test al, al
        jz .no
        cmp al, '.'
        jne .l
        clc
        ret
.no:    stc
        ret

; CF clear if the string at SI contains '*' or '?'.
has_wild:
.l:     lodsb
        test al, al
        jz .no
        cmp al, '*'
        je .yes
        cmp al, '?'
        jne .l
.yes:   clc
        ret
.no:    stc
        ret

; CF clear if the file at DS:SI exists.
file_exists:
        mov dx, si
        mov cx, 0x06
        mov ah, 0x4E
        int 0x21
        ret

%include "lineedit.inc"

; Copy NUL-terminated SI to DI (DI ends after the NUL).
strcpy: lodsb
        stosb
        test al, al
        jnz strcpy
        ret

upcase: cmp al, 'a'
        jb .r
        cmp al, 'z'
        ja .r
        sub al, 32
.r:     ret

putc:   push ax
        push dx
        mov dl, al
        mov ah, 0x02
        int 0x21
        pop dx
        pop ax
        ret

print:  push ax
        push si
.l:     lodsb
        test al, al
        jz .e
        call putc
        jmp .l
.e:     pop si
        pop ax
        ret

newline:
        push si
        mov si, crlf
        call print
        pop si
        ret

; Print EAX in decimal.
print_num:
        xor cx, cx
print_num_w:                            ; right-aligned in CX columns
        pushad
        mov ebx, 10
        xor si, si
.d:     xor edx, edx
        div ebx
        push dx
        inc si
        test eax, eax
        jnz .d
        mov ax, cx
        sub ax, si
        jle .digits
        mov cx, ax
.pad:   mov al, ' '
        call putc
        loop .pad
.digits:
        mov cx, si
.o:     pop ax
        add al, '0'
        call putc
        loop .o
        popad
        ret

; Print EAX (0-99) as two digits.
print_2:
        push ax
        aam
        add ax, '00'
        xchg al, ah
        call putc
        mov al, ah
        call putc
        pop ax
        ret

print_error:
        mov si, msg_notfound
        cmp ax, 2
        je .p
        cmp ax, 3
        je .path
        cmp ax, 5
        je .access
        cmp ax, 8
        je .mem
        cmp ax, 11
        je .fmt
        mov si, msg_failed
        jmp .p
.path:  mov si, msg_baddir
        jmp .p
.access:
        mov si, msg_access
        jmp .p
.mem:   mov si, msg_nomem
        jmp .p
.fmt:   mov si, msg_badfmt
.p:     jmp print

; ------------------------------------------------------------------ data

crlf          db 13, 10, 0
autoexec_name db '\AUTOEXEC.BAT', 0
msg_ver       db 'TinyDOS version 1.0 (rusty486)', 13, 10, 0
msg_badcmd    db 'Bad command or file name', 13, 10, 0
msg_notprog1  db 'Not a program: ', 0
msg_notprog2  db '. To view it, type: TYPE ', 0
msg_baddrive  db 'Invalid drive specification', 13, 10, 0
msg_baddir    db 'Invalid directory', 13, 10, 0
msg_mdfail    db 'Unable to create directory', 13, 10, 0
msg_rdfail    db 'Invalid path, not directory, or directory not empty', 13, 10, 0
msg_notfound  db 'File not found', 13, 10, 0
msg_access    db 'Access denied', 13, 10, 0
msg_nomem     db 'Not enough memory', 13, 10, 0
msg_badfmt    db 'Invalid program file', 13, 10, 0
msg_failed    db 'Operation failed', 13, 10, 0
msg_copied    db '        1 file(s) copied', 13, 10, 0
msg_copyusage db 'Usage: COPY source destination', 13, 10, 0
msg_mem       db ' bytes free conventional memory', 13, 10, 0
msg_dirof     db ' Directory of ', 0
msg_dirtag    db '    <DIR>  ', 0
msg_files     db ' file(s)  ', 0
msg_bytes     db ' bytes', 13, 10, 0
msg_free      db ' bytes free', 13, 10, 0
msg_echo_on   db 'ECHO is on', 13, 10, 0
msg_echo_off  db 'ECHO is off', 13, 10, 0
msg_pause     db 'Press any key to continue . . .', 0
msg_date      db 'Current date is ', 0
msg_time      db 'Current time is ', 0
msg_help      db 'TinyDOS commands:', 13, 10
              db '  CD [dir]       change directory     CLS            clear the screen', 13, 10
              db '  COPY src dst   copy a file          DATE / TIME    show date / time', 13, 10
              db '  DEL file       delete files         DIR [path]     list files (/W /P)', 13, 10
              db '  ECHO [text]    print text           EXIT           leave this shell', 13, 10
              db '  MD dir         make a directory     MEM            free memory', 13, 10
              db '  PAUSE          wait for a key       RD dir         remove a directory', 13, 10
              db '  REN old new    rename a file        TYPE file      show a text file', 13, 10
              db '  VER            version              X:             change drive', 13, 10
              db '  HISTORY        list recent commands', 13, 10
              db 'Programs (.COM, .EXE, .BAT) run from the current directory or the root.', 13, 10
              db 'Keys: Up/Down history, F3 last command, Tab completes names (twice: list),', 13, 10
              db '      Left/Right/Home/End/Ctrl+arrows move, Ins overwrite, Esc clears.', 13, 10, 0

permanent     db 0
echo_on       db 1
h_src         dw 0
h_dst         dw 0
dir_files     dd 0
dir_wide      db 0
dir_page      db 0
dir_lines     db 0
dir_col       db 0
dir_bytes     dd 0
args          dw 0
args_trim     dw 0
epb           times 14 db 0
cmdname       times 64 db 0
progpath      times 80 db 0
cmdtail       times 128 db 0
cwdbuf        times 72 db 0
path1         times 80 db 0
path2         times 80 db 0
dirpart       times 80 db 0
outline       times 16 db 0
linebuf       times 130 db 0
; Line editor state (lineedit.inc).
ed_len        dw 0
ed_pos        dw 0
ed_old        dw 0
ed_back       dw 0
ed_row        db 0
ed_col        db 0
ed_tabs       db 0
ed_over       db 0
hist_count    dw 0
hist_next     dw 0
cp_word       dw 0
cp_name       dw 0
cp_first      db 0
cp_dot        db 0
cp_count      dw 0
cp_ptr        dw 0
cp_matches    times CP_MAX * 14 db 0
hist_buf      times HIST_N * HIST_SZ db 0
dta           times 128 db 0
iobuf         times 4096 db 0
batchbuf      times 4096 db 0
              times 512 db 0
stack_top:
prog_end:
