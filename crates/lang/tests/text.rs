//! Text (dos/rustland/examples/TEXT.RS): the Notepad-style plain text editor.

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::fat::Fs;
use rusty486_core::machine::{Drive, Machine};
use rusty486_firmware::{tinydos_floppy, BIOS};

const CTRL: u8 = 4;
const ALT: u8 = 8;
const SHIFT: u8 = 2;

fn source() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dos/rustland/examples/TEXT.RS")).unwrap()
}

struct Pc {
    m: Machine,
}

impl Pc {
    fn run(&mut self, ms: u64) {
        let hz = self.m.clock_hz;
        self.m.run_cycles(hz / 1000 * ms);
        assert!(!self.m.cpu.shutdown);
    }
    fn key(&mut self, scan: u8, ascii: u8, flags: u8) {
        self.m.key_event(scan, false, true, ascii, flags, false);
        self.m.key_event(scan, false, false, 0, flags, false);
        self.run(120);
        if flags != 0 {
            self.m.key_event(0x1D, false, false, 0, 0, true);
            self.run(10);
        }
    }
    fn typ(&mut self, s: &str) {
        for c in s.chars() {
            let (sc, a) = match c {
                '\n' => (0x1C, 13),
                '\t' => (0x0F, 9),
                _ => (0x10, c as u8),
            };
            self.key(sc, a, 0);
        }
    }
    fn ctrl(&mut self, c: char, scan: u8) {
        self.key(scan, c as u8 - b'A' + 1, CTRL);
    }
    fn screen(&self) -> String {
        self.m.screen_text()
    }
    fn expect(&self, what: &str) {
        let s = self.screen();
        assert!(s.contains(what), "expected {what:?}:\n{s}");
    }
    fn file(&self, name: &str) -> Vec<u8> {
        let mut d = self.m.disk(Drive::A).unwrap().data.clone();
        let fs = Fs::open(&mut d).unwrap();
        fs.read(name).unwrap()
    }
}

fn boot(files: &[(&str, &[u8])]) -> Pc {
    let exe = rusty486_lang::compile(&source()).unwrap();
    let mut all: Vec<(&str, &[u8])> = vec![("TEXT.EXE", &exe)];
    all.extend_from_slice(files);
    let disk = tinydos_floppy("T", &all).unwrap();
    let mut m = Machine::new(BIOS.to_vec());
    m.insert_disk(Drive::A, DiskImage::new(disk));
    let mut pc = Pc { m };
    pc.run(1500);
    pc
}

#[test]
fn committed_build_is_current() {
    let built = rusty486_lang::compile(&source()).unwrap();
    let committed = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dos/text/TEXT.EXE")).unwrap();
    assert!(built == committed, "dos/text/TEXT.EXE is stale: run scripts/build-rustland.ps1");
}

#[test]
fn edits_keep_line_ends_and_tabs() {
    // LF line ends, a tab, and no final newline: all kept on save.
    let mut pc = boot(&[("NOTES.TXT", b"alpha\tbeta\nsecond line\nlast")]);
    pc.typ("TEXT NOTES.TXT\n");
    pc.run(1500);
    pc.expect("NOTES.TXT");
    pc.expect("alpha   beta");
    pc.expect("Ln 1, Col 1");
    pc.expect("LF");
    // End of the file, type, save.
    pc.key(0x4F, 0, CTRL);
    pc.typ("!");
    pc.expect("NOTES.TXT *");
    pc.ctrl('S', 0x1F);
    pc.expect("Saved");
    assert_eq!(pc.file("NOTES.TXT"), b"alpha\tbeta\nsecond line\nlast!");

    // Go to line 2, select the word "second", cut it, paste at the start.
    pc.ctrl('G', 0x22);
    pc.expect("Go To Line");
    pc.typ("2\n");
    pc.expect("Ln 2, Col 1");
    for _ in 0..6 {
        pc.key(0x4D, 0, SHIFT);
    }
    pc.ctrl('X', 0x2D);
    pc.key(0x47, 0, CTRL);
    pc.ctrl('V', 0x2F);
    pc.expect("secondalpha");
    // Undo the paste and the cut.
    pc.ctrl('Z', 0x2C);
    pc.ctrl('Z', 0x2C);
    pc.expect("second line");
    pc.expect("alpha   beta");

    // Find is case-insensitive and selects the match.
    pc.ctrl('F', 0x21);
    pc.typ("LINE\n");
    pc.expect("Ln 2, Col 12");

    // Replace all.
    pc.ctrl('H', 0x23);
    pc.typ("a\n");
    pc.typ("4\n");
    pc.expect("4 replacement(s) made.");
    pc.key(0x1C, 13, 0);
    pc.expect("4lph4   bet4");

    // Exit: save the changes.
    pc.key(0x2D, 0, ALT);
    pc.expect("Save the changes?");
    pc.typ("y");
    pc.run(500);
    pc.expect("A:\\>");
    assert_eq!(pc.file("NOTES.TXT"), b"4lph4\tbet4\nsecond line\nl4st!");
}

#[test]
fn new_file_crlf_wrap_and_menus() {
    let mut pc = boot(&[]);
    pc.typ("TEXT\n");
    pc.run(1500);
    pc.expect("Untitled");
    pc.expect("CRLF");
    // A long line wraps with Word Wrap on (Ctrl+W).
    let long = "the quick brown fox jumps over the lazy dog ".repeat(3);
    pc.typ(&long);
    pc.typ("\nend");
    let s = pc.screen();
    assert!(s.lines().nth(2).unwrap().starts_with("end"), "{s}");
    pc.ctrl('W', 0x11);
    let s = pc.screen();
    assert!(s.lines().nth(1).unwrap().trim_end().ends_with("jumps over the"), "{s}");
    assert!(s.lines().nth(2).unwrap().starts_with("lazy dog the quick"), "{s}");
    pc.expect("Ln 2, Col 4");

    // The View menu shows the check mark; turn on line numbers.
    pc.key(0x2F, 0, ALT);
    pc.expect("Word Wrap");
    pc.expect("Line Numbers");
    pc.typ("l");
    let s = pc.screen();
    assert!(s.lines().nth(1).unwrap().starts_with("     1 the quick"), "{s}");

    // Save As adds .TXT; the file is written with CR LF.
    pc.ctrl('S', 0x1F);
    pc.expect("Save As");
    pc.typ("NEW\n");
    pc.expect("NEW.TXT");
    let want = format!("{long}\r\nend");
    // A new file ends with a line break.
    assert_eq!(String::from_utf8(pc.file("NEW.TXT")).unwrap(), format!("{want}\r\n"));

    // Open shows the file list.
    pc.ctrl('O', 0x18);
    pc.expect("NEW.TXT");
    pc.expect("TEXT.EXE");
    pc.key(0x01, 27, 0);
    pc.key(0x2D, 0, ALT);
    pc.run(500);
    pc.expect("A:\\>");
    // The view settings are saved.
    assert_eq!(pc.file("TEXT.CFG"), [b'T', 1, 1, 1, 8]);
}
