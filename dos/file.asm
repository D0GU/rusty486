; =============================================================================
; TinyDOS files and directories
; =============================================================================

; ---------------------------------------------------------------- names

; Parse one path component at FS:SI into comp83 (11 bytes, space padded,
; '*' expanded to '?'). Returns SI at the terminator, AL = terminator
; ('\', '/' or 0). CF if the name is invalid.
parse_component:
        push cx
        push di
        mov di, comp83
        mov cx, 11
        mov al, ' '
        push es
        push ds
        pop es
        rep stosb
        pop es
        cmp byte [fs:si], '.'
        jne .normal
        ; "." or ".."
        mov byte [comp83], '.'
        inc si
        cmp byte [fs:si], '.'
        jne .dotend
        mov byte [comp83+1], '.'
        inc si
.dotend:
        mov al, [fs:si]
        call is_sep
        jne .bad
        jmp .ok
.normal:
        xor di, di                      ; index into comp83
        xor cl, cl                      ; 0 = name part, 1 = extension
.l:     mov al, [fs:si]
        call is_sep
        je .ok
        inc si
        cmp al, '.'
        jne .notdot
        test cl, cl
        jnz .bad
        mov cl, 1
        mov di, 8
        jmp .l
.notdot:
        cmp al, '*'
        jne .notstar
.star:  test cl, cl
        jnz .starext
        cmp di, 8
        jae .l
        mov byte [comp83+di], '?'
        inc di
        jmp .star
.starext:
        cmp di, 11
        jae .l
        mov byte [comp83+di], '?'
        inc di
        jmp .starext
.notstar:
        cmp al, ' '
        jbe .bad
        cmp al, 'a'
        jb .upper
        cmp al, 'z'
        ja .upper
        sub al, 32
.upper: test cl, cl
        jnz .ext
        cmp di, 8
        jae .l                          ; truncate long names, like DOS
        jmp .store
.ext:   cmp di, 11
        jae .l
.store: mov [comp83+di], al
        inc di
        jmp .l
.ok:    mov al, [fs:si]
        clc
        pop di
        pop cx
        ret
.bad:   stc
        pop di
        pop cx
        ret

; ZF if AL is a path terminator: 0, '\' or '/'.
is_sep: test al, al
        je .r
        cmp al, '\'
        je .r
        cmp al, '/'
.r:     ret

; ---------------------------------------------------------------- resolution

; Resolve the path at FS:SI. Sets [dp], [res_dir] (parent directory cluster,
; 0 = root), comp83 (final name) and [res_name] (1 if there is a final name).
; CF with AX = error on failure.
resolve:
        mov al, [cur_drive]
        cmp byte [fs:si], 0
        je .gotdrv
        cmp byte [fs:si+1], ':'
        jne .gotdrv
        mov al, [fs:si]
        or al, 0x20
        sub al, 'a'
        add si, 2
.gotdrv:
        call select_drive
        jc .err
        mov bx, [dp]
        mov ax, [bx+D_CWD]
        mov cl, [fs:si]
        cmp cl, '\'
        je .abs
        cmp cl, '/'
        jne .rel
.abs:   xor ax, ax
        inc si
.rel:   mov [res_dir], ax
.comp:  call parse_component
        jc .badname
        test al, al
        jz .final
        inc si                          ; skip the separator
        cmp byte [comp83], ' '
        je .comp                        ; empty component ("a\\b")
        call descend
        jc .err
        jmp .comp
.final: mov byte [res_name], 0
        cmp byte [comp83], ' '
        je .done
        cmp byte [comp83], '.'
        jne .named
        call descend
        jc .err
        jmp .done
.named: mov byte [res_name], 1
.done:  clc
        ret
.badname:
        mov ax, E_NOPATH
.err:   stc
        ret

; Move [res_dir] into the subdirectory named by comp83. CF/E_NOPATH if absent.
descend:
        cmp word [comp83], '. '
        je .same
        mov ax, [res_dir]
        cmp word [comp83], '..'
        jne .lookup
        test ax, ax
        jz .same                        ; ".." at the root stays there
.lookup:
        mov si, comp83
        mov dl, A_DIR | A_HIDDEN | A_SYSTEM
        xor cx, cx
        call find_entry
        jc .nopath
        test byte [bx+DE_ATTR], A_DIR
        jz .nopath
        mov ax, [bx+DE_CLUSTER]
        mov [res_dir], ax
.same:  clc
        ret
.nopath:
        mov ax, E_NOPATH
        stc
        ret

; ---------------------------------------------------------------- directories

; Entry CX of directory AX (cluster, 0 = root) -> BX = pointer into secbuf,
; EAX = its sector. CF past the end of the directory.
dir_entry:
        push cx
        push dx
        push si
        mov si, [dp]
        test ax, ax
        jnz .chain
        cmp cx, [si+D_ROOTENTS]
        jae .end
        movzx eax, cx
        shr eax, 4
        add eax, [si+D_ROOTSTART]
        jmp .load
.chain: ; entries per cluster = 16 << clshift
        push cx
        mov dx, cx
        mov cl, [si+D_CLSHIFT]
        add cl, 4
        shr dx, cl                      ; cluster number in the chain
        pop cx
.walk:  test dx, dx
        jz .here
        call fat_get
        cmp ax, 0xFFFF
        je .end
        dec dx
        jmp .walk
.here:  cmp ax, 2
        jb .end
        call cluster_lba
        push cx
        movzx edx, cx
        shr edx, 4
        mov cl, [si+D_CLSHIFT]
        mov bx, 1
        shl bx, cl
        dec bx
        and dx, bx                      ; sector within the cluster
        add eax, edx
        pop cx
.load:  call get_sector
        jc .end
        and cx, 15
        shl cx, 5
        add bx, cx
        clc
        jmp .r
.end:   stc
.r:     pop si
        pop dx
        pop cx
        ret

; Search directory AX from index CX for a name matching the 11-byte pattern
; at DS:SI ('?' wildcards). DL = attribute mask (hidden/system/directory
; entries only match if allowed; A_VOLUME searches for the label).
; Returns CX = index, BX = entry pointer. CF if not found.
find_entry:
        mov [fe_dir], ax
        mov [fe_pat], si
        mov [fe_mask], dl
.next:  mov ax, [fe_dir]
        call dir_entry
        jc .none
        mov al, [bx]
        test al, al
        jz .none                        ; end of directory
        cmp al, 0xE5
        je .skip
        mov al, [bx+DE_ATTR]
        cmp al, 0x0F
        je .skip                        ; long file name entry
        test al, A_VOLUME
        jz .notvol
        test byte [fe_mask], A_VOLUME
        jz .skip
        jmp .cmp
.notvol:
        test byte [fe_mask], A_VOLUME
        jz .attrs
        cmp byte [fe_mask], A_VOLUME
        je .skip                        ; label-only search
.attrs: mov ah, [fe_mask]
        not ah
        and al, ah
        test al, A_HIDDEN | A_SYSTEM | A_DIR
        jnz .skip
.cmp:   push cx
        mov si, [fe_pat]
        xor di, di
        mov cx, 11
.c:     mov al, [si]
        inc si
        cmp al, '?'
        je .cnext
        cmp al, [bx+di]
        jne .nomatch
.cnext: inc di
        loop .c
        pop cx
        clc
        ret
.nomatch:
        pop cx
.skip:  inc cx
        jmp .next
.none:  stc
        ret

; Find a free entry in directory AX, growing a subdirectory if needed.
; Returns CX = index, BX = entry pointer. CF/AX = error.
free_slot:
        mov [fe_dir], ax
        xor cx, cx
.l:     mov ax, [fe_dir]
        call dir_entry
        jc .grow
        mov al, [bx]
        test al, al
        jz .found
        cmp al, 0xE5
        je .found
        inc cx
        jmp .l
.found: clc
        ret
.grow:  mov ax, [fe_dir]
        test ax, ax
        jz .full
        ; Find the last cluster and append one.
.last:  mov dx, ax
        call fat_get
        cmp ax, 0xFFFF
        jne .last
        push cx
        call alloc_cluster              ; linked after DX
        pop cx
        jc .full
        jmp .l
.full:  mov ax, E_ACCESS
        stc
        ret

; Write a new directory entry at BX: comp83 name, attribute AL, cluster DX.
init_entry:
        push cx
        push si
        push di
        push es
        push ds
        pop es
        push ax
        mov di, bx
        mov si, comp83
        mov cx, 11
        rep movsb
        mov cx, 11                      ; clear bytes 11-21
        xor al, al
        rep stosb
        pop ax
        mov [bx+DE_ATTR], al
        mov [bx+DE_CLUSTER], dx
        mov dword [bx+DE_SIZE], 0
        push dx
        call dos_datetime
        mov [bx+DE_TIME], ax
        mov [bx+DE_DATE], dx
        pop dx
        call mark_dirty
        pop es
        pop di
        pop si
        pop cx
        ret

; Look up the resolved name in [res_dir]. Returns BX = entry, CF if absent.
lookup_name:
        cmp byte [res_name], 0
        je .none
        mov ax, [res_dir]
        mov si, comp83
        mov dl, A_DIR | A_HIDDEN | A_SYSTEM
        xor cx, cx
        jmp find_entry
.none:  stc
        ret

; ---------------------------------------------------------------- handles

sft_init:
        push ds
        pop es
        mov di, sft
        mov cx, NSFT * SFT_SIZE
        xor al, al
        rep stosb
        mov byte [sft+S_FLAGS], 2       ; entry 0: the console
        mov byte [sft+S_REFS], 5
        ret

; Handle BX of the current process -> SI = SFT entry. CF/E_HANDLE.
handle_sft:
        cmp bx, 20
        jae .bad
        push es
        mov es, [cur_psp]
        movzx si, byte [es:0x18+bx]
        pop es
        cmp si, NSFT
        jae .bad
        imul si, si, SFT_SIZE
        add si, sft
        cmp byte [si+S_FLAGS], 0
        je .bad
        clc
        ret
.bad:   mov ax, E_HANDLE
        stc
        ret

; Allocate an SFT entry (SI) and a handle in the current process (AX).
; The handle refers to the entry. CF/AX = error.
new_handle:
        mov si, sft
        xor dl, dl
.s:     cmp byte [si+S_FLAGS], 0
        je .gotsft
        add si, SFT_SIZE
        inc dl
        cmp dl, NSFT
        jb .s
        jmp .toomany
.gotsft:
        push es
        mov es, [cur_psp]
        xor bx, bx
.h:     cmp byte [es:0x18+bx], 0xFF
        je .gothandle
        inc bx
        cmp bx, 20
        jb .h
        pop es
.toomany:
        mov ax, E_TOOMANY
        stc
        ret
.gothandle:
        mov [es:0x18+bx], dl
        pop es
        push di
        push es
        push ds
        pop es
        mov di, si
        mov cx, SFT_SIZE
        xor al, al
        rep stosb
        pop es
        pop di
        mov byte [si+S_REFS], 1
        mov ax, bx
        clc
        ret

; Open the directory entry at BX (in secbuf) with access mode CL.
; Returns AX = handle. CF/AX = error.
open_entry:
        push bx
        mov [oe_mode], cl
        mov eax, [sc_lba]
        mov [oe_lba], eax
        mov [oe_off], bx
        call new_handle
        pop bx
        jc .r
        push ax
        ; Reload the entry (new_handle doesn't touch secbuf).
        mov byte [si+S_FLAGS], 1
        mov al, [dp_index]
        mov [si+S_DRIVE], al
        mov al, [oe_mode]
        mov [si+S_MODE], al
        mov al, [bx+DE_ATTR]
        mov [si+S_ATTR], al
        mov ax, [bx+DE_CLUSTER]
        mov [si+S_CLUSTER], ax
        mov [si+S_CURCL], ax
        mov word [si+S_CURIDX], 0
        mov eax, [bx+DE_SIZE]
        mov [si+S_SIZE], eax
        mov eax, [oe_lba]
        mov [si+S_DIRLBA], eax
        mov ax, [oe_off]
        sub ax, secbuf
        mov [si+S_DIROFF], ax
        pop ax
        clc
.r:     ret

; Write an SFT entry's size/cluster back to its directory entry.
update_dirent:
        cmp byte [si+S_DIRTY], 0
        je .r
        mov al, [si+S_DRIVE]
        call select_drive
        jc .r
        mov eax, [si+S_DIRLBA]
        call get_sector
        jc .r
        add bx, [si+S_DIROFF]
        mov ax, [si+S_CLUSTER]
        mov [bx+DE_CLUSTER], ax
        mov eax, [si+S_SIZE]
        mov [bx+DE_SIZE], eax
        or byte [bx+DE_ATTR], A_ARCHIVE
        call dos_datetime
        mov [bx+DE_TIME], ax
        mov [bx+DE_DATE], dx
        call mark_dirty
        mov byte [si+S_DIRTY], 0
.r:     ret

; Close handle BX of the current process.
close_handle:
        call handle_sft
        jc .r
        push es
        mov es, [cur_psp]
        mov byte [es:0x18+bx], 0xFF
        pop es
        dec byte [si+S_REFS]
        jnz .ok
        cmp byte [si+S_FLAGS], 1
        jne .free
        call update_dirent
        call flush_sector
        call fat_flush
.free:  mov byte [si+S_FLAGS], 0
.ok:    clc
.r:     ret

; Cluster containing the file position of SFT SI -> AX. With [grow] set,
; missing clusters are allocated. CF at end of chain / disk full.
pos_cluster:
        push cx
        push dx
        push di
        mov al, [si+S_DRIVE]
        call select_drive
        mov di, [dp]
        mov eax, [si+S_POS]
        mov cl, [di+D_CLSHIFT]
        add cl, 9
        shr eax, cl
        mov dx, ax                      ; wanted index
        mov ax, [si+S_CLUSTER]
        cmp ax, 2
        jae .have_first
        cmp byte [grow], 0
        je .eof
        push dx
        xor dx, dx
        call alloc_cluster
        pop dx
        jc .eof
        mov [si+S_CLUSTER], ax
        mov [si+S_CURCL], ax
        mov word [si+S_CURIDX], 0
        mov byte [si+S_DIRTY], 1
.have_first:
        xor cx, cx
        cmp dx, [si+S_CURIDX]
        jb .walk
        mov ax, [si+S_CURCL]
        mov cx, [si+S_CURIDX]
        cmp ax, 2
        jae .walk
        mov ax, [si+S_CLUSTER]
        xor cx, cx
.walk:  cmp cx, dx
        je .found
        push ax
        call fat_get
        cmp ax, 0xFFFF
        jne .step
        cmp byte [grow], 0
        je .eofpop
        pop ax
        push dx
        mov dx, ax
        call alloc_cluster              ; linked after the last one
        pop dx
        jc .eof
        jmp .stepped
.step:  add sp, 2
.stepped:
        inc cx
        jmp .walk
.eofpop:
        add sp, 2
.eof:   stc
        jmp .r
.found: mov [si+S_CURCL], ax
        mov [si+S_CURIDX], cx
        clc
.r:     pop di
        pop dx
        pop cx
        ret

; Sector holding the file position of SFT SI: EAX = LBA, DX = offset in it.
pos_sector:
        call pos_cluster
        jc .r
        call cluster_lba
        push ecx
        push bx
        mov bx, [dp]
        mov ecx, [si+S_POS]
        shr ecx, 9
        mov dl, [bx+D_SPC]
        dec dl
        and cl, dl
        movzx ecx, cl
        add eax, ecx
        mov dx, [si+S_POS]
        and dx, 511
        pop bx
        pop ecx
        clc
.r:     ret

; Read CX bytes from file SI into ES:DI. Returns AX = bytes read.
file_read:
        mov byte [grow], 0
        mov eax, [si+S_SIZE]
        sub eax, [si+S_POS]
        jbe .none
        movzx ecx, cx
        cmp ecx, eax
        jbe .ok
        mov cx, ax
.ok:    xor ax, ax
        mov [rw_total], ax
.l:     test cx, cx
        jz .done
        push cx
        call pos_sector
        pop cx
        jc .done
        call get_sector
        jc .done
        mov ax, 512
        sub ax, dx
        cmp ax, cx
        jbe .n
        mov ax, cx
.n:     push si
        push cx
        mov cx, ax
        mov si, bx
        add si, dx
        rep movsb
        pop cx
        pop si
        sub cx, ax
        add [rw_total], ax
        movzx eax, ax
        add [si+S_POS], eax
        jmp .l
.none:  xor ax, ax
        ret
.done:  mov ax, [rw_total]
        ret

; Write CX bytes from FS:DX to file SI. Returns AX = bytes written.
file_write:
        mov byte [grow], 1
        mov word [rw_total], 0
        mov [rw_src], dx
        test cx, cx
        jnz .l
        ; Zero-length write truncates at the current position.
        mov eax, [si+S_POS]
        mov [si+S_SIZE], eax
        mov byte [si+S_DIRTY], 1
        xor ax, ax
        ret
.l:     test cx, cx
        jz .done
        push cx
        call pos_sector
        pop cx
        jc .done
        mov [rw_lba], eax
        mov ax, 512
        sub ax, dx
        cmp ax, cx
        jbe .n
        mov ax, cx
.n:     mov [rw_n], ax
        mov eax, [rw_lba]
        cmp word [rw_n], 512
        je .whole
        call get_sector                 ; partial sector: read-modify-write
        jc .done
        jmp .copy
.whole: call claim_sector               ; whole sector: no need to read it
.copy:  mov ax, [rw_n]
        push si
        push di
        push cx
        push es
        push ds
        pop es
        mov cx, ax
        mov di, bx
        add di, dx
        mov si, [rw_src]
        push ds
        push fs
        pop ds
        rep movsb
        pop ds
        pop es
        pop cx
        pop di
        pop si
        call mark_dirty
        add [rw_src], ax
        sub cx, ax
        add [rw_total], ax
        movzx eax, ax
        add [si+S_POS], eax
        mov eax, [si+S_POS]
        cmp eax, [si+S_SIZE]
        jbe .l
        mov [si+S_SIZE], eax
        mov byte [si+S_DIRTY], 1
        jmp .l
.done:  mov byte [si+S_DIRTY], 1
        mov ax, [rw_total]
        ret

; ---------------------------------------------------------------- INT 21h

; AH=3Ch create, 5Bh create new, 6Ch extended open/create.
fn_3c:  mov byte [create_mode], 0       ; truncate existing
        jmp create_common
fn_5b:  mov byte [create_mode], 1       ; fail if it exists
create_common:
        call caller_dsdx
        mov cx, [bp+F_CX]
        mov [create_attr], cl
        call create_file
        jc fail
        mov [bp+F_AX], ax
        jmp ok

; Create/truncate the file at FS:SI with attribute [create_attr].
; [create_mode]: 0 truncate, 1 fail if present. Returns AX = handle.
create_file:
        call resolve
        jc .r
        cmp byte [res_name], 0
        je .noname
        call lookup_name
        jc .new
        cmp byte [create_mode], 0
        jne .exists
        test byte [bx+DE_ATTR], A_DIR | A_RDONLY | A_VOLUME
        jnz .access
        mov ax, [bx+DE_CLUSTER]
        mov word [bx+DE_CLUSTER], 0
        mov dword [bx+DE_SIZE], 0
        call mark_dirty
        push bx
        call free_chain                 ; touches only the FAT window
        pop bx
        jmp .open
.new:   mov ax, [res_dir]
        call free_slot
        jc .r
        mov al, [create_attr]
        or al, A_ARCHIVE
        xor dx, dx
        call init_entry
.open:  mov cl, 2                       ; read/write
        call open_entry
        ret
.exists:
        mov ax, E_EXISTS
        stc
        ret
.access:
        mov ax, E_ACCESS
        stc
        ret
.noname:
        mov ax, E_NOFILE
        stc
.r:     ret

fn_6c:  ; BL = mode, CX = attr, DL = action, DS:SI = name
        mov fs, [bp+F_DS]
        mov si, [bp+F_SI]
        mov al, [bp+F_DL]
        mov [ext_action], al
        mov cx, [bp+F_CX]
        mov [create_attr], cl
        push si
        call resolve
        pop si
        jc fail
        call lookup_name
        jc .absent
        mov al, [ext_action]
        and al, 0x0F
        jz .eexists
        cmp al, 2
        je .replace
        ; open
        mov cl, [bp+F_BL]
        and cl, 7
        call open_entry
        jc fail
        mov [bp+F_AX], ax
        mov word [bp+F_CX], 1
        jmp ok
.replace:
        mov byte [create_mode], 0
        call create_file
        jc fail
        mov [bp+F_AX], ax
        mov word [bp+F_CX], 3
        jmp ok
.absent:
        test byte [ext_action], 0x10
        jz .enofile
        mov byte [create_mode], 1
        call create_file
        jc fail
        mov [bp+F_AX], ax
        mov word [bp+F_CX], 2
        jmp ok
.eexists:
        mov ax, E_EXISTS
        jmp fail
.enofile:
        mov ax, E_NOFILE
        jmp fail

; AH=3Dh open DS:DX with mode AL.
fn_3d:  call caller_dsdx
        call resolve
        jc fail
        call lookup_name
        jc .nofile
        test byte [bx+DE_ATTR], A_DIR | A_VOLUME
        jnz .access
        mov cl, [bp+F_AL]
        test cl, 3
        jz .open
        test byte [bx+DE_ATTR], A_RDONLY
        jnz .access
.open:  call open_entry
        jc fail
        mov [bp+F_AX], ax
        jmp ok
.nofile:
        mov ax, E_NOFILE
        jmp fail
.access:
        mov ax, E_ACCESS
        jmp fail

fn_3e:  mov bx, [bp+F_BX]
        call close_handle
        jc fail
        jmp ok

; AH=3Fh read: BX handle, CX count, DS:DX buffer.
fn_3f:  mov bx, [bp+F_BX]
        call handle_sft
        jc fail
        mov es, [bp+F_DS]
        mov di, [bp+F_DX]
        mov cx, [bp+F_CX]
        cmp byte [si+S_FLAGS], 2
        jne .file
        call con_read
        jmp .ret
.file:  call file_read
.ret:   mov [bp+F_AX], ax
        jmp ok

; AH=40h write: BX handle, CX count, DS:DX data.
fn_40:  mov bx, [bp+F_BX]
        call handle_sft
        jc fail
        mov fs, [bp+F_DS]
        mov dx, [bp+F_DX]
        mov cx, [bp+F_CX]
        cmp byte [si+S_FLAGS], 2
        jne .file
        mov [bp+F_AX], cx
        mov si, dx
        jcxz .cdone
.c:     mov al, [fs:si]
        call putc
        inc si
        loop .c
.cdone: jmp ok
.file:  test byte [si+S_MODE], 3
        jz .access
        call file_write
        mov [bp+F_AX], ax
        jmp ok
.access:
        mov ax, E_ACCESS
        jmp fail

; AH=41h delete DS:DX.
fn_41:  call caller_dsdx
        call resolve
        jc fail
        call lookup_name
        jc .nofile
        test byte [bx+DE_ATTR], A_DIR | A_VOLUME | A_RDONLY
        jnz .access
        mov byte [bx], 0xE5
        call mark_dirty
        mov ax, [bx+DE_CLUSTER]
        call free_chain
        call flush_sector
        call fat_flush
        jmp ok
.nofile:
        mov ax, E_NOFILE
        jmp fail
.access:
        mov ax, E_ACCESS
        jmp fail

; AH=42h seek: BX handle, AL origin, CX:DX offset -> DX:AX.
fn_42:  mov bx, [bp+F_BX]
        call handle_sft
        jc fail
        mov ax, [bp+F_CX]
        shl eax, 16
        mov ax, [bp+F_DX]
        mov cl, [bp+F_AL]
        cmp cl, 1
        jb .set
        je .cur
        add eax, [si+S_SIZE]
        jmp .set
.cur:   add eax, [si+S_POS]
.set:   cmp byte [si+S_FLAGS], 2
        jne .file
        xor eax, eax
.file:  mov [si+S_POS], eax
        mov [bp+F_AX], ax
        shr eax, 16
        mov [bp+F_DX], ax
        jmp ok

; AH=43h: AL=0 get attributes -> CX, AL=1 set from CX.
fn_43:  call caller_dsdx
        call resolve
        jc fail
        call lookup_name
        jc .nofile
        cmp byte [bp+F_AL], 0
        jne .set
        movzx ax, byte [bx+DE_ATTR]
        mov [bp+F_CX], ax
        jmp ok
.set:   mov al, [bx+DE_ATTR]
        and al, A_DIR | A_VOLUME
        mov cl, [bp+F_CL]
        and cl, ~(A_DIR | A_VOLUME)
        or al, cl
        mov [bx+DE_ATTR], al
        call mark_dirty
        call flush_sector
        jmp ok
.nofile:
        mov ax, E_NOFILE
        jmp fail

; AH=44h IOCTL (device information only).
fn_44:  mov al, [bp+F_AL]
        cmp al, 0
        je .info
        cmp al, 1
        je .ok
        cmp al, 7
        jbe .status
        cmp al, 8
        je .removable
        cmp al, 9
        je .remote
.ok:    jmp ok
.status:
        mov byte [bp+F_AL], 0xFF        ; ready
        jmp ok
.info:  mov bx, [bp+F_BX]
        call handle_sft
        jc fail
        cmp byte [si+S_FLAGS], 2
        je .con
        movzx ax, byte [si+S_DRIVE]
        mov [bp+F_DX], ax
        jmp ok
.con:   mov word [bp+F_DX], 0x80D3      ; char device: console in/out
        jmp ok
.removable:
        mov bl, [bp+F_BL]
        mov ax, 0                       ; A:, B: removable
        cmp bl, 3
        jb .rm
        inc ax
.rm:    mov [bp+F_AX], ax
        jmp ok
.remote:
        mov word [bp+F_DX], 0
        jmp ok

; AH=45h dup handle BX -> AX.
fn_45:  mov bx, [bp+F_BX]
        call handle_sft
        jc fail
        push es
        mov es, [cur_psp]
        mov dl, [es:0x18+bx]
        xor bx, bx
.h:     cmp byte [es:0x18+bx], 0xFF
        je .got
        inc bx
        cmp bx, 20
        jb .h
        pop es
        mov ax, E_TOOMANY
        jmp fail
.got:   mov [es:0x18+bx], dl
        pop es
        inc byte [si+S_REFS]
        mov [bp+F_AX], bx
        jmp ok

; AH=46h force dup: make handle CX refer to handle BX's file.
fn_46:  mov bx, [bp+F_BX]
        call handle_sft
        jc fail
        push si
        mov bx, [bp+F_CX]
        call handle_sft
        jc .noclose
        call close_handle
.noclose:
        pop si
        mov bx, [bp+F_BX]
        push es
        mov es, [cur_psp]
        mov dl, [es:0x18+bx]
        mov bx, [bp+F_CX]
        cmp bx, 20
        jae .bad
        mov [es:0x18+bx], dl
        pop es
        inc byte [si+S_REFS]
        jmp ok
.bad:   pop es
        mov ax, E_HANDLE
        jmp fail

; AH=68h commit file.
fn_68:  mov bx, [bp+F_BX]
        call handle_sft
        jc fail
        cmp byte [si+S_FLAGS], 1
        jne .ok
        call update_dirent
        call flush_sector
        call fat_flush
.ok:    jmp ok

; AH=57h: AL=0 get file time/date (CX/DX), AL=1 set.
fn_57:  mov bx, [bp+F_BX]
        call handle_sft
        jc fail
        cmp byte [si+S_FLAGS], 1
        jne .zero
        mov al, [si+S_DRIVE]
        call select_drive
        mov eax, [si+S_DIRLBA]
        call get_sector
        add bx, [si+S_DIROFF]
        cmp byte [bp+F_AL], 0
        jne .set
        mov ax, [bx+DE_TIME]
        mov [bp+F_CX], ax
        mov ax, [bx+DE_DATE]
        mov [bp+F_DX], ax
        jmp ok
.set:   mov ax, [bp+F_CX]
        mov [bx+DE_TIME], ax
        mov ax, [bp+F_DX]
        mov [bx+DE_DATE], ax
        call mark_dirty
        jmp ok
.zero:  mov word [bp+F_CX], 0
        mov word [bp+F_DX], 0
        jmp ok

; AH=39h mkdir DS:DX.
fn_39:  call caller_dsdx
        call resolve
        jc fail
        cmp byte [res_name], 0
        je .access
        call lookup_name
        jnc .access
        mov ax, [res_dir]
        mov [mk_parent], ax
        mov eax, [comp83]               ; keep the new directory's name
        mov [mk_name], eax
        mov eax, [comp83+4]
        mov [mk_name+4], eax
        mov eax, [comp83+8]
        mov [mk_name+8], eax
        ; New cluster with "." and "..".
        xor dx, dx
        call alloc_cluster
        jc .access
        mov [new_dir], ax
        call cluster_lba
        call get_sector
        mov dword [comp83], '.   '
        mov dword [comp83+4], '    '
        mov dword [comp83+8], '   '
        mov al, A_DIR
        mov dx, [new_dir]
        call init_entry                 ; "."
        add bx, 32
        mov byte [comp83+1], '.'
        mov al, A_DIR
        mov dx, [mk_parent]
        call init_entry                 ; ".."
        call flush_sector
        mov eax, [mk_name]
        mov [comp83], eax
        mov eax, [mk_name+4]
        mov [comp83+4], eax
        mov eax, [mk_name+8]
        mov [comp83+8], eax
        mov ax, [mk_parent]
        call free_slot
        jc .fail_free
        mov al, A_DIR
        mov dx, [new_dir]
        call init_entry
        call flush_sector
        call fat_flush
        jmp ok
.fail_free:
        push ax
        mov ax, [new_dir]
        call free_chain
        call fat_flush
        pop ax
        jmp fail
.access:
        mov ax, E_ACCESS
        jmp fail

; AH=3Ah rmdir DS:DX.
fn_3a:  call caller_dsdx
        call resolve
        jc fail
        call lookup_name
        jc .nopath
        test byte [bx+DE_ATTR], A_DIR
        jz .nopath
        mov eax, [sc_lba]
        mov [rm_lba], eax
        mov [rm_off], bx
        mov ax, [bx+DE_CLUSTER]
        mov [rm_cluster], ax
        ; Must be empty apart from "." and "..".
        mov cx, 2
.chk:   mov ax, [rm_cluster]
        call dir_entry
        jc .empty
        mov al, [bx]
        test al, al
        jz .empty
        cmp al, 0xE5
        je .nx
        jmp .access
.nx:    inc cx
        jmp .chk
.empty: mov eax, [rm_lba]
        call get_sector
        mov bx, [rm_off]
        mov byte [bx], 0xE5
        call mark_dirty
        mov ax, [rm_cluster]
        call free_chain
        call flush_sector
        call fat_flush
        jmp ok
.nopath:
        mov ax, E_NOPATH
        jmp fail
.access:
        mov ax, E_ACCESS
        jmp fail

; AH=3Bh chdir DS:DX.
fn_3b:  call caller_dsdx
        call resolve
        jc fail
        mov ax, [res_dir]
        cmp byte [res_name], 0
        je .set
        call lookup_name
        jc .nopath
        test byte [bx+DE_ATTR], A_DIR
        jz .nopath
        mov ax, [bx+DE_CLUSTER]
.set:   mov bx, [dp]
        mov [bx+D_CWD], ax
        ; Update the textual path.
        call caller_dsdx
        call chdir_text
        jmp ok
.nopath:
        mov ax, E_NOPATH
        jmp fail

; Apply the path at FS:SI to the current drive's D_PATH text.
chdir_text:
        mov di, [dp]
        add di, D_PATH
        cmp byte [fs:si+1], ':'
        jne .nd
        add si, 2
.nd:    mov al, [fs:si]
        cmp al, '\'
        je .root
        cmp al, '/'
        jne .comp
.root:  mov byte [di], 0
        inc si
.comp:  mov al, [fs:si]
        test al, al
        jz .done
        cmp al, '\'
        je .skip
        cmp al, '/'
        je .skip
        cmp al, '.'
        jne .name
        cmp byte [fs:si+1], '.'
        je .up
        inc si                          ; "."
        jmp .comp
.up:    add si, 2
        ; Remove the last component.
        push si
        mov si, di
        xor bx, bx                      ; position of the last '\'
        xor cx, cx
.f:     mov al, [si]
        test al, al
        jz .cut
        inc cx
        cmp al, '\'
        jne .fn
        mov bx, cx
.fn:    inc si
        jmp .f
.cut:   test bx, bx
        jz .clear
        dec bx
        mov byte [di+bx], 0
        jmp .upd
.clear: mov byte [di], 0
.upd:   pop si
        jmp .comp
.skip:  inc si
        jmp .comp
.name:  ; Append "\NAME" (or "NAME" at the root).
        push si
        mov si, di
.e:     cmp byte [si], 0
        je .at
        inc si
        jmp .e
.at:    cmp si, di
        je .first
        mov byte [si], '\'
        inc si
.first: pop bx                          ; source pointer
.cp:    mov al, [fs:bx]
        test al, al
        jz .end
        cmp al, '\'
        je .end
        cmp al, '/'
        je .end
        cmp al, 'a'
        jb .u
        cmp al, 'z'
        ja .u
        sub al, 32
.u:     mov [si], al
        inc si
        inc bx
        mov ax, si
        sub ax, di
        cmp ax, 62
        jb .cp
.end:   mov byte [si], 0
        mov si, bx
        jmp .comp
.done:  ret

; AH=47h get current directory of drive DL into DS:SI.
fn_47:  mov al, [bp+F_DL]
        dec al
        jns .have
        mov al, [cur_drive]
.have:  call select_drive
        jc fail
        mov si, [dp]
        add si, D_PATH
        mov es, [bp+F_DS]
        mov di, [bp+F_SI]
.c:     lodsb
        stosb
        test al, al
        jnz .c
        mov word [bp+F_AX], 0x0100
        jmp ok

; AH=56h rename DS:DX -> ES:DI (same directory only).
fn_56:  call caller_dsdx
        call resolve
        jc fail
        call lookup_name
        jc .nofile
        mov eax, [sc_lba]
        mov [rm_lba], eax
        mov [rm_off], bx
        mov ax, [res_dir]
        mov [rm_cluster], ax
        mov al, [dp_index]
        mov [rn_drive], al
        mov fs, [bp+F_ES]
        mov si, [bp+F_DI]
        call resolve
        jc fail
        mov al, [dp_index]
        cmp al, [rn_drive]
        jne .notsame
        mov ax, [res_dir]
        cmp ax, [rm_cluster]
        jne .notsame
        call lookup_name
        jnc .exists
        mov eax, [rm_lba]
        call get_sector
        mov di, [rm_off]
        mov si, comp83
        push es
        push ds
        pop es
        mov cx, 11
        rep movsb
        pop es
        call mark_dirty
        call flush_sector
        jmp ok
.nofile:
        mov ax, E_NOFILE
        jmp fail
.notsame:
        mov ax, E_NOTSAME
        jmp fail
.exists:
        mov ax, E_ACCESS
        jmp fail

; AH=4Eh find first: DS:DX pattern, CX attributes. Results go to the DTA.
fn_4e:  call caller_dsdx
        call resolve
        jc .nofile
        cmp byte [res_name], 0
        jne .pat
        ; "C:\DIR\" means everything in it.
        mov dword [comp83], '????'
        mov dword [comp83+4], '????'
        mov word [comp83+8], '??'
        mov byte [comp83+10], '?'
.pat:   les di, [dta]
        mov al, [dp_index]
        mov [es:di], al
        inc di
        mov si, comp83
        mov cx, 11
        rep movsb
        mov al, [bp+F_CL]
        mov [es:di], al                 ; DTA+12: attributes
        mov ax, [res_dir]
        mov [es:di+1], ax               ; DTA+13: directory
        mov word [es:di+3], 0           ; DTA+15: next index
        jmp find_continue
.nofile:
        mov ax, E_NOFILE
        jmp fail

fn_4f:  jmp find_continue

; Search using the state in the DTA and fill in the result.
find_continue:
        les di, [dta]
        mov al, [es:di]
        call select_drive
        jc .none
        ; Copy the pattern locally.
        xor bx, bx
.cp:    mov al, [es:di+bx+1]
        mov [find_pat+bx], al
        inc bx
        cmp bx, 11
        jb .cp
        mov dl, [es:di+12]
        mov ax, [es:di+13]
        mov cx, [es:di+15]
        mov si, find_pat
        push es
        push di
        call find_entry
        pop di
        pop es
        jc .none
        inc cx
        mov [es:di+15], cx
        ; Fill the result: attr, time, date, size, name.
        mov al, [bx+DE_ATTR]
        mov [es:di+21], al
        mov ax, [bx+DE_TIME]
        mov [es:di+22], ax
        mov ax, [bx+DE_DATE]
        mov [es:di+24], ax
        mov eax, [bx+DE_SIZE]
        mov [es:di+26], eax
        add di, 30
        mov si, bx
        call format_name
        jmp ok
.none:  mov ax, E_NOMORE
        jmp fail

; Format the 11-byte name at DS:SI as "NAME.EXT\0" at ES:DI.
format_name:
        push cx
        mov cx, 8
.b:     mov al, [si]
        cmp al, ' '
        je .bend
        stosb
        inc si
        loop .b
.bend:  add si, cx
        cmp byte [si], ' '
        je .noext
        mov al, '.'
        stosb
        mov cx, 3
.e:     mov al, [si]
        cmp al, ' '
        je .noext
        stosb
        inc si
        loop .e
.noext: xor al, al
        stosb
        pop cx
        ret

; ---------------------------------------------------------------- data
comp83      times 12 db 0
res_dir     dw 0
res_name    db 0
res_save    dd 0
fe_dir      dw 0
fe_pat      dw 0
fe_mask     db 0
oe_mode     db 0
oe_lba      dd 0
oe_off      dw 0
oe_keep     dw 0
grow        db 0
rw_total    dw 0
rw_src      dw 0
rw_lba      dd 0
rw_n        dw 0
mk_parent   dw 0
mk_name     times 12 db 0
create_mode db 0
create_attr db 0
ext_action  db 0
new_dir     dw 0
rm_lba      dd 0
rm_off      dw 0
rm_cluster  dw 0
rn_drive    db 0
find_pat    times 11 db 0
        align 2
sft         times NSFT * SFT_SIZE db 0
