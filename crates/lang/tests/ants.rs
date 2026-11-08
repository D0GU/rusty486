//! Ant Colony (dos/rustland/examples/ANTS.RS): a side-view ant farm in
//! 640x480x256, scrolling over a 1920-pixel-wide world, with the mouse.

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
    /// A pixel of the screen as shown (page and scroll position).
    fn pixel(&self, x: usize, y: usize) -> u8 {
        let v = &self.m.bus.video;
        let base = v.y_offset as usize * 1920 + v.x_offset as usize;
        v.vram[base + y * 1920 + x]
    }
    fn count(&self, colours: std::ops::Range<u8>) -> usize {
        (0..400).flat_map(|y| (0..640).map(move |x| (x, y))).filter(|&(x, y)| colours.contains(&self.pixel(x, y))).count()
    }
    fn mouse(&mut self, x: u16, y: u16) {
        self.m.mouse_move(x, (y as u32 * 400 / 480) as u16);
        self.run(50);
    }
}

#[test]
fn committed_build_is_current() {
    let built = rusty486_lang::compile(&source()).unwrap();
    let committed = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dos/ants/ANTS.EXE")).unwrap();
    assert!(built == committed, "dos/ants/ANTS.EXE is stale: run scripts/build-rustland.ps1");
}

#[test]
fn ant_farm_digs_forages_and_scrolls() {
    let exe = rusty486_lang::compile(&source()).unwrap();
    let disk = tinydos_floppy("T", &[("ANTS.EXE", &exe)]).unwrap();
    let mut m = Machine::new(BIOS.to_vec());
    m.insert_disk(Drive::A, DiskImage::new(disk));
    let mut pc = Pc { m };
    pc.run(1500);
    pc.typ("ANTS\n");
    pc.run(6000);
    let v = &pc.m.bus.video;
    assert_eq!((v.xres, v.yres, v.bpp, v.virt_width), (640, 480, 8, 1920));
    // The view starts over the nest, in the middle of the world.
    assert_eq!(v.x_offset, 960 - 320);
    pc.shot("start");
    // Sky at the top, earth below, the panel border.
    assert!((16..24).contains(&pc.pixel(5, 5)) || (24..26).contains(&pc.pixel(5, 5)), "sky: {}", pc.pixel(5, 5));
    assert!((32..48).contains(&pc.pixel(5, 300)) || (56..60).contains(&pc.pixel(5, 300)), "earth: {}", pc.pixel(5, 300));
    assert_eq!(pc.pixel(300, 400), 91, "panel border");
    let tunnel0 = pc.count(52..56);

    // Ants come out, and the first gallery gets dug.
    pc.run(20_000);
    pc.shot("later");
    assert!(pc.count(80..82) > 20, "no ants on screen");
    assert!(pc.count(52..56) > tunnel0 + 50, "nothing was dug: {} -> {}", tunnel0, pc.count(52..56));

    // Dig tool (selected at the start): drag over earth to mark it.
    let marked0 = pc.count(48..52);
    pc.mouse(100, 300);
    pc.m.mouse_buttons(1);
    pc.run(100);
    pc.mouse(140, 300);
    pc.run(100);
    pc.m.mouse_buttons(0);
    pc.mouse(320, 200);
    pc.run(300);
    pc.shot("marked");
    assert!(pc.count(48..52) > marked0 + 100, "no earth marked for digging");

    // Scroll right with the arrow key.
    pc.key(0x4D, 0);
    pc.run(200);
    assert_eq!(pc.m.bus.video.x_offset, 960 - 320 + 64);

    pc.key(0x01, 27);
    pc.run(500);
    assert!(pc.m.screen_text().contains("cells were dug"), "{}", pc.m.screen_text());
}
