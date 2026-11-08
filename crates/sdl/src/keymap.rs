//! Host keyboard -> PC keyboard: set-1 scancodes, ASCII and BIOS shift flags.

use rusty486_core::devices::input::shift;
use sdl2::keyboard::{Keycode, Scancode};

/// A PC key: set-1 scancode and whether it's an E0-extended key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcKey {
    pub code: u8,
    pub ext: bool,
}

/// US-layout (unshifted, shifted) characters for the main key block, by scancode.
fn us_chars(code: u8) -> Option<(u8, u8)> {
    const ROWS: &[(u8, &[u8], &[u8])] = &[
        (0x02, b"1234567890-=", b"!@#$%^&*()_+"),
        (0x10, b"qwertyuiop[]", b"QWERTYUIOP{}"),
        (0x1E, b"asdfghjkl;'`", b"ASDFGHJKL:\"~"),
        (0x2B, b"\\zxcvbnm,./", b"|ZXCVBNM<>?"),
    ];
    for (start, lo, hi) in ROWS {
        if code >= *start && ((code - start) as usize) < lo.len() {
            let i = (code - start) as usize;
            return Some((lo[i], hi[i]));
        }
    }
    match code {
        0x39 => Some((b' ', b' ')),
        _ => None,
    }
}

pub fn pc_key(sc: Scancode) -> Option<PcKey> {
    use Scancode::*;
    let k = |code| Some(PcKey { code, ext: false });
    let e = |code| Some(PcKey { code, ext: true });
    match sc {
        Escape => k(0x01),
        Num1 => k(0x02),
        Num2 => k(0x03),
        Num3 => k(0x04),
        Num4 => k(0x05),
        Num5 => k(0x06),
        Num6 => k(0x07),
        Num7 => k(0x08),
        Num8 => k(0x09),
        Num9 => k(0x0A),
        Num0 => k(0x0B),
        Minus => k(0x0C),
        Equals => k(0x0D),
        Backspace => k(0x0E),
        Tab => k(0x0F),
        Q => k(0x10),
        W => k(0x11),
        E => k(0x12),
        R => k(0x13),
        T => k(0x14),
        Y => k(0x15),
        U => k(0x16),
        I => k(0x17),
        O => k(0x18),
        P => k(0x19),
        LeftBracket => k(0x1A),
        RightBracket => k(0x1B),
        Return => k(0x1C),
        LCtrl => k(0x1D),
        A => k(0x1E),
        S => k(0x1F),
        D => k(0x20),
        F => k(0x21),
        G => k(0x22),
        H => k(0x23),
        J => k(0x24),
        K => k(0x25),
        L => k(0x26),
        Semicolon => k(0x27),
        Apostrophe => k(0x28),
        Grave => k(0x29),
        LShift => k(0x2A),
        Backslash | NonUsBackslash => k(0x2B),
        Z => k(0x2C),
        X => k(0x2D),
        C => k(0x2E),
        V => k(0x2F),
        B => k(0x30),
        N => k(0x31),
        M => k(0x32),
        Comma => k(0x33),
        Period => k(0x34),
        Slash => k(0x35),
        RShift => k(0x36),
        KpMultiply => k(0x37),
        LAlt => k(0x38),
        Space => k(0x39),
        CapsLock => k(0x3A),
        F1 => k(0x3B),
        F2 => k(0x3C),
        F3 => k(0x3D),
        F4 => k(0x3E),
        F5 => k(0x3F),
        F6 => k(0x40),
        F7 => k(0x41),
        F8 => k(0x42),
        F9 => k(0x43),
        F10 => k(0x44),
        NumLockClear => k(0x45),
        ScrollLock => k(0x46),
        Kp7 => k(0x47),
        Kp8 => k(0x48),
        Kp9 => k(0x49),
        KpMinus => k(0x4A),
        Kp4 => k(0x4B),
        Kp5 => k(0x4C),
        Kp6 => k(0x4D),
        KpPlus => k(0x4E),
        Kp1 => k(0x4F),
        Kp2 => k(0x50),
        Kp3 => k(0x51),
        Kp0 => k(0x52),
        KpPeriod => k(0x53),
        F11 => k(0x57),
        F12 => k(0x58),
        KpEnter => e(0x1C),
        RCtrl => e(0x1D),
        KpDivide => e(0x35),
        PrintScreen => e(0x37),
        RAlt => e(0x38),
        Home => e(0x47),
        Up => e(0x48),
        PageUp => e(0x49),
        Left => e(0x4B),
        Right => e(0x4D),
        End => e(0x4F),
        Down => e(0x50),
        PageDown => e(0x51),
        Insert => e(0x52),
        Delete => e(0x53),
        LGui => e(0x5B),
        RGui => e(0x5C),
        Application => e(0x5D),
        _ => None,
    }
}

/// Tracks shift/lock state and produces the ASCII byte for key presses.
#[derive(Default)]
pub struct KeyState {
    pub flags: u8,
}

impl KeyState {
    /// Update flags for a key event. Returns true if it's a modifier key.
    pub fn update(&mut self, key: PcKey, down: bool) -> bool {
        let set = |flags: &mut u8, bit: u8, on: bool| {
            if on {
                *flags |= bit
            } else {
                *flags &= !bit
            }
        };
        match (key.code, key.ext) {
            (0x2A, false) => set(&mut self.flags, shift::LSHIFT, down),
            (0x36, false) => set(&mut self.flags, shift::RSHIFT, down),
            (0x1D, _) => set(&mut self.flags, shift::CTRL, down),
            (0x38, _) => set(&mut self.flags, shift::ALT, down),
            (0x3A, false) => {
                if down {
                    self.flags ^= shift::CAPS;
                }
            }
            (0x45, false) => {
                if down {
                    self.flags ^= shift::NUM;
                }
            }
            (0x46, false) => {
                if down {
                    self.flags ^= shift::SCROLL;
                }
            }
            (0x52, _) => {
                if down {
                    self.flags ^= shift::INSERT;
                }
                return false;
            }
            (0x5B | 0x5C | 0x5D, true) => {}
            _ => return false,
        }
        true
    }

    /// The ASCII code a key press produces given the current flags.
    /// `layout_char` is the host layout's unshifted character for letter keys.
    pub fn ascii(&self, key: PcKey, layout_char: Option<char>) -> u8 {
        let shifted = self.flags & (shift::LSHIFT | shift::RSHIFT) != 0;
        let ctrl = self.flags & shift::CTRL != 0;
        let alt = self.flags & shift::ALT != 0;
        if alt {
            return 0;
        }
        if key.ext {
            return match key.code {
                0x1C => 0x0D,
                0x35 => b'/',
                _ => 0,
            };
        }
        match key.code {
            0x01 => return 0x1B,
            0x0E => return 0x08,
            0x0F => return if shifted { 0 } else { 0x09 },
            0x1C => return if ctrl { 0x0A } else { 0x0D },
            0x37 => return b'*',
            0x4A => return b'-',
            0x4E => return b'+',
            0x47..=0x53 => {
                return if self.flags & shift::NUM != 0 && !shifted {
                    b"789-456+1230."[(key.code - 0x47) as usize]
                } else {
                    0
                };
            }
            _ => {}
        }
        let Some((lo, hi)) = us_chars(key.code) else { return 0 };
        let mut c = if shifted { hi } else { lo };
        if lo.is_ascii_lowercase() {
            // Letters follow the host layout (e.g. QWERTZ) and Caps Lock.
            let base = layout_char.filter(|c| c.is_ascii_lowercase()).map_or(lo, |c| c as u8);
            let upper = shifted ^ (self.flags & shift::CAPS != 0);
            c = if upper { base.to_ascii_uppercase() } else { base };
            if ctrl {
                return base - b'a' + 1;
            }
        } else if ctrl {
            return match c {
                b'[' | b'{' => 0x1B,
                b'\\' | b'|' => 0x1C,
                b']' | b'}' => 0x1D,
                b'-' | b'_' => 0x1F,
                _ => 0,
            };
        }
        c
    }
}

/// The host layout's character for a keycode (letters only matter).
pub fn layout_char(kc: Option<Keycode>) -> Option<char> {
    let name = kc?.name();
    let mut chars = name.chars();
    let c = chars.next()?;
    (chars.next().is_none() && c.is_ascii_alphabetic()).then(|| c.to_ascii_lowercase())
}

/// Key needed to type an ASCII character on a US layout: (key, shift held).
pub fn key_for_char(c: char) -> Option<(PcKey, bool)> {
    match c {
        '\n' | '\r' => return Some((PcKey { code: 0x1C, ext: false }, false)),
        '\t' => return Some((PcKey { code: 0x0F, ext: false }, false)),
        '\x08' => return Some((PcKey { code: 0x0E, ext: false }, false)),
        '\x1b' => return Some((PcKey { code: 0x01, ext: false }, false)),
        _ => {}
    }
    let b = u8::try_from(c).ok()?;
    for code in 0x02..=0x39u8 {
        if let Some((lo, hi)) = us_chars(code) {
            if lo == b {
                return Some((PcKey { code, ext: false }, false));
            }
            if hi == b {
                return Some((PcKey { code, ext: false }, true));
            }
        }
    }
    None
}

/// Named keys for MCP: "enter", "esc", "f1", "up", "ctrl", "a"...
pub fn key_by_name(name: &str) -> Option<PcKey> {
    let n = name.to_ascii_lowercase();
    let k = |code| Some(PcKey { code, ext: false });
    let e = |code| Some(PcKey { code, ext: true });
    if let Some(f) = n.strip_prefix('f').and_then(|d| d.parse::<u8>().ok()) {
        return match f {
            1..=10 => k(0x3A + f),
            11 => k(0x57),
            12 => k(0x58),
            _ => None,
        };
    }
    match n.as_str() {
        "esc" | "escape" => k(0x01),
        "enter" | "return" => k(0x1C),
        "space" => k(0x39),
        "tab" => k(0x0F),
        "backspace" => k(0x0E),
        "ctrl" | "control" => k(0x1D),
        "shift" => k(0x2A),
        "alt" => k(0x38),
        "capslock" => k(0x3A),
        "up" => e(0x48),
        "down" => e(0x50),
        "left" => e(0x4B),
        "right" => e(0x4D),
        "home" => e(0x47),
        "end" => e(0x4F),
        "pageup" | "pgup" => e(0x49),
        "pagedown" | "pgdn" => e(0x51),
        "insert" | "ins" => e(0x52),
        "delete" | "del" => e(0x53),
        s if s.chars().count() == 1 => key_for_char(s.chars().next()?).map(|(k, _)| k),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_mapping() {
        let mut s = KeyState::default();
        let a = PcKey { code: 0x1E, ext: false };
        assert_eq!(s.ascii(a, Some('a')), b'a');
        s.update(PcKey { code: 0x2A, ext: false }, true);
        assert_eq!(s.ascii(a, Some('a')), b'A');
        assert_eq!(s.ascii(PcKey { code: 0x02, ext: false }, None), b'!');
        s.update(PcKey { code: 0x2A, ext: false }, false);
        s.update(PcKey { code: 0x1D, ext: false }, true);
        assert_eq!(s.ascii(PcKey { code: 0x2E, ext: false }, Some('c')), 3);
    }

    #[test]
    fn chars_to_keys() {
        assert_eq!(key_for_char('A'), Some((PcKey { code: 0x1E, ext: false }, true)));
        assert_eq!(key_for_char('?'), Some((PcKey { code: 0x35, ext: false }, true)));
        assert_eq!(key_by_name("F10"), Some(PcKey { code: 0x44, ext: false }));
    }
}
