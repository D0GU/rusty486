//! Boots TinyDOS headless and drives COMMAND.COM through the keyboard.

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::fat::Fs;
use rusty486_core::machine::{Drive, Machine};
use rusty486_firmware::{tinydos_floppy, BIOS};

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
fn autoexec_runs() {
    let pc = Pc::boot(&[("AUTOEXEC.BAT", b"@ECHO OFF\r\nECHO Autoexec says hi\r\n")]);
    pc.expect("Autoexec says hi");
}
