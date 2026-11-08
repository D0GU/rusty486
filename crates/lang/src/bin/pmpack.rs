//! pmpack: turn a statically linked i386 ELF executable into a TinyDOS .EXE
//! that runs under the rusty486 PM stub.
//!
//! usage: pmpack <input.elf> <output.exe> [heap_end_hex]

use rusty486_lang::{pm_exe, PmImage};

fn u16le(b: &[u8], at: usize) -> u32 {
    u16::from_le_bytes([b[at], b[at + 1]]) as u32
}
fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: pmpack <input.elf> <output.exe> [heap_end_hex]");
        std::process::exit(2);
    }
    let elf = std::fs::read(&args[1]).unwrap_or_else(|e| panic!("{}: {e}", args[1]));
    let heap_end = args.get(3).map(|h| u32::from_str_radix(h.trim_start_matches("0x"), 16).expect("heap_end")).unwrap_or(0);
    assert!(&elf[..4] == b"\x7FELF" && elf[4] == 1, "not a 32-bit ELF file");
    let entry = u32le(&elf, 24);
    let phoff = u32le(&elf, 28) as usize;
    let phentsize = u16le(&elf, 42) as usize;
    let phnum = u16le(&elf, 44) as usize;
    // PT_LOAD segments: (vaddr, file bytes, memsz)
    let mut segs = Vec::new();
    for i in 0..phnum {
        let ph = phoff + i * phentsize;
        if u32le(&elf, ph) != 1 {
            continue;
        }
        let (off, vaddr, filesz, memsz) =
            (u32le(&elf, ph + 4) as usize, u32le(&elf, ph + 8), u32le(&elf, ph + 16) as usize, u32le(&elf, ph + 20));
        if memsz == 0 {
            continue;
        }
        segs.push((vaddr, &elf[off..off + filesz], memsz));
    }
    segs.sort_by_key(|s| s.0);
    let load = segs.first().expect("no loadable segments").0;
    let file_end = segs.iter().filter(|s| !s.1.is_empty()).map(|s| s.0 + s.1.len() as u32).max().unwrap();
    let mem_end = segs.iter().map(|s| s.0 + s.2).max().unwrap();
    let mut image = vec![0u8; (file_end - load) as usize];
    for (vaddr, bytes, _) in &segs {
        let at = (vaddr - load) as usize;
        image[at..at + bytes.len()].copy_from_slice(bytes);
    }
    let stack_top = mem_end.div_ceil(16) * 16;
    let exe = pm_exe(&PmImage { image: &image, load, bss: stack_top - file_end, entry, stack_top, heap_end });
    std::fs::write(&args[2], &exe).unwrap();
    println!(
        "{}: {} bytes (image {:#x}..{:#x}, bss to {:#x}, entry {:#x})",
        args[2],
        exe.len(),
        load,
        file_end,
        mem_end,
        entry
    );
}
