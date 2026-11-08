//! 8254 programmable interval timer at 40h-43h, clocked at 1.193182 MHz.
//! Channel 0 drives IRQ0; channel 2 drives the PC speaker (gated by port 61h).

pub const PIT_HZ: u64 = 1_193_182;

#[derive(Clone, Debug)]
pub struct Channel {
    pub mode: u8,
    /// 1 = low byte, 2 = high byte, 3 = low then high.
    access: u8,
    /// Reload value; 0 means 65536.
    pub reload: u32,
    /// Ticks left in the current period.
    count: u32,
    latch: Option<u16>,
    read_high: bool,
    write_high: bool,
    write_low: u8,
    /// Counting has been armed by a complete reload write.
    pub running: bool,
    pub gate: bool,
    /// Output level (mode 0: goes high at terminal count).
    pub out: bool,
}

impl Default for Channel {
    fn default() -> Self {
        Channel {
            mode: 3,
            access: 3,
            reload: 0x10000,
            count: 0x10000,
            latch: None,
            read_high: false,
            write_high: false,
            write_low: 0,
            running: false,
            gate: true,
            out: true,
        }
    }
}

impl Channel {
    fn period(&self) -> u32 {
        if self.reload == 0 {
            0x10000
        } else {
            self.reload
        }
    }

    fn load(&mut self, v: u16) {
        self.reload = if v == 0 { 0x10000 } else { v as u32 };
        self.count = self.reload;
        self.running = true;
        self.out = self.mode != 0;
    }

    /// Advance by `ticks`, returning the number of rising output edges.
    fn advance(&mut self, ticks: u64) -> u64 {
        if !self.running || !self.gate || ticks == 0 {
            return 0;
        }
        match self.mode {
            0 | 1 | 4 | 5 => {
                // One-shot: output rises once at terminal count.
                if self.out {
                    self.count = (self.count as u64).wrapping_sub(ticks) as u32;
                    return 0;
                }
                if ticks >= self.count as u64 {
                    self.out = true;
                    self.count = 0;
                    1
                } else {
                    self.count -= ticks as u32;
                    0
                }
            }
            _ => {
                // Periodic (modes 2 and 3): one rising edge per period.
                let p = self.period() as u64;
                let t = ticks + (p - self.count as u64);
                let edges = t / p;
                self.count = (p - t % p) as u32;
                self.out = self.mode != 3 || self.count as u64 > p / 2;
                edges
            }
        }
    }

    /// Ticks until the next rising edge, if one will happen.
    fn ticks_to_edge(&self) -> Option<u64> {
        if !self.running || !self.gate {
            return None;
        }
        match self.mode {
            0 | 1 | 4 | 5 if !self.out => Some(self.count as u64),
            2 | 3 | 6 | 7 => Some(self.count as u64),
            _ => None,
        }
    }

    fn current(&self) -> u16 {
        self.count as u16
    }
}

#[derive(Clone, Debug, Default)]
pub struct Pit {
    pub ch: [Channel; 3],
    /// Fractional PIT ticks carried between CPU-cycle conversions
    /// (numerator in units of 1/clock_hz).
    frac: u64,
    /// Port 61h bit 1: speaker data enable.
    pub speaker_enable: bool,
    refresh: bool,
}

impl Pit {
    pub fn handles(port: u16) -> bool {
        (0x40..=0x43).contains(&port) || port == 0x61
    }

    /// Advance by CPU cycles at `clock_hz`. Returns rising edges on channel 0.
    pub fn advance_cycles(&mut self, cycles: u64, clock_hz: u64) -> u64 {
        let total = cycles * PIT_HZ + self.frac;
        let ticks = total / clock_hz;
        self.frac = total % clock_hz;
        self.refresh ^= ticks & 1 == 1;
        let edges = self.ch[0].advance(ticks);
        self.ch[1].advance(ticks);
        self.ch[2].advance(ticks);
        edges
    }

    /// CPU cycles until channel 0's next rising edge.
    pub fn cycles_to_irq0(&self, clock_hz: u64) -> Option<u64> {
        let t = self.ch[0].ticks_to_edge()?;
        let need = (t * clock_hz).saturating_sub(self.frac);
        Some(need.div_ceil(PIT_HZ).max(1))
    }

    /// Frequency the speaker is playing, if it's on (for audio output).
    pub fn speaker_tone(&self) -> Option<f64> {
        let c = &self.ch[2];
        (self.speaker_enable && c.gate && c.running && matches!(c.mode, 2 | 3))
            .then(|| PIT_HZ as f64 / c.period() as f64)
    }

    pub fn read(&mut self, port: u16) -> u8 {
        if port == 0x61 {
            let c2 = &self.ch[2];
            return (c2.gate as u8) | (self.speaker_enable as u8) << 1 | (self.refresh as u8) << 4 | (c2.out as u8) << 5;
        }
        if port == 0x43 {
            return 0xFF;
        }
        let c = &mut self.ch[(port - 0x40) as usize];
        let v = c.latch.unwrap_or_else(|| c.current());
        let byte = match c.access {
            1 => v as u8,
            2 => (v >> 8) as u8,
            _ => {
                let b = if c.read_high { (v >> 8) as u8 } else { v as u8 };
                c.read_high = !c.read_high;
                if !c.read_high {
                    c.latch = None;
                }
                return b;
            }
        };
        c.latch = None;
        byte
    }

    pub fn write(&mut self, port: u16, v: u8) {
        match port {
            0x61 => {
                self.ch[2].gate = v & 1 != 0;
                self.speaker_enable = v & 2 != 0;
            }
            0x43 => {
                let sel = (v >> 6) as usize;
                if sel == 3 {
                    // Read-back: latch counts of selected channels.
                    for i in 0..3 {
                        if v & (2 << i) != 0 && v & 0x20 == 0 {
                            let cur = self.ch[i].current();
                            self.ch[i].latch = Some(cur);
                        }
                    }
                    return;
                }
                let c = &mut self.ch[sel];
                let access = (v >> 4) & 3;
                if access == 0 {
                    c.latch = Some(c.current());
                    c.read_high = false;
                    return;
                }
                c.access = access;
                c.mode = match (v >> 1) & 7 {
                    6 => 2,
                    7 => 3,
                    m => m,
                };
                c.read_high = false;
                c.write_high = false;
                c.running = false;
                c.out = c.mode != 0;
            }
            _ => {
                let c = &mut self.ch[(port - 0x40) as usize];
                match c.access {
                    1 => c.load(v as u16),
                    2 => c.load((v as u16) << 8),
                    _ => {
                        if c.write_high {
                            let full = (v as u16) << 8 | c.write_low as u16;
                            c.write_high = false;
                            c.load(full);
                        } else {
                            c.write_low = v;
                            c.write_high = true;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_18hz() {
        let mut p = Pit::default();
        p.write(0x43, 0x36);
        p.write(0x40, 0);
        p.write(0x40, 0);
        // One second at 33 MHz should give ~18.2 interrupts.
        let mut edges = 0;
        for _ in 0..1000 {
            edges += p.advance_cycles(33_000, 33_000_000);
        }
        assert!((18..=19).contains(&edges), "{edges}");
    }

    #[test]
    fn latch_read() {
        let mut p = Pit::default();
        p.write(0x43, 0x34);
        p.write(0x40, 0x00);
        p.write(0x40, 0x10); // 0x1000
        p.advance_cycles(0x100 * 33_000_000 / PIT_HZ + 1, 33_000_000);
        p.write(0x43, 0x00);
        let lo = p.read(0x40);
        let hi = p.read(0x40);
        let v = (hi as u16) << 8 | lo as u16;
        assert!((0xEFE..=0xF01).contains(&v), "{v:#x}");
    }
}
