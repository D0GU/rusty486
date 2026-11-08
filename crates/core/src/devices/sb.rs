//! Sound Blaster 2.0 DSP at 220h (IRQ 5, DMA 1): 8-bit PCM by single-cycle
//! or auto-init DMA, direct DAC output, and the usual detection commands.
//! The OPL2 at 220h-223h/228h-229h lives in `opl`.

use std::collections::VecDeque;

pub const BASE: u16 = 0x220;
pub const IRQ: u8 = 5;
pub const DMA_CHANNEL: usize = 1;

#[derive(Clone, Debug, Default)]
pub struct SoundBlaster {
    reset_latch: bool,
    out: VecDeque<u8>,
    /// Pending command and the parameter bytes it still needs.
    cmd: Option<u8>,
    params: Vec<u8>,
    need: usize,
    pub speaker: bool,
    pub time_constant: u8,
    /// Current DAC output level (unsigned 8-bit, 0x80 = silence).
    pub dac: u8,
    /// DMA playback state.
    pub dma_active: bool,
    pub auto_init: bool,
    pub paused: bool,
    pub block_size: u32,
    /// Bytes left in the current block.
    pub remaining: u32,
    /// Finish the auto-init loop at the end of the current block.
    exit_auto: bool,
    /// An 8-bit DMA interrupt is pending acknowledgement (read 22Eh).
    pub irq_pending: bool,
}

impl SoundBlaster {
    pub fn handles(port: u16) -> bool {
        matches!(port, 0x226 | 0x22A | 0x22C | 0x22E)
    }

    /// PCM sample rate from the time constant.
    pub fn rate(&self) -> u32 {
        1_000_000 / (256 - self.time_constant as u32).max(1)
    }

    pub fn read(&mut self, port: u16) -> u8 {
        match port {
            0x22A => self.out.pop_front().unwrap_or(0xFF),
            0x22C => 0x7F, // ready for a command (bit 7 clear)
            0x22E => {
                self.irq_pending = false;
                if self.out.is_empty() { 0x7F } else { 0xFF }
            }
            _ => 0xFF,
        }
    }

    /// Port write. Returns true if an interrupt should be raised now.
    pub fn write(&mut self, port: u16, v: u8) -> bool {
        match port {
            0x226 => {
                if v & 1 != 0 {
                    self.reset_latch = true;
                } else if self.reset_latch {
                    self.reset_latch = false;
                    *self = SoundBlaster::default();
                    self.out.push_back(0xAA);
                    self.dac = 0x80;
                }
                false
            }
            0x22C => self.command_byte(v),
            _ => false,
        }
    }

    fn command_byte(&mut self, v: u8) -> bool {
        if let Some(cmd) = self.cmd {
            self.params.push(v);
            if self.params.len() < self.need {
                return false;
            }
            self.cmd = None;
            let p = std::mem::take(&mut self.params);
            return self.execute(cmd, &p);
        }
        let need = match v {
            0x10 | 0x40 | 0xE0 | 0xE4 => 1,
            0x14 | 0x24 | 0x48 | 0x91 | 0x41 | 0x42 => 2,
            _ => 0,
        };
        if need == 0 {
            return self.execute(v, &[]);
        }
        self.cmd = Some(v);
        self.need = need;
        false
    }

    fn execute(&mut self, cmd: u8, p: &[u8]) -> bool {
        let word = || p[0] as u32 | (p.get(1).copied().unwrap_or(0) as u32) << 8;
        match cmd {
            0x10 => self.dac = p[0],                          // direct DAC
            0x14 | 0x91 => {
                // 8-bit single-cycle DMA output
                self.block_size = word() + 1;
                self.remaining = self.block_size;
                self.dma_active = true;
                self.auto_init = false;
                self.paused = false;
            }
            0x1C | 0x90 => {
                // 8-bit auto-init DMA output (block size from 48h)
                self.remaining = self.block_size.max(1);
                self.dma_active = true;
                self.auto_init = true;
                self.paused = false;
                self.exit_auto = false;
            }
            0x20 => self.out.push_back(0x80),                // direct ADC: silence
            0x40 => self.time_constant = p[0],
            0x41 | 0x42 => {
                // SB16 output rate (Hz, high byte first): map to a time constant.
                let hz = ((p[0] as u32) << 8 | p[1] as u32).max(4000);
                self.time_constant = (256 - 1_000_000 / hz).min(255) as u8;
            }
            0x48 => self.block_size = word() + 1,
            0xD0 => self.paused = true,
            0xD1 => self.speaker = true,
            0xD3 => self.speaker = false,
            0xD4 => self.paused = false,
            0xD8 => self.out.push_back(if self.speaker { 0xFF } else { 0x00 }),
            0xDA => self.exit_auto = true,
            0xE0 => self.out.push_back(!p[0]),                // DSP identification
            0xE1 => {
                self.out.push_back(2); // DSP version 2.01
                self.out.push_back(1);
            }
            0xE4 => {}                                        // write test register
            0xE8 => self.out.push_back(0),
            0xF2 => {
                self.irq_pending = true;
                return true;
            }
            _ => {}
        }
        false
    }

    /// Called once per PCM sample period while DMA playback runs: stores the
    /// byte read from DMA. Returns true at the end of a block (raise IRQ).
    pub fn dma_sample(&mut self, byte: u8) -> bool {
        self.dac = byte;
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining > 0 {
            return false;
        }
        if self.auto_init && !self.exit_auto {
            self.remaining = self.block_size.max(1);
        } else {
            self.dma_active = false;
            self.exit_auto = false;
        }
        self.irq_pending = true;
        true
    }
}
