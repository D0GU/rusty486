//! Packing flat 32-bit images into TinyDOS executables with the PM stub.

use alloc::vec;
use alloc::vec::Vec;

/// The DOS extender stub (`dos/pmstub.asm`).
pub const PM_STUB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/pmstub.bin"));

/// A flat 32-bit program for the PM stub.
#[derive(Clone, Debug)]
pub struct PmImage<'a> {
    /// Bytes of the image, linked to run at `load` (above 1 MB).
    pub image: &'a [u8],
    pub load: u32,
    /// Zeroed bytes directly after the image.
    pub bss: u32,
    /// `extern "C" fn(systab: *const u32) -> u8`.
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
    w(&mut out, 0x0A, extra); // minalloc: the transfer buffer
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
