//! Ant Colony (dos/rustland/examples/ANTS.RS): 640x480x256 with the mouse.

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::machine::{Drive, Machine};
use rusty486_firmware::{tinydos_floppy, BIOS};

fn source() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dos/rustland/examples/ANTS.RS")).unwrap()
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
    fn key(&mut self, scan: u8, ascii: u8) {
        self.m.key_event(scan, false, true, ascii, 0, false);
        self.m.key_event(scan, false, false, 0, 0, false);
        self.run(100);
    }
    fn typ(&mut self, s: &str) {
        for c in s.chars() {
            let (sc, a) = if c == '\n' { (0x1C, 13) } else { (0x10, c as u8) };
            self.key(sc, a);
        }
    }
    /// Save the screen as a PPM when ANTS_SHOT names a directory.
    fn shot(&self, name: &str) {
        if let Ok(dir) = std::env::var("ANTS_SHOT") {
            let mut px = Vec::new();
            let (w, h) = self.m.bus.video.render_graphics(&mut px);
            let mut out = format!("P6 {w} {h} 255\n").into_bytes();
            for p in px {
                out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
            }
            std::fs::write(format!("{dir}/{name}.ppm"), out).unwrap();
        }
    }
    /// A pixel of the page being shown.
    fn pixel(&self, x: usize, y: usize) -> u8 {
        let v = &self.m.bus.video;
        v.vram[(v.y_offset as usize + y) * 640 + x]
    }
}

#[test]
fn committed_build_is_current() {
    let built = rusty486_lang::compile(&source()).unwrap();
    let committed = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dos/ants/ANTS.EXE")).unwrap();
    assert!(built == committed, "dos/ants/ANTS.EXE is stale: run scripts/build-rustland.ps1");
}

#[test]
fn colony_runs_with_the_mouse() {
    let exe = rusty486_lang::compile(&source()).unwrap();
    let disk = tinydos_floppy("T", &[("ANTS.EXE", &exe)]).unwrap();
    let mut m = Machine::new(BIOS.to_vec());
    m.insert_disk(Drive::A, DiskImage::new(disk));
    let mut pc = Pc { m };
    pc.run(1500);
    pc.typ("ANTS\n");
    pc.run(3000);
    let v = &pc.m.bus.video;
    assert_eq!((v.xres, v.yres, v.bpp), (640, 480, 8));
    pc.shot("start");
    // The world has grass and the panel is drawn.
    assert!((16..48).contains(&pc.pixel(10, 300)) || pc.pixel(10, 300) >= 48, "no world");
    assert_eq!(pc.pixel(300, 428), 91, "panel border");

    // Ants walk out of the nest.
    let start = pc.m.cpu.cycles;
    pc.run(10_000);
    let _ = start;
    pc.shot("later");
    let ants = (0..428).flat_map(|y| (0..640).map(move |x| (x, y))).filter(|&(x, y)| {
        let p = pc.pixel(x, y);
        (p == 80 || p == 81) && ((x as i32 - 320).pow(2) + (y as i32 - 220).pow(2) > 60 * 60)
    });
    assert!(ants.count() > 5, "no ants away from the nest");

    // Click the Rock tool, then drag on the world: rock appears there.
    pc.m.mouse_move(BUTTON_ROCK_X, (436u32 * 400 / 480) as u16);
    pc.run(100);
    pc.m.mouse_buttons(1);
    pc.run(100);
    pc.m.mouse_buttons(0);
    pc.run(100);
    pc.m.mouse_move(100, (50u32 * 400 / 480) as u16);
    pc.run(100);
    pc.m.mouse_buttons(1);
    pc.run(300);
    pc.m.mouse_buttons(0);
    pc.run(200);
    pc.shot("rock");
    let p = pc.pixel(96, 48);
    assert!((48..52).contains(&p), "no rock under the mouse: {p}");

    // Esc quits back to DOS with a summary.
    pc.key(0x01, 27);
    pc.run(500);
    assert!(pc.m.screen_text().contains("ants were born"), "{}", pc.m.screen_text());
}

/// The "2 Rock" button is the second in the first row.
const BUTTON_ROCK_X: u16 = 6 + (6 * 8 + 10) + 4 + 10;
