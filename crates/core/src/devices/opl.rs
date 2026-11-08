//! Yamaha YM3812 (OPL2) FM synthesizer, as on the AdLib and Sound Blaster.
//!
//! A behavioural model rather than a die-level one: 9 two-operator channels
//! (FM or additive), 4 waveforms, ADSR envelopes in the decibel domain,
//! feedback, tremolo and vibrato, rhythm mode and the two timers (used by
//! programs to detect the chip). Ports 388h/389h (also 220h-223h, 228h/229h).

use std::f32::consts::TAU;

/// The OPL2's native sample rate.
pub const OPL_RATE: f32 = 49_716.0;
const SINE_LEN: usize = 1024;

/// Register offset (0-21) -> operator number (0-17), or None.
fn op_index(off: u8) -> Option<usize> {
    const MAP: [i8; 22] = [0, 1, 2, 3, 4, 5, -1, -1, 6, 7, 8, 9, 10, 11, -1, -1, 12, 13, 14, 15, 16, 17];
    MAP.get(off as usize).filter(|v| **v >= 0).map(|v| *v as usize)
}

/// Channel -> (modulator operator, carrier operator).
fn channel_ops(ch: usize) -> (usize, usize) {
    let base = (ch % 3) + (ch / 3) * 6;
    (base, base + 3)
}

const MULT: [f32; 16] = [0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 10.0, 12.0, 12.0, 15.0, 15.0];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    #[default]
    Off,
}

#[derive(Clone, Debug, Default)]
struct Operator {
    am: bool,
    vib: bool,
    sustain: bool, // EG type: hold at the sustain level while the key is on
    ksr: bool,
    mult: u8,
    ksl: u8,
    tl: u8,
    ar: u8,
    dr: u8,
    sl: u8,
    rr: u8,
    wave: u8,
    phase: f32,
    /// Envelope attenuation in dB (0 = loudest, 96 = silent).
    env: f32,
    stage: Stage,
    /// Last two outputs, for feedback.
    out1: f32,
    out2: f32,
}

#[derive(Clone, Debug, Default)]
struct Channel {
    fnum: u16,
    block: u8,
    key: bool,
    feedback: u8,
    additive: bool,
}

#[derive(Clone, Debug)]
pub struct Opl2 {
    addr: u8,
    regs: [u8; 256],
    ops: [Operator; 18],
    ch: [Channel; 9],
    sine: Vec<f32>,
    /// Time accumulator for the LFOs (seconds).
    lfo_t: f32,
    noise: u32,
    // Timers: start cycle and period in cycles (0 = stopped).
    t1_start: u64,
    t2_start: u64,
    t1_period: u64,
    t2_period: u64,
    status: u8,
    /// Output sample rate the caller asks for.
    rate: f32,
}

impl Default for Opl2 {
    fn default() -> Self {
        Self::new(44_100.0)
    }
}

impl Opl2 {
    pub fn new(rate: f32) -> Self {
        let sine = (0..SINE_LEN).map(|i| (i as f32 / SINE_LEN as f32 * TAU).sin()).collect();
        let mut o = Opl2 {
            addr: 0,
            regs: [0; 256],
            ops: Default::default(),
            ch: Default::default(),
            sine,
            lfo_t: 0.0,
            noise: 1,
            t1_start: 0,
            t2_start: 0,
            t1_period: 0,
            t2_period: 0,
            status: 0,
            rate,
        };
        for op in &mut o.ops {
            op.env = 96.0;
        }
        o
    }

    pub fn handles(port: u16) -> bool {
        matches!(port, 0x388 | 0x389 | 0x220..=0x223 | 0x228 | 0x229)
    }

    /// Status register (timer flags), updated for the current time.
    pub fn read_status(&mut self, now: u64) -> u8 {
        let ctrl = self.regs[4];
        if self.t1_period > 0 && now >= self.t1_start + self.t1_period && ctrl & 0x40 == 0 {
            self.status |= 0x40;
        }
        if self.t2_period > 0 && now >= self.t2_start + self.t2_period && ctrl & 0x20 == 0 {
            self.status |= 0x20;
        }
        if self.status & 0x60 != 0 {
            self.status |= 0x80;
        }
        self.status
    }

    pub fn write_addr(&mut self, v: u8) {
        self.addr = v;
    }

    /// Write the selected register. `now`/`clock_hz` time the timers.
    pub fn write_data(&mut self, v: u8, now: u64, clock_hz: u64) {
        let r = self.addr;
        self.regs[r as usize] = v;
        match r {
            0x04 => {
                if v & 0x80 != 0 {
                    self.status = 0;
                    return;
                }
                let us = |n: u64| n * clock_hz / 1_000_000;
                if v & 1 != 0 {
                    self.t1_start = now;
                    self.t1_period = us((256 - self.regs[2] as u64) * 80);
                } else {
                    self.t1_period = 0;
                }
                if v & 2 != 0 {
                    self.t2_start = now;
                    self.t2_period = us((256 - self.regs[3] as u64) * 320);
                } else {
                    self.t2_period = 0;
                }
            }
            0x20..=0x35 => {
                if let Some(i) = op_index(r - 0x20) {
                    let o = &mut self.ops[i];
                    o.am = v & 0x80 != 0;
                    o.vib = v & 0x40 != 0;
                    o.sustain = v & 0x20 != 0;
                    o.ksr = v & 0x10 != 0;
                    o.mult = v & 15;
                }
            }
            0x40..=0x55 => {
                if let Some(i) = op_index(r - 0x40) {
                    self.ops[i].ksl = v >> 6;
                    self.ops[i].tl = v & 63;
                }
            }
            0x60..=0x75 => {
                if let Some(i) = op_index(r - 0x60) {
                    self.ops[i].ar = v >> 4;
                    self.ops[i].dr = v & 15;
                }
            }
            0x80..=0x95 => {
                if let Some(i) = op_index(r - 0x80) {
                    self.ops[i].sl = v >> 4;
                    self.ops[i].rr = v & 15;
                }
            }
            0xA0..=0xA8 => {
                let c = &mut self.ch[(r - 0xA0) as usize];
                c.fnum = (c.fnum & 0x300) | v as u16;
            }
            0xB0..=0xB8 => {
                let ch = (r - 0xB0) as usize;
                let c = &mut self.ch[ch];
                c.fnum = (c.fnum & 0xFF) | ((v & 3) as u16) << 8;
                c.block = (v >> 2) & 7;
                let key = v & 0x20 != 0;
                if key != c.key {
                    c.key = key;
                    let (m, k) = channel_ops(ch);
                    self.key(m, key);
                    self.key(k, key);
                }
            }
            0xBD => {
                // Rhythm mode key-ons: BD (ch6), SD (ch7 car), TOM (ch8 mod),
                // CYM (ch8 car), HH (ch7 mod).
                if v & 0x20 != 0 {
                    let (m6, c6) = channel_ops(6);
                    let (m7, c7) = channel_ops(7);
                    let (m8, c8) = channel_ops(8);
                    for (op, bit) in [(m6, 0x10), (c6, 0x10), (c7, 0x08), (m8, 0x04), (c8, 0x02), (m7, 0x01)] {
                        let on = v & bit != 0;
                        let was = self.ops[op].stage != Stage::Off && self.ops[op].stage != Stage::Release;
                        if on != was {
                            self.key(op, on);
                        }
                    }
                }
            }
            0xC0..=0xC8 => {
                let c = &mut self.ch[(r - 0xC0) as usize];
                c.feedback = (v >> 1) & 7;
                c.additive = v & 1 != 0;
            }
            0xE0..=0xF5 => {
                if let Some(i) = op_index(r - 0xE0) {
                    self.ops[i].wave = if self.regs[1] & 0x20 != 0 { v & 3 } else { 0 };
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, op: usize, on: bool) {
        let o = &mut self.ops[op];
        if on {
            o.stage = Stage::Attack;
            o.phase = 0.0;
        } else if o.stage != Stage::Off {
            o.stage = Stage::Release;
        }
    }

    /// Time in seconds for an envelope rate (attack: full rise; decay/release:
    /// 0 to 96 dB), approximating the OPL2 rate tables.
    fn rate_time(rate: u8, ksr_boost: u8, attack: bool) -> f32 {
        if rate == 0 {
            return f32::INFINITY;
        }
        let eff = (rate as f32 * 4.0 + ksr_boost as f32).min(63.0);
        if eff >= 60.0 && attack {
            return 0.0;
        }
        let base = if attack { 2.826 } else { 39.28 };
        base / 2f32.powf((eff - 4.0) / 4.0)
    }

    fn wave(&self, w: u8, phase: f32) -> f32 {
        let p = phase.rem_euclid(1.0);
        let s = self.sine[(p * SINE_LEN as f32) as usize % SINE_LEN];
        match w {
            0 => s,
            1 => s.max(0.0),
            2 => s.abs(),
            _ => {
                if (p * 4.0) as u32 % 2 == 0 {
                    s.abs()
                } else {
                    0.0
                }
            }
        }
    }

    /// Advance an operator one output sample; returns its envelope gain.
    fn envelope(&mut self, op: usize, ch: usize) -> f32 {
        let dt = 1.0 / self.rate;
        let block = self.ch[ch].block;
        let fnum = self.ch[ch].fnum;
        let o = &mut self.ops[op];
        let ksr_boost = if o.ksr { block * 2 + (fnum >> 9) as u8 } else { (block * 2 + (fnum >> 9) as u8) >> 2 };
        let sl_db = if o.sl == 15 { 93.0 } else { o.sl as f32 * 3.0 };
        match o.stage {
            Stage::Attack => {
                let t = Self::rate_time(o.ar, ksr_boost, true);
                if t == 0.0 {
                    o.env = 0.0;
                } else if t.is_finite() {
                    // Exponential approach to full level.
                    let k = (dt / t * 8.0).min(1.0);
                    o.env -= (o.env + 1.0) * k;
                }
                if o.env <= 0.0 {
                    o.env = 0.0;
                    o.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let t = Self::rate_time(o.dr, ksr_boost, false);
                o.env += 96.0 * dt / t;
                if o.env >= sl_db {
                    o.env = sl_db;
                    o.stage = if o.sustain { Stage::Sustain } else { Stage::Release };
                }
            }
            Stage::Sustain => {}
            Stage::Release => {
                let t = Self::rate_time(o.rr, ksr_boost, false);
                o.env += 96.0 * dt / t;
                if o.env >= 96.0 {
                    o.env = 96.0;
                    o.stage = Stage::Off;
                }
            }
            Stage::Off => o.env = 96.0,
        }
        let tl = o.tl as f32 * 0.75;
        let ksl = match o.ksl {
            0 => 0.0,
            k => (block as f32 * 3.0 * [0.0, 1.0, 0.5, 2.0][k as usize]).min(24.0),
        };
        let am_depth = if self.regs[0xBD] & 0x80 != 0 { 4.8 } else { 1.0 };
        let trem = if self.ops[op].am {
            am_depth * 0.5 * (1.0 - (self.lfo_t * 3.7 * TAU).cos())
        } else {
            0.0
        };
        let att = self.ops[op].env + tl + ksl + trem;
        if att >= 96.0 {
            0.0
        } else {
            10f32.powf(-att / 20.0)
        }
    }

    fn freq(&self, ch: usize, op: usize) -> f32 {
        let c = &self.ch[ch];
        let base = c.fnum as f32 * OPL_RATE / (1u32 << (20 - c.block)) as f32;
        let mut f = base * MULT[self.ops[op].mult as usize];
        if self.ops[op].vib {
            let depth = if self.regs[0xBD] & 0x40 != 0 { 14.0 } else { 7.0 };
            f *= 2f32.powf(depth / 1200.0 * (self.lfo_t * 6.1 * TAU).sin());
        }
        f
    }

    /// Compute one operator: advance its phase and return its output.
    fn operator(&mut self, ch: usize, op: usize, modulation: f32) -> f32 {
        let gain = self.envelope(op, ch);
        let f = self.freq(ch, op);
        let o = &mut self.ops[op];
        o.phase = (o.phase + f / self.rate).fract();
        if gain == 0.0 {
            return 0.0;
        }
        let w = self.ops[op].wave;
        let p = self.ops[op].phase + modulation;
        self.wave(w, p) * gain
    }

    /// Generate one output sample in roughly [-1, 1].
    pub fn sample(&mut self) -> f32 {
        self.lfo_t += 1.0 / self.rate;
        if self.lfo_t > 1000.0 {
            self.lfo_t -= 1000.0;
        }
        let rhythm = self.regs[0xBD] & 0x20 != 0;
        let mut mix = 0.0;
        for ch in 0..9 {
            if rhythm && ch >= 7 {
                continue;
            }
            let (m, c) = channel_ops(ch);
            if self.ops[m].stage == Stage::Off && self.ops[c].stage == Stage::Off {
                continue;
            }
            // Feedback: the modulator modulates itself (in cycles of phase).
            let fb = self.ch[ch].feedback;
            let fb_mod = if fb > 0 {
                (self.ops[m].out1 + self.ops[m].out2) * 0.5 * 4.0 / (1u32 << (9 - fb)) as f32 * 2.0
            } else {
                0.0
            };
            let mo = self.operator(ch, m, fb_mod);
            self.ops[m].out2 = self.ops[m].out1;
            self.ops[m].out1 = mo;
            let out = if self.ch[ch].additive {
                mo + self.operator(ch, c, 0.0)
            } else {
                // Full-scale modulation shifts the carrier by +-4 cycles.
                self.operator(ch, c, mo * 4.0)
            };
            mix += out;
        }
        if rhythm {
            mix += self.percussion();
        }
        mix / 4.0
    }

    /// Rhythm-mode snare, tom, cymbal and hi-hat (approximations).
    fn percussion(&mut self) -> f32 {
        // 23-bit noise generator.
        let bit = ((self.noise >> 22) ^ (self.noise >> 8)) & 1;
        self.noise = (self.noise << 1 | bit) & 0x7F_FFFF;
        let noise: f32 = if bit == 1 { 1.0 } else { -1.0 };
        let (m7, c7) = channel_ops(7);
        let (m8, c8) = channel_ops(8);
        let hh = self.operator(7, m7, 0.0).signum() * self.envelope_level(m7) * noise.max(0.0);
        let sd = self.operator(7, c7, 0.0) * 0.5 + noise * self.envelope_level(c7) * 0.5;
        let tom = self.operator(8, m8, 0.0);
        let cym = self.operator(8, c8, 0.0).signum() * self.envelope_level(c8) * 0.5;
        (hh + sd + tom + cym) * 0.7
    }

    fn envelope_level(&self, op: usize) -> f32 {
        let o = &self.ops[op];
        let att = o.env + o.tl as f32 * 0.75;
        if att >= 96.0 {
            0.0
        } else {
            10f32.powf(-att / 20.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(o: &mut Opl2, r: u8, v: u8) {
        o.write_addr(r);
        o.write_data(v, 0, 33_000_000);
    }

    #[test]
    fn plays_a_note() {
        let mut o = Opl2::new(44_100.0);
        // A simple instrument on channel 0 (operators 0 and 3).
        write(&mut o, 0x20, 0x01);
        write(&mut o, 0x23, 0x01);
        write(&mut o, 0x40, 0x3F); // modulator silent: a pure carrier tone
        write(&mut o, 0x43, 0x00);
        write(&mut o, 0x60, 0xF0);
        write(&mut o, 0x63, 0xF0);
        write(&mut o, 0x80, 0x77);
        write(&mut o, 0x83, 0x77);
        write(&mut o, 0xA0, 0x98);
        write(&mut o, 0xB0, 0x31); // key on, F-number 0x198, block 4: ~309 Hz
        let samples: Vec<f32> = (0..4410).map(|_| o.sample()).collect();
        let peak = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(peak > 0.05, "silent: {peak}");
        // Zero crossings in 0.1 s: about 2 * 309 * 0.1.
        let crossings = samples.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((55..=70).contains(&crossings), "crossings {crossings}");
        write(&mut o, 0xB0, 0x11); // key off
        for _ in 0..44_100 {
            o.sample();
        }
        assert!(o.sample().abs() < 0.01);
    }

    #[test]
    fn timer_detection() {
        let mut o = Opl2::new(44_100.0);
        let hz = 33_000_000;
        o.write_addr(4);
        o.write_data(0x60, 0, hz);
        o.write_data(0x80, 0, hz);
        assert_eq!(o.read_status(0) & 0xE0, 0);
        o.write_addr(2);
        o.write_data(0xFF, 0, hz);
        o.write_addr(4);
        o.write_data(0x21, 1000, hz);
        assert_eq!(o.read_status(1000 + hz / 10_000) & 0xE0, 0xC0); // after 100 us
    }
}
