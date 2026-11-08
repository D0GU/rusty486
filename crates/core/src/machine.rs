//! The complete rusty486 machine: CPU + memory + devices.

use std::collections::BTreeSet;

use crate::bus::Bus;
use crate::cpu::{Cpu, CS};
use crate::devices::disk::{self, DiskController, DiskImage};
use crate::devices::video::{self, Video};

pub const RAM_SIZE: usize = 16 * 1024 * 1024;
pub const BIOS_SIZE: usize = 64 * 1024;
const BIOS_BASE: u32 = 0xF0000;
/// The BIOS is also visible at the top of the 4 GB address space.
const BIOS_HIGH: u32 = 0xFFFF_0000;
/// Bochs-style debug console: bytes written here appear in the host log.
pub const DEBUG_PORT: u16 = 0xE9;

/// Everything on the motherboard except the CPU.
pub struct PcBus {
    pub ram: Vec<u8>,
    pub bios: Vec<u8>,
    pub disk: DiskController,
    pub video: Video,
    /// Bytes written to the debug port since the frontend last drained it.
    pub debug_out: Vec<u8>,
    /// Fast A20 gate (port 92h bit 1). When off, address line 20 is forced low.
    pub a20: bool,
}

impl PcBus {
    pub fn new(bios: Vec<u8>) -> Self {
        assert!(bios.len() <= BIOS_SIZE, "BIOS image must be at most 64 KB");
        // Right-align the ROM so its reset vector lands at F000:FFF0.
        let mut rom = vec![0xFF; BIOS_SIZE];
        rom[BIOS_SIZE - bios.len()..].copy_from_slice(&bios);
        PcBus {
            ram: vec![0; RAM_SIZE],
            bios: rom,
            disk: DiskController::default(),
            video: Video::default(),
            debug_out: Vec::new(),
            a20: true,
        }
    }

    fn map(&self, addr: u32) -> u32 {
        if self.a20 {
            addr
        } else {
            addr & !(1 << 20)
        }
    }
}

impl Bus for PcBus {
    fn read8(&mut self, addr: u32) -> u8 {
        let addr = self.map(addr);
        if (BIOS_BASE..BIOS_BASE + BIOS_SIZE as u32).contains(&addr) {
            self.bios[(addr - BIOS_BASE) as usize]
        } else if addr >= BIOS_HIGH {
            self.bios[(addr - BIOS_HIGH) as usize]
        } else {
            self.ram.get(addr as usize).copied().unwrap_or(0xFF)
        }
    }

    fn write8(&mut self, addr: u32, value: u8) {
        let addr = self.map(addr);
        if (BIOS_BASE..BIOS_BASE + BIOS_SIZE as u32).contains(&addr) {
            return; // ROM
        }
        if let Some(b) = self.ram.get_mut(addr as usize) {
            *b = value;
        }
    }

    fn io_read8(&mut self, port: u16) -> u8 {
        match port {
            0x92 => (self.a20 as u8) << 1,
            p if Video::handles(p) => self.video.read(p),
            p if (disk::BASE_PORT..disk::BASE_PORT + 0x20).contains(&p) => self.disk.read(p),
            _ => 0xFF,
        }
    }

    fn io_write8(&mut self, port: u16, value: u8) {
        match port {
            DEBUG_PORT => self.debug_out.push(value),
            0x92 => self.a20 = value & 2 != 0,
            p if Video::handles(p) => self.video.write(p, value),
            p if (disk::BASE_PORT..disk::BASE_PORT + 0x20).contains(&p) => {
                self.disk.write(p, value, &mut self.ram)
            }
            _ => {}
        }
    }
}

/// Drive slots, matching BIOS drive numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drive {
    A = 0,
    B = 1,
    C = 2,
    D = 3,
}

pub struct Machine {
    pub cpu: Cpu,
    pub bus: PcBus,
    /// Emulated clock speed in Hz, used by frontends to pace execution.
    pub clock_hz: u64,
    /// Linear addresses where execution stops (checked before each instruction).
    pub breakpoints: BTreeSet<u32>,
}

/// Why `run_cycles` returned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunResult {
    /// The cycle budget was used up.
    Completed,
    /// The next instruction is at a breakpoint (linear address).
    Breakpoint(u32),
    /// The CPU triple-faulted and needs a reset.
    Shutdown,
}

impl Machine {
    pub fn new(bios: Vec<u8>) -> Self {
        Machine {
            cpu: Cpu::new(),
            bus: PcBus::new(bios),
            clock_hz: 33_000_000,
            breakpoints: BTreeSet::new(),
        }
    }

    /// Linear address of the next instruction (CS base + EIP).
    pub fn pc(&self) -> u32 {
        self.cpu.seg_base[CS].wrapping_add(self.cpu.eip)
    }

    /// Execute exactly one instruction (or interrupt delivery).
    pub fn step(&mut self) {
        self.cpu.step(&mut self.bus);
    }

    pub fn insert_disk(&mut self, drive: Drive, image: DiskImage) {
        self.bus.disk.drives[drive as usize] = Some(image);
    }

    pub fn eject_disk(&mut self, drive: Drive) -> Option<DiskImage> {
        self.bus.disk.drives[drive as usize].take()
    }

    pub fn disk(&self, drive: Drive) -> Option<&DiskImage> {
        self.bus.disk.drives[drive as usize].as_ref()
    }

    /// Hardware reset: CPU back to F000:FFF0. RAM and disks are kept.
    pub fn reset(&mut self) {
        self.cpu.reset();
        self.bus.a20 = true;
    }

    /// Run until at least `cycles` more clock cycles have elapsed, or until
    /// execution reaches a breakpoint. The instruction at the current PC
    /// always executes first, so resuming from a breakpoint makes progress.
    pub fn run_cycles(&mut self, cycles: u64) -> RunResult {
        let end = self.cpu.cycles + cycles;
        while self.cpu.cycles < end {
            if self.cpu.shutdown {
                self.cpu.cycles = end;
                return RunResult::Shutdown;
            }
            if self.cpu.halted && self.cpu.eflags & crate::cpu::flags::IF == 0 {
                // Halted with interrupts off: nothing can wake us. Skip ahead.
                self.cpu.cycles = end;
                break;
            }
            self.cpu.step(&mut self.bus);
            if !self.breakpoints.is_empty() && !self.cpu.halted {
                let pc = self.pc();
                if self.breakpoints.contains(&pc) {
                    return RunResult::Breakpoint(pc);
                }
            }
        }
        RunResult::Completed
    }

    /// The 80x25 text buffer: (character, attribute) byte pairs.
    pub fn text_buffer(&self) -> &[u8] {
        let n = video::TEXT_COLS * video::TEXT_ROWS * 2;
        &self.bus.ram[video::TEXT_BASE..video::TEXT_BASE + n]
    }

    /// The text screen as plain text, one line per row (for tests and tools).
    pub fn screen_text(&self) -> String {
        self.text_buffer()
            .chunks(video::TEXT_COLS * 2)
            .map(|row| {
                let line: String = row
                    .chunks(2)
                    .map(|c| match c[0] {
                        0x20..=0x7E => c[0] as char,
                        _ => ' ',
                    })
                    .collect();
                line.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Take everything written to the debug port so far.
    pub fn drain_debug(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bus.debug_out)
    }
}
