//! Boots TinyDOS headless and drives COMMAND.COM through the keyboard.

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::fat::Fs;
use rusty486_core::machine::{Drive, Machine};
use rusty486_firmware::{pm_exe, tinydos_floppy, PmImage, BIOS};

struct Pc {
    m: Machine,
}

impl Pc {
    fn boot(files: &[(&str, &[u8])]) -> Pc {
        let disk = tinydos_floppy("TEST", files).expect("build disk");
        let mut m = Machine::new(BIOS.to_vec());
        m.insert_disk(Drive::A, DiskImage::new(disk));
        let mut pc = Pc { m };
        pc.run_ms(1500);
        pc
    }

    fn run_ms(&mut self, ms: u64) {
        let hz = self.m.clock_hz;
        self.m.run_cycles(hz / 1000 * ms);
        assert!(!self.m.cpu.shutdown, "CPU shutdown\n{}", self.screen());
    }

    fn screen(&self) -> String {
        self.m.screen_text()
    }

    fn type_line(&mut self, s: &str) {
        for c in s.chars().chain(std::iter::once('\r')) {
            let (code, ascii) = if c == '\r' { (0x1C, 13) } else { (0x02, c as u8) };
            self.m.key_event(code, false, true, ascii, 0, false);
            self.m.key_event(code, false, false, 0, 0, false);
            self.run_ms(3);
        }
        self.run_ms(400);
    }

    fn expect(&self, needle: &str) {
        let s = self.screen();
        assert!(s.contains(needle), "expected {needle:?} on screen:\n{s}");
    }

    fn disk(&mut self) -> Vec<u8> {
        self.m.disk(Drive::A).unwrap().data.clone()
    }
}

fn asm(src: &str) -> Vec<u8> {
    match rusty486_asm::assemble(src) {
        Ok(o) => o.bytes,
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn boots_to_prompt() {
    let pc = Pc::boot(&[]);
    pc.expect("131072 KB RAM OK");
    pc.expect("TinyDOS 1.0");
    pc.expect("A:\\>");
}

#[test]
fn internal_commands() {
    let mut pc = Pc::boot(&[("README.TXT", b"Hello from a text file!\r\n")]);
    pc.type_line("VER");
    pc.expect("TinyDOS version 1.0");
    pc.type_line("TYPE README.TXT");
    pc.expect("Hello from a text file!");
    pc.type_line("DIR");
    pc.expect("COMMAND  COM");
    pc.expect("README   TXT");
    pc.expect("file(s)");
    pc.type_line("MD GAMES");
    pc.type_line("CD GAMES");
    pc.expect("A:\\GAMES>");
    pc.type_line("COPY \\README.TXT COPY.TXT");
    pc.expect("1 file(s) copied");
    pc.type_line("CD ..");
    pc.type_line("CLS");
    pc.type_line("DIR GAMES");
    pc.expect("COPY     TXT");
    // The copy really is on the disk.
    let mut disk = pc.disk();
    let fs = Fs::open(&mut disk).unwrap();
    assert_eq!(fs.read("GAMES/COPY.TXT").unwrap(), b"Hello from a text file!\r\n");
}

#[test]
fn runs_programs_and_files() {
    // A .COM program that writes a file, reads it back and prints it.
    let prog = asm(r#"
        org 0x100
        mov ah, 0x3C
        xor cx, cx
        mov dx, fname
        int 0x21
        jc fail
        mov bx, ax
        mov ah, 0x40
        mov cx, msg_len
        mov dx, msg
        int 0x21
        mov ah, 0x3E
        int 0x21
        mov ax, 0x3D00
        mov dx, fname
        int 0x21
        jc fail
        mov bx, ax
        mov ah, 0x3F
        mov cx, 100
        mov dx, buf
        int 0x21
        mov cx, ax
        mov ah, 0x40
        mov bx, 1
        mov dx, buf
        int 0x21
        mov ax, 0x4C07
        int 0x21
fail:   mov dx, failmsg
        mov ah, 9
        int 0x21
        mov ax, 0x4C01
        int 0x21
fname   db 'OUT.TXT', 0
msg     db 'Round trip through the file system works!', 13, 10
msg_len equ $ - msg
failmsg db 'FAILED$'
buf     times 100 db 0
    "#);
    let mut pc = Pc::boot(&[("FILEIO.COM", &prog)]);
    pc.type_line("FILEIO");
    pc.expect("Round trip through the file system works!");
    pc.type_line("DIR OUT.TXT");
    pc.expect("OUT      TXT");
    let mut disk = pc.disk();
    let fs = Fs::open(&mut disk).unwrap();
    assert!(fs.read("OUT.TXT").unwrap().starts_with(b"Round trip"));
}

#[test]
fn runs_exe_with_relocations() {
    // Hand-built MZ executable: code segment + data segment needing a fixup.
    let exe = asm(r#"
hdr:    db 'MZ'
        dw (file_end - hdr) % 512       ; bytes in last page
        dw (file_end - hdr + 511) / 512 ; pages
        dw 1                            ; relocations
        dw 2                            ; header paragraphs (32 bytes)
        dw 0x10, 0xFFFF                 ; min/max alloc
        dw 0, 0x200                     ; SS:SP (SS relative)
        dw 0                            ; checksum
        dw 0, 0                         ; IP, CS
        dw 0x1C, 0                      ; relocation table offset, overlay
        dw fix - code, 0                ; the relocation: offset in CS
        times 32 - ($ - hdr) db 0
code:   mov ax, 0x0001                  ; patched to data's segment
fix     equ $ - 2
        mov ds, ax
        mov dx, 0
        mov ah, 9
        int 0x21
        mov ax, 0x4C00
        int 0x21
        times 16 - (($ - code) % 16) db 0
data:   db 'Hello from an EXE!', 13, 10, '$'
file_end:
    "#);
    let mut pc = Pc::boot(&[("HELLO.EXE", &exe)]);
    pc.type_line("HELLO");
    pc.expect("Hello from an EXE!");
    pc.type_line("MEM");
    pc.expect("bytes free conventional memory");
}

#[test]
fn sees_files_added_by_the_host() {
    let mut pc = Pc::boot(&[]);
    pc.type_line("MD WORK");
    pc.type_line("CD WORK");
    {
        let disk = pc.m.disk_mut(Drive::A).unwrap();
        let mut fs = Fs::open(&mut disk.data).unwrap();
        fs.write("WORK/NEW.TXT", b"added while DOS was running").unwrap();
    }
    pc.type_line("TYPE NEW.TXT");
    pc.expect("added while DOS was running");
    pc.expect("A:\\WORK>");
}

#[test]
fn graphics_mode_13h_and_vbe() {
    let gfx = asm(r#"
        org 0x100
        mov ax, 0x0013
        int 0x10
        push word 0xA000
        pop es
        xor di, di
        xor dx, dx              ; y
.y:     xor cx, cx              ; x
.x:     mov ax, cx
        add ax, dx
        stosb
        inc cx
        cmp cx, 320
        jb .x
        inc dx
        cmp dx, 200
        jb .y
        mov ax, 0x0E41          ; text in graphics mode
        mov bx, 0x000F
        int 0x10
        ; VBE controller info
        push ds
        pop es
        mov di, vbeinfo
        mov ax, 0x4F00
        int 0x10
        cmp ax, 0x004F
        jne .fail
        cmp dword [vbeinfo], 'VESA'
        jne .fail
        mov dx, okmsg
        mov ah, 9
        int 0x21
        mov ax, 0x4C00
        int 0x21
.fail:  mov ax, 0x4C01
        int 0x21
okmsg   db 'VESA ok$'
vbeinfo times 512 db 0
    "#);
    let vbe = asm(r#"
        org 0x100
        mov ax, 0x4F02
        mov bx, 0x0101          ; 640x480x256, banked
        int 0x10
        mov ax, 0x4F05
        xor bx, bx
        mov dx, 4               ; bank 4 (64 KB units)
        int 0x10
        push word 0xA000
        pop es
        mov byte [es:0x0010], 0x55
        mov ax, 0x4C00
        int 0x21
    "#);
    let mut pc = Pc::boot(&[("GFX.COM", &gfx), ("VBE.COM", &vbe)]);
    pc.type_line("GFX");
    let v = &pc.m.bus.video;
    assert!(v.graphics(), "mode 13h not active");
    assert_eq!((v.xres, v.yres, v.bpp), (320, 200, 8));
    assert_eq!(v.vram[10 * 320 + 20], 30);
    // The 'A' glyph was drawn in colour 15 somewhere in the first cell.
    assert!((0..8).any(|y| (0..8).any(|x| v.vram[y * 320 + x] == 15)), "no text pixels");
    let log = String::from_utf8_lossy(&pc.m.bus.debug_out).into_owned();
    assert!(log.contains("VESA ok"), "debug log: {log}");
    pc.type_line("VBE");
    let v = &pc.m.bus.video;
    assert_eq!((v.xres, v.yres, v.bpp), (640, 480, 8));
    assert_eq!(v.vram[4 * 65536 + 0x10], 0x55);
}

#[test]
fn sound_blaster_dma_and_adlib() {
    let prog = asm(r#"
        org 0x100
        ; --- AdLib detection through the OPL2 timers
        mov ax, 0x0460
        call opl
        mov ax, 0x0480
        call opl
        mov dx, 0x388
        in al, dx
        mov [s1], al
        mov ax, 0x02FF
        call opl
        mov ax, 0x0421
        call opl
        mov cx, 0
        mov dx, 200
        mov ah, 0x86
        int 0x15                ; wait ~200 us (rounded up to a tick)
        mov dx, 0x388
        in al, dx
        mov [s2], al
        mov ax, 0x0460
        call opl
        mov ax, 0x0480
        call opl
        mov al, [s1]
        and al, 0xE0
        jnz fail
        mov al, [s2]
        and al, 0xE0
        cmp al, 0xC0
        jne fail
        mov dx, adlib_ok
        mov ah, 9
        int 0x21
        ; --- Sound Blaster: reset, version, DMA playback with IRQ 5
        mov ax, 0x250D          ; IRQ 5 = INT 0Dh
        mov dx, irq
        int 0x21
        in al, 0x21
        and al, ~0x20
        out 0x21, al
        mov dx, 0x226
        mov al, 1
        out dx, al
        xor al, al
        out dx, al
        call dsp_read
        cmp al, 0xAA
        jne fail
        mov al, 0xE1
        call dsp_write
        call dsp_read
        cmp al, 2
        jne fail
        call dsp_read
        mov al, 0xD1            ; speaker on
        call dsp_write
        mov al, 0x40            ; 11025 Hz
        call dsp_write
        mov al, 165
        call dsp_write
        ; DMA channel 1: single transfer, memory -> device
        mov ax, ds
        mov dx, ax
        shl ax, 4
        shr dx, 12
        add ax, buf
        adc dx, 0
        mov bx, ax
        mov al, 5
        out 0x0A, al
        xor al, al
        out 0x0C, al
        mov al, 0x49
        out 0x0B, al
        mov al, bl
        out 0x02, al
        mov al, bh
        out 0x02, al
        mov al, dl
        out 0x83, al
        mov al, (1000 - 1) & 0xFF
        out 0x03, al
        mov al, (1000 - 1) >> 8
        out 0x03, al
        mov al, 1
        out 0x0A, al
        mov al, 0x14            ; 8-bit single-cycle output
        call dsp_write
        mov al, (1000 - 1) & 0xFF
        call dsp_write
        mov al, (1000 - 1) >> 8
        call dsp_write
        mov cx, 40              ; wait up to ~2 s for the IRQ
.wait:  cmp byte [flag], 1
        je .done
        hlt
        loop .wait
        jmp fail
.done:  mov dx, sb_ok
        mov ah, 9
        int 0x21
        mov ax, 0x4C00
        int 0x21
fail:   mov dx, failed
        mov ah, 9
        int 0x21
        mov ax, 0x4C01
        int 0x21

opl:    push dx
        mov dx, 0x388
        xchg al, ah
        out dx, al
        inc dx
        xchg al, ah
        out dx, al
        pop dx
        ret
dsp_write:
        push dx
        push ax
        mov dx, 0x22C
.w:     in al, dx
        test al, 0x80
        jnz .w
        pop ax
        out dx, al
        pop dx
        ret
dsp_read:
        push dx
        mov dx, 0x22E
.r:     in al, dx
        test al, 0x80
        jz .r
        mov dx, 0x22A
        in al, dx
        pop dx
        ret
irq:    push ax
        push dx
        mov dx, 0x22E
        in al, dx               ; acknowledge the DSP interrupt
        mov byte [cs:flag], 1
        mov al, 0x20
        out 0x20, al
        pop dx
        pop ax
        iret

s1      db 0
s2      db 0
flag    db 0
adlib_ok db 'AdLib detected ', '$'
sb_ok   db 'SB IRQ ok', '$'
failed  db 'SOUND FAILED', '$'
buf:
%rep 10
        times 50 db 0x20
        times 50 db 0xE0
%endrep
    "#);
    let mut pc = Pc::boot(&[("SOUND.COM", &prog)]);
    let _ = pc.m.take_audio();
    pc.type_line("SOUND");
    pc.run_ms(500);
    pc.expect("AdLib detected");
    pc.expect("SB IRQ ok");
    // The square wave played through the DAC is in the audio stream.
    let audio = pc.m.take_audio();
    let loud = audio.iter().filter(|s| s.unsigned_abs() > 8000).count();
    assert!(loud > 100, "no Sound Blaster output in {} samples", audio.len());
}

#[test]
fn autoexec_runs() {
    let pc = Pc::boot(&[("AUTOEXEC.BAT", b"@ECHO OFF\r\nECHO Autoexec says hi\r\n")]);
    pc.expect("Autoexec says hi");
}

/// Pack a `bits 32` / `org 0x800000` program for the PM stub.
fn pm_program(src: &str) -> Vec<u8> {
    let image = asm(src);
    pm_exe(&PmImage {
        image: &image,
        load: 0x80_0000,
        bss: 0x1000,
        entry: 0x80_0000,
        stack_top: 0x90_0000,
        heap_end: 0,
    })
}

#[test]
fn pm_stub_runs_32bit_programs() {
    let hello = pm_program(r#"
        bits 32
        org 0x800000
main:   mov ebx, [esp+4]                ; system table
        mov [systab], ebx
        mov esi, msg1
        call print
        ; Heap is above the image and BSS.
        mov eax, [ebx+24]
        cmp eax, end_of_image
        jb .bad
        mov dword [eax], 0x12345678
        mov esi, msg2
        call print
        ; The heap reaches the top of the 128 MB of RAM.
        mov ebx, [systab]
        cmp dword [ebx+28], 0x8000000
        jne .bad
        ; Read the command tail (" ARGS" -> 5 bytes).
        mov eax, [ebx+20]
        cmp byte [eax], 5
        jne .bad
        mov esi, msg3
        call print
        mov eax, 3
        ret
.bad:   mov esi, msgbad
        call print
        mov eax, 1
        ret

; Print the '$'-terminated string at ESI through DOS AH=09h.
print:  mov ebx, [systab]
        mov edi, [ebx+8]                ; transfer buffer
.c:     lodsb
        stosb
        cmp al, '$'
        jne .c
        mov dword [regs], 0x0900        ; eax
        mov dword [regs+12], 0          ; edx
        mov eax, [ebx+12]
        mov [regs+28], ax               ; ds
        push regs
        push dword 0x21
        call [ebx]
        add esp, 8
        ret

msg1    db 'Hello from 32-bit protected mode!', 13, 10, '$'
msg2    db 'Heap OK', 13, 10, '$'
msg3    db 'Args OK', 13, 10, '$'
msgbad  db 'BAD', 13, 10, '$'
systab  dd 0
regs    times 9 dd 0
end_of_image:
    "#);
    let crash = pm_program(r#"
        bits 32
        org 0x800000
        mov eax, 0
        div eax
    "#);
    let mut pc = Pc::boot(&[("HELLO32.EXE", &hello), ("CRASH.EXE", &crash)]);
    pc.type_line("HELLO32 ARGS");
    pc.expect("Hello from 32-bit protected mode!");
    pc.expect("Heap OK");
    pc.expect("Args OK");
    pc.type_line("CRASH");
    pc.expect("CPU exception 00h at EIP 00800005");
    // Both programs returned to DOS and the prompt still works.
    pc.type_line("VER");
    pc.expect("TinyDOS version 1.0");
}

#[test]
fn command_line_history_and_completion() {
    let mut pc = Pc::boot(&[
        ("README.TXT", b"Hello from a text file!\r\n"),
        ("RECIPE.TXT", b"Two eggs.\r\n"),
    ]);
    let key = |pc: &mut Pc, code: u8, ext: bool, ascii: u8| {
        pc.m.key_event(code, ext, true, ascii, 0, false);
        pc.m.key_event(code, ext, false, 0, 0, false);
        pc.run_ms(20);
    };
    let typ = |pc: &mut Pc, s: &str| {
        for c in s.chars() {
            pc.m.key_event(0x02, false, true, c as u8, 0, false);
            pc.m.key_event(0x02, false, false, 0, 0, false);
            pc.run_ms(5);
        }
    };
    const UP: u8 = 0x48;
    const LEFT: u8 = 0x4B;
    const TAB: u8 = 0x0F;
    const ENTER: u8 = 0x1C;

    // History: Up recalls VER.
    pc.type_line("VER");
    key(&mut pc, UP, true, 0);
    key(&mut pc, ENTER, false, 13);
    pc.run_ms(300);
    assert_eq!(pc.screen().matches("TinyDOS version 1.0").count(), 2, "{}", pc.screen());

    // Editing in the middle of the line: "VR", Left, "E" -> "VER".
    typ(&mut pc, "CLS");
    key(&mut pc, ENTER, false, 13);
    pc.run_ms(200);
    typ(&mut pc, "VR");
    key(&mut pc, LEFT, true, 0);
    typ(&mut pc, "E");
    key(&mut pc, ENTER, false, 13);
    pc.run_ms(300);
    pc.expect("TinyDOS version 1.0");

    // Esc clears the line.
    typ(&mut pc, "XYZZY");
    key(&mut pc, 0x01, false, 27);
    typ(&mut pc, "VER");
    key(&mut pc, ENTER, false, 13);
    pc.run_ms(300);
    assert!(!pc.screen().contains("XYZZY"), "{}", pc.screen());

    // Completing a file name.
    pc.type_line("CLS");
    typ(&mut pc, "TYPE REA");
    key(&mut pc, TAB, false, 9);
    key(&mut pc, ENTER, false, 13);
    pc.run_ms(300);
    pc.expect("TYPE README.TXT");
    pc.expect("Hello from a text file!");

    // Two matches: Tab does nothing, a second Tab lists them.
    typ(&mut pc, "TYPE RE");
    key(&mut pc, TAB, false, 9);
    key(&mut pc, TAB, false, 9);
    pc.run_ms(100);
    pc.expect("README.TXT");
    pc.expect("RECIPE.TXT");
    key(&mut pc, 0x01, false, 27);
    key(&mut pc, ENTER, false, 13);

    // Directories get a backslash; commands complete too.
    pc.type_line("MD GAMES");
    typ(&mut pc, "CD GA");
    key(&mut pc, TAB, false, 9);
    key(&mut pc, ENTER, false, 13);
    pc.run_ms(300);
    pc.expect(r"A:\GAMES>");
    pc.type_line("CD \\");
    typ(&mut pc, "HISTO");
    key(&mut pc, TAB, false, 9);
    key(&mut pc, ENTER, false, 13);
    pc.run_ms(300);
    pc.expect("   1  VER");
    pc.expect("CD GAMES\\");

    // DIR shows ".." without a stray extension.
    pc.type_line("CD GAMES");
    pc.type_line("DIR");
    let s = pc.screen();
    assert!(s.lines().any(|l| l.starts_with("..   ") && l.contains("<DIR>") && !l.starts_with("..       .")), "{s}");

    // DIR /W lists names in columns, directories in brackets.
    pc.type_line("CD \\");
    pc.type_line("CLS");
    pc.type_line("DIR /W");
    let s = pc.screen();
    assert!(s.contains("[GAMES]") && s.contains("README.TXT") && s.contains("RECIPE.TXT") && s.contains("file(s)"), "{s}");
    assert!(s.lines().any(|l| l.contains("README.TXT") && l.contains("RECIPE.TXT")), "{s}");

    // Typing a text file's name doesn't run it (it used to hang).
    pc.type_line("CLS");
    pc.type_line("readme.txt");
    pc.expect("Not a program: README.TXT. To view it, type: TYPE README.TXT");
    pc.type_line("VER");
    pc.expect("TinyDOS version 1.0");
    pc.type_line("NOSUCH.TXT");
    pc.expect("Bad command or file name");
}
