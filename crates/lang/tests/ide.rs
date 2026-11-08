//! Drives the Rustland R++ IDE (dos/rustland/RUSTLAND.EXE, built by
//! scripts/build-rustland.ps1) inside a headless rusty486.

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::fat::Fs;
use rusty486_core::machine::{Drive, Machine};
use rusty486_firmware::{tinydos_floppy, BIOS};

const CTRL: u8 = 0x04;
const ALT: u8 = 0x08;

struct Pc {
    m: Machine,
}

impl Pc {
    fn run_ms(&mut self, ms: u64) {
        let hz = self.m.clock_hz;
        self.m.run_cycles(hz / 1000 * ms);
        assert!(!self.m.cpu.shutdown, "CPU shutdown\n{}", self.m.screen_text());
    }
    fn key(&mut self, scan: u8, ascii: u8, flags: u8) {
        self.m.key_event(scan, false, true, ascii, flags, false);
        self.m.key_event(scan, false, false, 0, flags, false);
        self.run_ms(20);
        // Release modifiers.
        if flags != 0 {
            self.m.key_event(0x1D, false, false, 0, 0, true);
        }
        self.run_ms(30);
    }
    fn typ(&mut self, s: &str) {
        for c in s.chars() {
            let ascii = if c == '\n' { 13 } else { c as u8 };
            let scan = if c == '\n' { 0x1C } else { 0x02 };
            self.key(scan, ascii, 0);
        }
    }
    fn screen(&self) -> String {
        self.m.screen_text()
    }
    /// Run until `needle` appears (up to `max_ms`); returns the time taken.
    fn wait_for(&mut self, needle: &str, max_ms: u64) -> u64 {
        let mut t = 0;
        while !self.screen().contains(needle) {
            assert!(t < max_ms, "timed out waiting for {needle:?}:\n{}", self.screen());
            self.run_ms(100);
            t += 100;
        }
        t
    }
}

const HELLO: &str = "fn main() {\r\n    let mut total = 0;\r\n    for i in 1..=10 {\r\n        total += i;\r\n    }\r\n    println!(\"Sum is {}\", total);\r\n}\r\n";

#[test]
fn ide_edits_compiles_and_runs() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../dos/rustland/RUSTLAND.EXE");
    let Ok(ide) = std::fs::read(path) else {
        eprintln!("RUSTLAND.EXE not built; skipping");
        return;
    };
    let disk = tinydos_floppy("TEST", &[("RUSTLAND.EXE", &ide), ("HELLO.RS", HELLO.as_bytes())]).unwrap();
    let mut m = Machine::new(BIOS.to_vec());
    m.insert_disk(Drive::A, DiskImage::new(disk));
    let mut pc = Pc { m };
    pc.run_ms(1500);
    pc.typ("RUSTLAND HELLO.RS\n");
    let t = pc.wait_for("Ctrl-F9", 10_000);
    eprintln!("IDE up after {t} ms");
    let s = pc.screen();
    assert!(s.contains("File") && s.contains("HELLO.RS") && s.contains("println!"), "{s}");

    // Ctrl+F9: compile and run.
    pc.key(0x43, 0, CTRL);
    let t = pc.wait_for("Program exited with code 0", 60_000);
    eprintln!("compiled and ran in {t} ms");
    assert!(pc.screen().contains("Sum is 55"), "{}", pc.screen());
    pc.key(0x39, b' ', 0);
    pc.wait_for("Ctrl-F9", 5000);

    // Break the program and compile: the error lands in the message window.
    pc.key(0x47, 0, CTRL); // Ctrl+Home
    pc.typ("x");
    pc.key(0x43, 0, ALT); // Alt+F9
    pc.wait_for("Press any key", 60_000);
    pc.key(0x39, b' ', 0);
    pc.run_ms(200);
    let s = pc.screen();
    assert!(s.contains("Error HELLO.RS 1:"), "{s}");

    // Fix it, save with F2 and exit with Alt+X.
    pc.key(0x53, 0, 0); // Del removes the x
    pc.key(0x3C, 0, 0);
    pc.key(0x2D, 0, ALT);
    pc.wait_for("A:\\>", 5000);
    let disk = pc.m.disk(Drive::A).unwrap().data.clone();
    let mut disk = disk;
    let fs = Fs::open(&mut disk).unwrap();
    assert_eq!(fs.read("HELLO.RS").unwrap(), HELLO.as_bytes());
    assert!(fs.read("HELLO.EXE").is_ok());
}
