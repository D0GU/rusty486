//! The complete rusty486 machine: CPU + memory + devices.

use std::collections::BTreeSet;

use crate::bus::Bus;
use crate::cpu::{Cpu, CS};
use crate::devices::audio::{self, Audio};
use crate::devices::disk::{self, DiskController, DiskImage};
use crate::devices::dma::Dma;
use crate::devices::input::Input;
use crate::devices::opl::Opl2;
use crate::devices::sb::{self, SoundBlaster};
use crate::devices::pic::Pic;
use crate::devices::pit::Pit;
use crate::devices::rtc::Rtc;
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
    pub pic: Pic,
    pub pit: Pit,
    pub input: Input,
    pub rtc: Rtc,
    pub dma: Dma,
    pub opl: Opl2,
    pub sb: SoundBlaster,
    pub audio: Audio,
    /// Next CPU cycle at which to bring the audio up to date.
    audio_due: u64,
    /// Bytes written to the debug port since the frontend last drained it.
    pub debug_out: Vec<u8>,
    /// Last value written to a POST diagnostic port (80h or 190h).
    pub post_code: u8,
    /// Fast A20 gate (port 92h bit 1). When off, address line 20 is forced low.
    pub a20: bool,
    /// CPU cycle count and clock as of the last tick (for time-based devices).
    pub now: u64,
    pub clock_hz: u64,
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
            pic: Pic::default(),
            pit: Pit::default(),
            input: Input::default(),
            rtc: Rtc::default(),
            dma: Dma::default(),
            opl: Opl2::new(audio::RATE as f32),
            sb: SoundBlaster::default(),
            audio: Audio::default(),
            audio_due: 0,
            debug_out: Vec::new(),
            post_code: 0,
            a20: true,
            now: 0,
            clock_hz: 33_000_000,
        }
    }

    /// Advance time-based devices to CPU cycle `now`.
    pub fn tick(&mut self, now: u64) {
        let delta = now.saturating_sub(self.now);
        self.now = now;
        if delta > 0 && self.pit.advance_cycles(delta, self.clock_hz) > 0 {
            self.pic.raise(0);
        }
        if now >= self.audio_due {
            self.audio_catch_up();
        }
    }

    /// Generate audio (and run Sound Blaster DMA) up to the current cycle.
    pub fn audio_catch_up(&mut self) {
        let now = self.now as f64;
        let clock = self.clock_hz as f64;
        let per_sample = clock / audio::RATE as f64;
        if self.audio.next_sample < now - clock {
            // Far behind (e.g. after a pause): skip ahead.
            self.audio.next_sample = now;
            self.audio.sb_next = now;
        }
        while self.audio.next_sample <= now {
            let t = self.audio.next_sample;
            // Sound Blaster DMA: one byte per PCM sample period.
            if self.sb.dma_active && !self.sb.paused {
                let period = clock / self.sb.rate() as f64;
                if self.audio.sb_next < t - clock / 10.0 {
                    self.audio.sb_next = t;
                }
                while self.audio.sb_next <= t {
                    match self.dma.read_byte(sb::DMA_CHANNEL, &self.ram) {
                        Some((b, _)) => {
                            if self.sb.dma_sample(b) {
                                self.pic.raise(sb::IRQ);
                            }
                        }
                        None => break, // DMA channel masked: stall
                    }
                    self.audio.sb_next += period;
                    if !self.sb.dma_active {
                        break;
                    }
                }
            } else {
                self.audio.sb_next = t;
            }
            let opl = self.opl.sample();
            let dac = self.sb.speaker.then_some(self.sb.dac);
            let tone = self.pit.speaker_tone();
            let level = (self.pit.speaker_enable && tone.is_none()).then_some(self.pit.ch[2].out);
            self.audio.mix(opl, dac, tone, level);
            self.audio.next_sample += per_sample;
        }
        // Catch up again in about a millisecond.
        self.audio_due = self.now + self.clock_hz / 1000;
    }

    /// CPU cycles until a device needs attention (for halt skipping).
    pub fn cycles_to_next_event(&self) -> Option<u64> {
        let pit = self.pit.cycles_to_irq0(self.clock_hz);
        let audio = (self.sb.dma_active || self.sb.speaker).then(|| self.audio_due.saturating_sub(self.now).max(1));
        match (pit, audio) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
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
        } else if (0xA0000..0xB0000).contains(&addr) && self.video.graphics() {
            self.video.vram[self.video.window_offset(addr)]
        } else if (video::LFB_BASE..video::LFB_BASE + video::VRAM_SIZE as u32).contains(&addr) {
            self.video.vram[(addr - video::LFB_BASE) as usize]
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
        if (0xA0000..0xB0000).contains(&addr) && self.video.graphics() {
            let o = self.video.window_offset(addr);
            self.video.vram[o] = value;
            return;
        }
        if (video::LFB_BASE..video::LFB_BASE + video::VRAM_SIZE as u32).contains(&addr) {
            self.video.vram[(addr - video::LFB_BASE) as usize] = value;
            return;
        }
        if let Some(b) = self.ram.get_mut(addr as usize) {
            *b = value;
        }
    }

    fn io_read16(&mut self, port: u16) -> u16 {
        if port == 0x1CE || port == 0x1CF {
            return self.video.read16(port);
        }
        u16::from_le_bytes([self.io_read8(port), self.io_read8(port.wrapping_add(1))])
    }

    fn io_write16(&mut self, port: u16, value: u16) {
        if port == 0x1CE || port == 0x1CF {
            return self.video.write16(port, value);
        }
        let [a, b] = value.to_le_bytes();
        self.io_write8(port, a);
        self.io_write8(port.wrapping_add(1), b);
    }

    fn io_read8(&mut self, port: u16) -> u8 {
        match port {
            0x92 => (self.a20 as u8) << 1,
            p if Pic::handles(p) => self.pic.read(p),
            p if Dma::handles(p) => self.dma.read(p),
            p if Opl2::handles(p) => {
                if matches!(p, 0x388 | 0x220 | 0x222 | 0x228) {
                    self.opl.read_status(self.now)
                } else {
                    0xFF
                }
            }
            p if SoundBlaster::handles(p) => self.sb.read(p),
            p if Pit::handles(p) => self.pit.read(p),
            p if Rtc::handles(p) => self.rtc.read(p, self.now, self.clock_hz),
            p if Input::handles(p) => self.input.read(p),
            p if Video::handles(p) => self.video.read(p, self.now, self.clock_hz),
            p if (disk::BASE_PORT..disk::BASE_PORT + 0x20).contains(&p) => self.disk.read(p),
            _ => 0xFF,
        }
    }

    fn io_write8(&mut self, port: u16, value: u8) {
        match port {
            DEBUG_PORT => self.debug_out.push(value),
            0x80 | 0x190 => self.post_code = value,
            0x92 => self.a20 = value & 2 != 0,
            p if Pic::handles(p) => self.pic.write(p, value),
            p if Pit::handles(p) => {
                if matches!(p, 0x42 | 0x43 | 0x61) {
                    self.audio_catch_up();
                }
                self.pit.write(p, value)
            }
            p if Dma::handles(p) => self.dma.write(p, value),
            p if Opl2::handles(p) => {
                if matches!(p, 0x388 | 0x220 | 0x222 | 0x228) {
                    self.opl.write_addr(value);
                } else {
                    self.audio_catch_up();
                    self.opl.write_data(value, self.now, self.clock_hz);
                }
            }
            p if SoundBlaster::handles(p) => {
                self.audio_catch_up();
                if self.sb.write(p, value) {
                    self.pic.raise(sb::IRQ);
                }
            }
            p if Rtc::handles(p) => self.rtc.write(p, value),
            p if Input::handles(p) => {
                if self.input.write(p, value) {
                    self.pic.raise(1);
                }
            }
            p if Video::handles(p) => self.video.write(p, value),
            p if (disk::BASE_PORT..disk::BASE_PORT + 0x20).contains(&p) => {
                self.disk.write(p, value, &mut self.ram)
            }
            _ => {}
        }
    }

    fn irq_ack(&mut self) -> Option<u8> {
        self.pic.ack()
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
        self.bus.clock_hz = self.clock_hz;
        self.cpu.step(&mut self.bus);
        self.bus.tick(self.cpu.cycles);
    }

    /// Set the real-time clock (local time, seconds since 1970).
    pub fn set_time(&mut self, secs: i64) {
        self.bus.rtc.set_time(secs, self.cpu.cycles);
    }

    /// Keyboard event: set-1 scancode, extended (E0) flag, down/up, the ASCII
    /// character it produces (0 if none), BIOS shift flags after the event,
    /// and whether it's a modifier key.
    pub fn key_event(&mut self, scancode: u8, extended: bool, down: bool, ascii: u8, flags: u8, modifier: bool) {
        if self.bus.input.key(scancode, extended, down, ascii, flags, modifier) {
            self.bus.pic.raise(1);
        }
    }

    pub fn mouse_move(&mut self, x: u16, y: u16) {
        if self.bus.input.mouse_move(x, y) {
            self.bus.pic.raise(1);
        }
    }

    pub fn mouse_buttons(&mut self, buttons: u8) {
        if self.bus.input.mouse_buttons(buttons) {
            self.bus.pic.raise(1);
        }
    }

    pub fn insert_disk(&mut self, drive: Drive, image: DiskImage) {
        self.bus.disk.drives[drive as usize] = Some(image);
        self.bus.disk.touch();
    }

    pub fn eject_disk(&mut self, drive: Drive) -> Option<DiskImage> {
        self.bus.disk.touch();
        self.bus.disk.drives[drive as usize].take()
    }

    /// Mutable access to a disk image for host-side edits (marks the media
    /// as changed so the guest drops its caches).
    pub fn disk_mut(&mut self, drive: Drive) -> Option<&mut DiskImage> {
        self.bus.disk.touch();
        self.bus.disk.drives[drive as usize].as_mut()
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
        self.bus.clock_hz = self.clock_hz;
        let end = self.cpu.cycles + cycles;
        while self.cpu.cycles < end {
            if self.cpu.shutdown {
                self.cpu.cycles = end;
                self.bus.tick(end);
                return RunResult::Shutdown;
            }
            if self.cpu.halted {
                if self.cpu.eflags & crate::cpu::flags::IF == 0 {
                    // Halted with interrupts off: nothing can wake us. Skip ahead.
                    self.cpu.cycles = end;
                    self.bus.tick(end);
                    break;
                }
                if !self.bus.pic.has_pending() {
                    // Waiting for an interrupt: jump to the next device event.
                    let left = end - self.cpu.cycles;
                    let skip = self.bus.cycles_to_next_event().unwrap_or(left).min(left);
                    self.cpu.cycles += skip.saturating_sub(1);
                }
            }
            self.cpu.step(&mut self.bus);
            self.bus.tick(self.cpu.cycles);
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

    /// Take the audio generated so far (44.1 kHz mono).
    pub fn take_audio(&mut self) -> Vec<i16> {
        self.bus.audio_catch_up();
        std::mem::take(&mut self.bus.audio.out)
    }

    /// Take everything written to the debug port so far.
    pub fn drain_debug(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bus.debug_out)
    }
}
