//! MC146818-style real-time clock and CMOS RAM at ports 70h/71h. The time
//! comes from the host (set by the frontend) and advances with emulated time.

#[derive(Clone, Debug)]
pub struct Rtc {
    index: u8,
    ram: [u8; 128],
    /// Host time (seconds since the Unix epoch, local time) at `base_cycle`.
    base_secs: i64,
    base_cycle: u64,
}

impl Default for Rtc {
    fn default() -> Self {
        let mut ram = [0u8; 128];
        ram[0x0A] = 0x26; // 32.768 kHz, 1024 Hz
        ram[0x0B] = 0x02; // 24-hour, BCD
        ram[0x0D] = 0x80; // battery good
        ram[0x10] = 0x40; // floppy 0: 1.44 MB
        ram[0x14] = 0x05; // equipment: floppy, colour 80x25
        ram[0x15] = 0x80; // 640 KB base memory
        ram[0x16] = 0x02;
        // Extended memory above 1 MB in KB (at most 65535), and 64 KB blocks
        // above 16 MB (what INT 15h AX=E801h reports).
        let ext_kb = (crate::machine::RAM_SIZE / 1024 - 1024).min(0xFFFF) as u16;
        let blocks = (crate::machine::RAM_SIZE.saturating_sub(16 << 20) >> 16) as u16;
        for (lo, hi, v) in [(0x17, 0x18, ext_kb), (0x30, 0x31, ext_kb), (0x34, 0x35, blocks)] {
            ram[lo] = v as u8;
            ram[hi] = (v >> 8) as u8;
        }
        // 2026-01-01 00:00:00 until the frontend sets the time.
        Rtc { index: 0, ram, base_secs: 1_767_225_600, base_cycle: 0 }
    }
}

fn bcd(v: i64) -> u8 {
    ((v / 10) << 4 | (v % 10)) as u8
}

/// Days since 1970-01-01 to (year, month, day). (Howard Hinnant's algorithm.)
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

impl Rtc {
    pub fn handles(port: u16) -> bool {
        port == 0x70 || port == 0x71
    }

    /// Set the current local time (seconds since 1970) as of CPU cycle `cycle`.
    pub fn set_time(&mut self, secs: i64, cycle: u64) {
        self.base_secs = secs;
        self.base_cycle = cycle;
    }

    pub fn now(&self, cycle: u64, clock_hz: u64) -> i64 {
        self.base_secs + (cycle.saturating_sub(self.base_cycle) / clock_hz.max(1)) as i64
    }

    pub fn read(&mut self, port: u16, cycle: u64, clock_hz: u64) -> u8 {
        if port == 0x70 {
            return 0xFF;
        }
        let t = self.now(cycle, clock_hz);
        let (secs, days) = (t.rem_euclid(86_400), t.div_euclid(86_400));
        let (y, m, d) = civil(days);
        match self.index {
            0x00 => bcd(secs % 60),
            0x02 => bcd(secs / 60 % 60),
            0x04 => bcd(secs / 3600),
            0x06 => bcd((days + 4).rem_euclid(7) + 1),
            0x07 => bcd(d),
            0x08 => bcd(m),
            0x09 => bcd(y % 100),
            0x32 => bcd(y / 100),
            0x0C => {
                let v = self.ram[0x0C];
                self.ram[0x0C] = 0;
                v
            }
            i => self.ram[i as usize],
        }
    }

    pub fn write(&mut self, port: u16, v: u8) {
        if port == 0x70 {
            self.index = v & 0x7F;
        } else if !matches!(self.index, 0x00..=0x09 | 0x32 | 0x0C | 0x0D) {
            self.ram[self.index as usize] = v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_date() {
        let mut r = Rtc::default();
        r.set_time(1_794_009_600 + 3 * 3600 + 25 * 60 + 9, 0); // 2026-11-07 03:25:09
        let mut rd = |i: u8| {
            r.write(0x70, i);
            r.read(0x71, 0, 1)
        };
        assert_eq!((rd(9), rd(8), rd(7)), (0x26, 0x11, 0x07));
        assert_eq!((rd(4), rd(2), rd(0)), (0x03, 0x25, 0x09));
        assert_eq!(rd(0x32), 0x20);
    }
}
