//! Drives other than C:: a second floppy in B:, a second hard disk in D:,
//! disks inserted, swapped and ejected while TinyDOS runs, and writes to a
//! large FAT16 disk (several sectors per cluster).

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::fat::{format, Fs, Geometry};
use rusty486_core::machine::{Drive, Machine};
use rusty486_firmware::{tinydos_floppy, BIOS};

struct Pc {
    m: Machine,
}

impl Pc {
    /// Boot TinyDOS from A: with the given extra drives attached.
    fn boot(extra: Vec<(Drive, Vec<u8>)>) -> Pc {
        let mut m = Machine::new(BIOS.to_vec());
        m.insert_disk(Drive::A, DiskImage::new(tinydos_floppy("BOOT", &[]).unwrap()));
        for (d, data) in extra {
            m.insert_disk(d, DiskImage::new(data));
        }
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
        self.run_ms(600);
    }

    /// Clear the screen, run a command, and return what it printed.
    fn run(&mut self, cmd: &str) -> String {
        self.type_line("CLS");
        self.type_line(cmd);
        self.screen()
    }

    fn image(&self, d: Drive) -> Vec<u8> {
        self.m.disk(d).expect("drive empty").data.clone()
    }
}

/// A blank FAT data disk of `bytes` holding `files` (paths may have folders).
fn data_disk(bytes: usize, label: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut d = vec![0u8; bytes];
    let g = Geometry::for_size(bytes, 1);
    format(&mut d, &[], &g, label);
    let mut fs = Fs::open(&mut d).unwrap();
    for (p, data) in files {
        fs.write_creating_dirs(p, data).unwrap();
    }
    d
}

const FLOPPY: usize = 1_474_560;
const MB: usize = 1024 * 1024;

fn read(img: &[u8], path: &str) -> Vec<u8> {
    let mut img = img.to_vec();
    let fs = Fs::open(&mut img).unwrap();
    fs.read(path).unwrap_or_else(|e| panic!("{path}: {e:?}"))
}

/// Both FAT copies must match and start with the media byte.
fn assert_fats_consistent(img: &[u8]) {
    let reserved = u16::from_le_bytes([img[14], img[15]]) as usize;
    let fat_sectors = u16::from_le_bytes([img[22], img[23]]) as usize;
    let fat1 = &img[reserved * 512..(reserved + fat_sectors) * 512];
    let fat2 = &img[(reserved + fat_sectors) * 512..(reserved + 2 * fat_sectors) * 512];
    assert_eq!(fat1[0], img[21], "FAT 1 lost its media byte");
    for s in 0..fat_sectors {
        assert!(fat1[s * 512..(s + 1) * 512] == fat2[s * 512..(s + 1) * 512], "FAT copies differ in sector {s}");
    }
}

fn pattern(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 + i / 251) as u8).collect()
}

#[test]
fn second_floppy_and_second_hard_disk_at_boot() {
    let b = data_disk(FLOPPY, "FLOPPYB", &[("HELLO.TXT", b"hello from drive B")]);
    let d = data_disk(32 * MB, "DISKD", &[("GAME/README.TXT", b"hello from drive D")]);
    let mut pc = Pc::boot(vec![(Drive::B, b), (Drive::D, d)]);

    assert!(pc.run("DIR B:").contains("HELLO"), "{}", pc.screen());
    assert!(pc.run("TYPE B:\\HELLO.TXT").contains("hello from drive B"));
    pc.type_line("D:");
    pc.type_line("CD GAME");
    assert!(pc.run("TYPE README.TXT").contains("hello from drive D"), "{}", pc.screen());
    assert!(pc.screen().contains("D:\\GAME>"));

    // Copy across drives and check the result from the host side.
    assert!(pc.run("COPY B:\\HELLO.TXT D:\\GAME\\COPY.TXT").contains("1 file(s) copied"), "{}", pc.screen());
    pc.type_line("DEL B:\\HELLO.TXT");
    let d = pc.image(Drive::D);
    assert_eq!(read(&d, "GAME/COPY.TXT"), b"hello from drive B");
    assert!(Fs::open(&mut pc.image(Drive::B)).unwrap().read("HELLO.TXT").is_err(), "deleted on B:");
    assert_fats_consistent(&d);
}

#[test]
fn hard_disk_inserted_and_ejected_while_running() {
    let mut pc = Pc::boot(vec![]);
    assert!(pc.run("DIR D:").contains("Invalid drive"), "{}", pc.screen());

    pc.m.insert_disk(Drive::D, DiskImage::new(data_disk(64 * MB, "HOTPLUG", &[("NEW.TXT", b"plugged in")])));
    assert!(pc.run("TYPE D:\\NEW.TXT").contains("plugged in"), "{}", pc.screen());
    pc.type_line("MD D:\\SAVES");
    assert!(pc.run("DIR D:").contains("SAVES"), "{}", pc.screen());

    pc.m.eject_disk(Drive::D);
    assert!(pc.run("DIR D:").contains("Invalid drive"), "{}", pc.screen());
}

#[test]
fn floppy_swapped_in_b() {
    let one = data_disk(FLOPPY, "ONE", &[("DISK.TXT", b"first floppy")]);
    let two = data_disk(FLOPPY, "TWO", &[("DISK.TXT", b"second floppy")]);
    let mut pc = Pc::boot(vec![(Drive::B, one)]);
    assert!(pc.run("TYPE B:\\DISK.TXT").contains("first floppy"));
    pc.m.insert_disk(Drive::B, DiskImage::new(two));
    let out = pc.run("TYPE B:\\DISK.TXT");
    assert!(out.contains("second floppy") && !out.contains("first floppy"), "{out}");
}

#[test]
fn large_files_on_a_big_fat16_disk() {
    // 128 MB: 4 KB clusters, so every new cluster spans several sectors.
    // (TinyDOS once zeroed FAT sector 0 while clearing such clusters.)
    let big = pattern(300_000);
    let b = data_disk(FLOPPY, "SRC", &[("BIG.DAT", &big)]);
    let d = data_disk(128 * MB, "BIGDISK", &[("KEEP.TXT", b"must survive")]);
    let mut pc = Pc::boot(vec![(Drive::B, b), (Drive::D, d)]);

    pc.type_line("MD D:\\DATA");
    let out = pc.run("COPY B:\\BIG.DAT D:\\DATA\\BIG.DAT");
    assert!(out.contains("1 file(s) copied"), "{out}");
    pc.run_ms(2000);
    let out = pc.run("COPY D:\\DATA\\BIG.DAT D:\\DATA\\BIG2.DAT");
    assert!(out.contains("1 file(s) copied"), "{out}");
    pc.run_ms(2000);

    let d = pc.image(Drive::D);
    assert_fats_consistent(&d);
    assert!(read(&d, "DATA/BIG.DAT") == big, "first copy differs");
    assert!(read(&d, "DATA/BIG2.DAT") == big, "second copy differs");
    assert_eq!(read(&d, "KEEP.TXT"), b"must survive");
    // TinyDOS still reads the disk after all that.
    assert!(pc.run("TYPE D:\\KEEP.TXT").contains("must survive"), "{}", pc.screen());
}
