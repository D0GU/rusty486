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

#[test]
fn full_document_continues_in_a_numbered_file() {
    // A 1.99 MB text file on a hard disk (it doesn't fit a floppy).
    let exe = rusty486_lang::compile(&source()).unwrap();
    let line = "All work and no play makes Jack a dull boy. More of the same, page after page.\r\n";
    let mut big = String::new();
    // Imported, each CR LF becomes one paragraph end: stop just short of
    // the 2,000,000-character limit.
    let mut lines = 0;
    while big.len() + line.len() - (lines + 1) < 1_999_900 {
        big.push_str(line);
        lines += 1;
    }
    let mut hdd = vec![0u8; 16 * 1024 * 1024];
    rusty486_firmware::format_tinydos(&mut hdd, "BIG").unwrap();
    {
        let mut fs = Fs::open(&mut hdd).unwrap();
        fs.write("WRITE.EXE", &exe).unwrap();
        fs.write("BOOK.TXT", big.as_bytes()).unwrap();
    }
    let floppy = tinydos_floppy("T", &[]).unwrap();
    let mut m = Machine::new(BIOS.to_vec());
    m.insert_disk(Drive::A, DiskImage::new(floppy));
    m.insert_disk(Drive::C, DiskImage::new(hdd));
    let mut pc = Pc { m };
    pc.run(1500);
    pc.typ("C:\n");
    pc.typ("WRITE BOOK.TXT\n");
    for _ in 0..60 {
        pc.run(500);
        if pc.screen().contains("BOOK.WP") {
            break;
        }
    }
    // Imported text saves as BOOK.WP, never over BOOK.TXT.
    pc.expect("BOOK.WP");
    pc.expect("Imported");
    // The status bar stays readable with big numbers.
    pc.expect("words");
    // Typing past the limit offers the next file.
    pc.key(0x4F, 0, CTRL);
    for _ in 0..8 {
        pc.typ("filling up the last of the space ");
        if pc.screen().contains("Document Full") {
            break;
        }
    }
    pc.expect("This document has reached 2,000,000 characters.");
    pc.expect("Save it and continue in BOOK2.WP?");
    pc.typ("y");
    pc.run(5000);
    pc.expect("BOOK2.WP");
    pc.expect("Continued from BOOK.WP");
    pc.typ("Chapter Thirty");
    pc.key(0x2D, 0, ALT);
    pc.run(500);
    pc.expect("Save changes?");
    pc.typ("y");
    pc.run(5000);
    pc.expect("Thanks for using R++ Write!");
    let mut d = pc.m.disk(Drive::C).unwrap().data.clone();
    let fs = Fs::open(&mut d).unwrap();
    assert_eq!(fs.read("BOOK.TXT").unwrap().len(), big.len(), "the imported text file was changed");
    assert!(fs.read("BOOK.WP").unwrap().len() > 3_000_000);
    let second = fs.read("BOOK2.WP").unwrap();
    assert!(String::from_utf8_lossy(&second).contains("Continued from BOOK.WP"));
    assert!(String::from_utf8_lossy(&second).contains("Chapter Thirty"));
}

#[test]
fn settings_lists_and_formatting() {
    let exe = rusty486_lang::compile(&source()).unwrap();
    let disk = tinydos_floppy("T", &[("WRITE.EXE", &exe)]).unwrap();
    let mut m = Machine::new(BIOS.to_vec());
    m.insert_disk(Drive::A, DiskImage::new(disk));
    let mut pc = Pc { m };
    pc.run(1500);
    pc.typ("WRITE\n");
    pc.run(2000);
    // The welcome page shows a bullet list (the bullet glyph is not ASCII).
    let s = pc.screen();
    let row = s.lines().find(|l| l.contains("Bullet lists (Alt+7)")).expect("bullet line").to_string();
    assert!(row.starts_with("            Bullet"), "{row:?}");
    // First-line indent is on: the first paragraph line is indented.
    assert!(s.lines().any(|l| l.starts_with("            Welcome ")), "{s}");

    // F8 Settings: turn the indent off and paragraph spacing on.
    pc.key(0x42, 0, 0);
    pc.expect("Space between paragraphs");
    pc.key(0x4D, 0, 0); // Right: indent off
    pc.key(0x50, 0, 0);
    pc.key(0x50, 0, 0);
    pc.key(0x4D, 0, 0); // Right: spacing on
    pc.key(0x01, 27, 0);
    pc.run(300);
    let s = pc.screen();
    assert!(s.lines().any(|l| l.starts_with("        Welcome ")), "{s}");
    // A blank spacer line now follows the chapter heading.
    let lines: Vec<&str> = s.lines().collect();
    let h = lines.iter().position(|l| l.contains("Chapter One")).unwrap();
    assert_eq!(lines[h + 1].trim(), "", "{s}");

    // New document: type, select a word, highlight it, change case, undo.
    pc.ctrl('N', 0x31);
    pc.typ("the quick fox");
    for _ in 0..3 {
        pc.key(0x4B, 0, SHIFT);
    }
    pc.key(0x25, 11, CTRL); // Ctrl+K highlight
    pc.key(0x18, 0, ALT); // Format menu
    pc.typ("o"); // lowercase (a separate undo step)
    pc.key(0x18, 0, ALT);
    pc.key(0x48, 0, 0); // Up from the top wraps to the last item: Title Case
    pc.key(0x48, 0, 0); // lowercase
    pc.key(0x48, 0, 0); // UPPERCASE
    pc.key(0x1C, 13, 0);
    pc.expect("the quick FOX");
    pc.ctrl('Z', 0x2C);
    pc.expect("the quick fox");
    pc.ctrl('Z', 0x2C); // undo the lowercase step
    pc.ctrl('Z', 0x2C); // undo the highlight
    pc.ctrl('Z', 0x2C); // undo the typing
    let s = pc.screen();
    assert!(!s.contains("the quick"), "{s}");
}
