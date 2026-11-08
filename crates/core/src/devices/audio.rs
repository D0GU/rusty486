//! Audio mixing: OPL2 FM, Sound Blaster DAC and PC speaker into a 44.1 kHz
//! mono stream. The bus "catches up" the audio to the current CPU cycle
//! before any sound register changes, so timing is sample-accurate.

pub const RATE: u32 = 44_100;
/// Keep at most this many unread samples (the frontend drains them).
const MAX_BUFFER: usize = RATE as usize * 2;

#[derive(Clone, Debug)]
pub struct Audio {
    /// Generated samples waiting for the frontend.
    pub out: Vec<i16>,
    /// CPU cycle of the next output sample.
    pub next_sample: f64,
    /// CPU cycle of the next Sound Blaster PCM sample.
    pub sb_next: f64,
    spk_phase: f32,
    /// Master volume (0.0-1.0).
    pub volume: f32,
}

impl Default for Audio {
    fn default() -> Self {
        Audio { out: Vec::new(), next_sample: 0.0, sb_next: 0.0, spk_phase: 0.0, volume: 0.8 }
    }
}

impl Audio {
    /// Mix one output sample from the sources' current levels.
    pub fn mix(&mut self, opl: f32, sb_dac: Option<u8>, speaker_tone: Option<f64>, speaker_level: Option<bool>) {
        let mut s = opl * 0.6;
        if let Some(d) = sb_dac {
            s += (d as f32 - 128.0) / 128.0 * 0.6;
        }
        if let Some(f) = speaker_tone {
            self.spk_phase = (self.spk_phase + f as f32 / RATE as f32).fract();
            s += if self.spk_phase < 0.5 { 0.15 } else { -0.15 };
        } else if let Some(level) = speaker_level {
            s += if level { 0.15 } else { -0.15 };
        }
        let v = (s * self.volume).clamp(-1.0, 1.0);
        if self.out.len() >= MAX_BUFFER {
            self.out.drain(..RATE as usize / 10);
        }
        self.out.push((v * 32767.0) as i16);
    }
}
