//! R++ Write (dos/rustland/examples/WRITE.RS): drives the word processor
//! under TinyDOS with menus, files and book tools.

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::fat::Fs;
use rusty486_core::machine::{Drive, Machine};
use rusty486_firmware::{tinydos_floppy, BIOS};

const CTRL: u8 = 4;
const ALT: u8 = 8;
const SHIFT: u8 = 2;

fn source() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dos/rustland/examples/WRITE.RS")).unwrap()
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
        self.run(150);
        if flags != 0 {
            self.m.key_event(0x1D, false, false, 0, 0, true);
            self.run(10);
        }
    }
    fn typ(&mut self, s: &str) {
        for c in s.chars() {
            let (sc, a) = if c == '\n' { (0x1C, 13) } else { (0x10, c as u8) };
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
}

#[test]
fn committed_build_is_current() {
    let built = rusty486_lang::compile(&source()).unwrap();
    let committed = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dos/write/WRITE.EXE")).unwrap();
    assert!(built == committed, "dos/write/WRITE.EXE is stale: run scripts/build-rustland.ps1");
}

#[test]
fn menus_files_and_book_tools() {
    let exe = rusty486_lang::compile(&source()).unwrap();
    let sample = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dos/write/SAMPLE.WP")).unwrap();
    let disk = tinydos_floppy("T", &[("WRITE.EXE", &exe), ("SAMPLE.WP", &sample)]).unwrap();
    let mut m = Machine::new(BIOS.to_vec());
    m.insert_disk(Drive::A, DiskImage::new(disk));
    let mut pc = Pc { m };
    pc.run(1500);
    pc.typ("WRITE\n");
    pc.run(1500);
    pc.expect("Chapter One");
    pc.expect("UNTITLED.WP");

    // Menus: F10 opens File; Right moves along the bar.
    pc.key(0x44, 0, 0);
    pc.expect("Export text...");
    pc.expect("Delete file...");
    for _ in 0..4 {
        pc.key(0x4D, 0, 0);
    }
    pc.expect("Chapter heading");
    pc.key(0x01, 27, 0);

    // A new document: a chapter heading and a paragraph.
    pc.ctrl('N', 0x31);
    pc.typ("The Beginning");
    pc.key(0x03, 0, ALT); // Alt+2: chapter
    pc.key(0x1C, 13, 0);
    pc.typ("It was a dark and stormy night.");
    pc.key(0x4B, 0, SHIFT);
    pc.ctrl('B', 0x30);
    pc.expect("The Beginning");
    pc.expect("9 words");

    // Save As, then New, then Open from the file list.
    pc.ctrl('S', 0x1F);
    pc.expect("Save document as");
    pc.typ("BOOK\n");
    pc.expect("BOOK.WP");
    pc.ctrl('N', 0x31);
    pc.ctrl('O', 0x18);
    pc.expect("Open a Document");
    pc.expect("BOOK.WP");
    pc.expect("SAMPLE.WP");
    pc.key(0x1C, 13, 0); // first file: BOOK.WP
    pc.expect("It was a dark and stormy night.");

    // Outline and statistics.
    pc.key(0x3F, 0, 0);
    pc.expect("The Beginning");
    pc.expect("words, p.1");
    pc.key(0x01, 27, 0);
    pc.key(0x43, 0, 0);
    pc.expect("Chapters:          1");
    pc.key(0x01, 27, 0);

    // Export as text.
    pc.key(0x21, 0, ALT);
    pc.typ("e");
    pc.expect("BOOK.TXT");
    pc.key(0x1C, 13, 0);

    // The sample book: outline lists both chapters and the section.
    pc.ctrl('O', 0x18);
    pc.key(0x50, 0, 0); // BOOK.TXT, BOOK.WP, SAMPLE.WP (sorted)
    pc.key(0x50, 0, 0);
    pc.key(0x1C, 13, 0);
    pc.expect("The Lighthouse Keeper");
    pc.key(0x3F, 0, 0);
    pc.expect("Chapter Two");
    pc.expect("The Logbook");
    pc.key(0x01, 27, 0);

    // Colors: pick Paper, then exit.
    pc.key(0x19, 0, ALT);
    pc.typ("c");
    pc.expect("Midnight");
    pc.key(0x50, 0, 0);
    pc.key(0x1C, 13, 0);
    pc.key(0x2D, 0, ALT);
    pc.run(500);
    pc.expect("Thanks for using R++ Write!");

    let mut d = pc.m.disk(Drive::A).unwrap().data.clone();
    let fs = Fs::open(&mut d).unwrap();
    let txt = String::from_utf8(fs.read("BOOK.TXT").unwrap()).unwrap();
    assert_eq!(txt, "# The Beginning\r\n\r\nIt was a dark and stormy night.\r\n\r\n");
    let wp = fs.read("BOOK.WP").unwrap();
    assert_eq!(&wp[..4], b"RWP2");
}
