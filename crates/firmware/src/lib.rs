//! rusty486 firmware: the BIOS ROM, boot sectors and TinyDOS (all assembled
//! from source at build time), and a builder for bootable TinyDOS disks.

use rusty486_core::fat::{self, FatError, Fs, Geometry};

/// The 64 KB BIOS ROM image (right-aligned by the machine).
pub const BIOS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bios.bin"));
/// Milestone 1 demo boot sector.
pub const HELLO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hello.bin"));
/// Raw program loader boot sector (loads a flat program at 1000:0100).
pub const LOADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/loader.bin"));
/// TinyDOS boot sector.
pub const DOS_BOOT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/boot.bin"));
/// TinyDOS kernel (stored in the reserved sectors after the boot sector).
pub const DOS_KERNEL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/kernel.bin"));
/// TinyDOS command interpreter.
pub const COMMAND_COM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/command.bin"));

pub const FLOPPY_1440K: usize = 1_474_560;

/// Reserved sectors: the boot sector plus room for a kernel of up to 63.5 KB
/// (the kernel segment), so disks can take newer kernels later.
fn reserved_sectors() -> u16 {
    1 + DOS_KERNEL.len().div_ceil(fat::SECTOR).max(127) as u16
}

/// Format `disk` as a bootable TinyDOS volume (FAT12 for floppy sizes,
/// FAT16 otherwise) with COMMAND.COM in the root.
pub fn format_tinydos(disk: &mut [u8], label: &str) -> Result<(), FatError> {
    let g = Geometry::for_size(disk.len(), reserved_sectors());
    fat::format(disk, DOS_BOOT, &g, label);
    let k = DOS_KERNEL;
    disk[fat::SECTOR..fat::SECTOR + k.len()].copy_from_slice(k);
    let mut fs = Fs::open(disk)?;
    fs.write("COMMAND.COM", COMMAND_COM)
}

/// A bootable 1.44 MB TinyDOS floppy containing `files` (paths may include
/// directories, which are created).
pub fn tinydos_floppy(label: &str, files: &[(&str, &[u8])]) -> Result<Vec<u8>, FatError> {
    let mut disk = vec![0u8; FLOPPY_1440K];
    format_tinydos(&mut disk, label)?;
    let mut fs = Fs::open(&mut disk)?;
    for (path, data) in files {
        fs.write_creating_dirs(path, data)?;
    }
    Ok(disk)
}

/// Whether a disk image contains a TinyDOS kernel (boot sector check).
pub fn is_tinydos(disk: &[u8]) -> bool {
    disk.len() >= 512 && disk[3..11] == *b"TINYDOS " && disk[510] == 0x55
}
