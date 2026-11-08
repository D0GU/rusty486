//! The rusty486 Disk Controller (RDC): a simple, invented bus-mastering disk
//! controller. The BIOS wraps it in the standard INT 13h interface.
//!
//! Commands execute instantly and copy sectors directly to/from RAM.
//!
//! | Port        | R/W | Meaning                                                   |
//! |-------------|-----|-----------------------------------------------------------|
//! | 0x300       | W   | Command: 1 = read, 2 = write                              |
//! | 0x300       | R   | Status: bit 0 = error, bit 7 = selected drive present     |
//! | 0x301       | R/W | Drive: 0x00/0x01 = floppy A/B, 0x80/0x81 = hard disk C/D  |
//! | 0x302-0x305 | R/W | LBA (32-bit, little-endian)                               |
//! | 0x306-0x307 | R/W | Sector count (16-bit)                                     |
//! | 0x308-0x30B | R/W | Physical memory address (32-bit)                          |
//! | 0x30C       | R   | Error code (INT 13h style: 0 ok, 1 bad cmd, 3 write-protected, 4 not found, 0x80 no drive) |
//! | 0x310-0x313 | R   | Total sectors of the selected drive                       |
//! | 0x314-0x315 | R   | Cylinders                                                 |
//! | 0x316       | R   | Heads                                                     |
//! | 0x317       | R   | Sectors per track                                         |
//! | 0x318       | R   | Media generation: changes whenever the host swaps or edits a disk |

pub const SECTOR: usize = 512;
pub const BASE_PORT: u16 = 0x300;

/// A disk image held in memory. The frontend loads and saves the bytes.
#[derive(Clone, Debug)]
pub struct DiskImage {
    pub data: Vec<u8>,
    pub cylinders: u16,
    pub heads: u8,
    pub sectors: u8,
    pub read_only: bool,
    /// Set when the guest writes to the disk.
    pub dirty: bool,
}

impl DiskImage {
    /// Wrap raw image bytes, picking a geometry from the size: standard floppy
    /// sizes get their usual geometry, anything else is a 16-head, 63-sector hard disk.
    pub fn new(mut data: Vec<u8>) -> Self {
        let (c, h, s) = match data.len() {
            163_840 => (40, 1, 8),
            184_320 => (40, 1, 9),
            327_680 => (40, 2, 8),
            368_640 => (40, 2, 9),
            737_280 => (80, 2, 9),
            1_228_800 => (80, 2, 15),
            1_474_560 => (80, 2, 18),
            2_949_120 => (80, 2, 36),
            n => {
                let sectors = n.div_ceil(SECTOR);
                let cyl = (sectors / (16 * 63)).clamp(1, 1024);
                (cyl, 16, 63)
            }
        };
        let total = (c * h * s) as usize * SECTOR;
        if data.len() < total {
            data.resize(total, 0);
        }
        DiskImage {
            data,
            cylinders: c as u16,
            heads: h as u8,
            sectors: s as u8,
            read_only: false,
            dirty: false,
        }
    }

    /// A blank, unformatted 1.44 MB floppy.
    pub fn blank_floppy() -> Self {
        Self::new(vec![0; 1_474_560])
    }

    pub fn total_sectors(&self) -> u32 {
        (self.data.len() / SECTOR) as u32
    }
}

#[derive(Default, Debug)]
pub struct DiskController {
    /// A:, B:, C:, D:
    pub drives: [Option<DiskImage>; 4],
    drive: u8,
    lba: u32,
    count: u16,
    addr: u32,
    error: u8,
    /// Bumped when disks are inserted, ejected or modified by the host, so
    /// the guest OS knows to drop its caches.
    pub generation: u8,
}

impl DiskController {
    fn slot(unit: u8) -> Option<usize> {
        match unit {
            0x00 => Some(0),
            0x01 => Some(1),
            0x80 => Some(2),
            0x81 => Some(3),
            _ => None,
        }
    }

    fn selected(&self) -> Option<&DiskImage> {
        Self::slot(self.drive).and_then(|i| self.drives[i].as_ref())
    }

    /// Note a host-side change to the disks.
    pub fn touch(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn read(&self, port: u16) -> u8 {
        let geo = self.selected();
        let byte = |v: u32, i: u16| (v >> (8 * i)) as u8;
        match port - BASE_PORT {
            0x00 => (self.error != 0) as u8 | if geo.is_some() { 0x80 } else { 0 },
            0x01 => self.drive,
            p @ 0x02..=0x05 => byte(self.lba, p - 2),
            p @ 0x06..=0x07 => byte(self.count as u32, p - 6),
            p @ 0x08..=0x0B => byte(self.addr, p - 8),
            0x0C => self.error,
            p @ 0x10..=0x13 => byte(geo.map_or(0, |d| d.total_sectors()), p - 0x10),
            p @ 0x14..=0x15 => byte(geo.map_or(0, |d| d.cylinders as u32), p - 0x14),
            0x16 => geo.map_or(0, |d| d.heads),
            0x17 => geo.map_or(0, |d| d.sectors),
            0x18 => self.generation,
            _ => 0xFF,
        }
    }

    /// Write a register. Writing the command register performs the transfer
    /// immediately against `ram` (the controller is a bus master).
    pub fn write(&mut self, port: u16, v: u8, ram: &mut [u8]) {
        let set = |reg: &mut u32, i: u16| {
            *reg = (*reg & !(0xFF << (8 * i))) | (v as u32) << (8 * i);
        };
        match port - BASE_PORT {
            0x00 => self.command(v, ram),
            0x01 => self.drive = v,
            p @ 0x02..=0x05 => set(&mut self.lba, p - 2),
            p @ 0x06..=0x07 => {
                let mut c = self.count as u32;
                set(&mut c, p - 6);
                self.count = c as u16;
            }
            p @ 0x08..=0x0B => set(&mut self.addr, p - 8),
            _ => {}
        }
    }

    fn command(&mut self, cmd: u8, ram: &mut [u8]) {
        self.error = 0;
        let Some(disk) = Self::slot(self.drive).and_then(|i| self.drives[i].as_mut()) else {
            self.error = 0x80;
            return;
        };
        let start = self.lba as usize * SECTOR;
        let len = self.count as usize * SECTOR;
        if start + len > disk.data.len() {
            self.error = 0x04;
            return;
        }
        let addr = self.addr as usize;
        if addr + len > ram.len() {
            self.error = 0x09; // DMA boundary / bad address
            return;
        }
        match cmd {
            1 => ram[addr..addr + len].copy_from_slice(&disk.data[start..start + len]),
            2 if disk.read_only => self.error = 0x03,
            2 => {
                disk.data[start..start + len].copy_from_slice(&ram[addr..addr + len]);
                disk.dirty = true;
            }
            _ => self.error = 0x01,
        }
    }
}
