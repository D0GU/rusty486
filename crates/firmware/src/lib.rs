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

/// The DOS extender stub that starts 32-bit protected-mode programs (see
/// `dos/pmstub.asm`).
pub const PM_STUB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pmstub.bin"));

/// A flat 32-bit program for the PM stub.
#[derive(Clone, Debug)]
pub struct PmImage<'a> {
    /// Bytes of the image, linked to run at `load` (must be above 1 MB).
    pub image: &'a [u8],
    pub load: u32,
    /// Zeroed bytes directly after the image.
    pub bss: u32,
    /// `extern "C" fn(systab: *const SysTab) -> u8`.
    pub entry: u32,
    pub stack_top: u32,
    /// End of the heap handed to the program (0 = the stub's default, 15 MB).
    pub heap_end: u32,
}

/// Wrap a flat 32-bit image into a TinyDOS .EXE: MZ header, PM stub, payload
/// header and image.
pub fn pm_exe(p: &PmImage) -> Vec<u8> {
    const HDR: usize = 32;
    const XFER: usize = 32768;
    let load_len = HDR + PM_STUB.len();
    let mut out = vec![0u8; HDR];
    let w = |o: &mut Vec<u8>, at: usize, v: u16| o[at..at + 2].copy_from_slice(&v.to_le_bytes());
    out[0] = b'M';
    out[1] = b'Z';
    w(&mut out, 2, (load_len % 512) as u16);
    w(&mut out, 4, load_len.div_ceil(512) as u16);
    w(&mut out, 6, 0); // relocations
    w(&mut out, 8, (HDR / 16) as u16);
    let extra = (XFER + 16).div_ceil(16) as u16;
    w(&mut out, 0x0A, extra); // minalloc: transfer buffer
    w(&mut out, 0x0C, extra); // maxalloc
    w(&mut out, 0x0E, 0); // SS
    w(&mut out, 0x10, ((PM_STUB.len() + XFER) & !15) as u16); // SP
    w(&mut out, 0x14, 0); // IP
    w(&mut out, 0x16, 0); // CS
    w(&mut out, 0x18, 0x1C);
    out.extend_from_slice(PM_STUB);
    out.extend_from_slice(b"R486");
    for v in [p.load, p.image.len() as u32, p.bss, p.entry, p.stack_top, p.heap_end, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(p.image);
    out
}
