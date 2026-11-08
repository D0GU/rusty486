//! The rusty486 input device (custom): keyboard and mouse events in a FIFO,
//! raising IRQ1 when events arrive, plus live key and mouse state ports.
//!
//! | Port  | R/W | Meaning                                                              |
//! |-------|-----|----------------------------------------------------------------------|
//! | 320h  | R   | Number of queued events (saturates at 255)                           |
//! | 321h  | R   | Head event type: 0 none, 1 key down, 2 key up, 3 mouse move, 4 mouse buttons |
//! | 321h  | W   | Pop the head event                                                   |
//! | 322h  | R   | Key: scancode (set 1).  Mouse: button bits (1 left, 2 right, 4 middle) |
//! | 323h  | R   | Key: ASCII character (0 if none).  Mouse: X movement (signed)        |
//! | 324h  | R   | Key: BIOS shift flags after the event.  Mouse: Y movement (signed)   |
//! | 325h  | R   | Key: bit 0 = extended (E0) key, bit 1 = modifier key                 |
//! | 326h  | R/W | Bit 0: raise IRQ1 when events arrive                                 |
//! | 328h  | W   | Select a scancode for 329h (bit 7 = extended)                        |
//! | 329h  | R   | 1 if the selected key is held down                                   |
//! | 32Ah  | R   | Mouse X (0-639), low/high at 32Ah/32Bh                               |
//! | 32Ch  | R   | Mouse Y (0-399), low/high at 32Ch/32Dh                               |
//! | 32Eh  | R   | Mouse buttons held                                                   |

use std::collections::VecDeque;

pub const BASE_PORT: u16 = 0x320;
const FIFO_LIMIT: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub kind: u8,
    pub a: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
}

/// BIOS shift-flag bits (BDA 417h).
pub mod shift {
    pub const RSHIFT: u8 = 0x01;
    pub const LSHIFT: u8 = 0x02;
    pub const CTRL: u8 = 0x04;
    pub const ALT: u8 = 0x08;
    pub const SCROLL: u8 = 0x10;
    pub const NUM: u8 = 0x20;
    pub const CAPS: u8 = 0x40;
    pub const INSERT: u8 = 0x80;
}

#[derive(Clone, Debug)]
pub struct Input {
    fifo: VecDeque<Event>,
    pub irq_enabled: bool,
    /// Keys held, indexed by scancode | 0x80 for extended keys.
    held: [bool; 256],
    select: u8,
    pub mouse_x: u16,
    pub mouse_y: u16,
    pub buttons: u8,
}

impl Default for Input {
    fn default() -> Self {
        Input {
            fifo: VecDeque::new(),
            irq_enabled: false,
            held: [false; 256],
            select: 0,
            mouse_x: 320,
            mouse_y: 200,
            buttons: 0,
        }
    }
}

impl Input {
    pub fn handles(port: u16) -> bool {
        (BASE_PORT..BASE_PORT + 0x10).contains(&port)
    }

    /// Queue an event. Returns true if IRQ1 should be raised.
    fn push(&mut self, e: Event) -> bool {
        if self.fifo.len() < FIFO_LIMIT {
            self.fifo.push_back(e);
        }
        self.irq_enabled
    }

    /// A key went down or up. `flags` are the BIOS shift flags after the event.
    pub fn key(&mut self, scancode: u8, extended: bool, down: bool, ascii: u8, flags: u8, modifier: bool) -> bool {
        self.held[(scancode & 0x7F | if extended { 0x80 } else { 0 }) as usize] = down;
        let ext = extended as u8 | (modifier as u8) << 1;
        self.push(Event { kind: if down { 1 } else { 2 }, a: scancode, b: ascii, c: flags, d: ext })
    }

    /// Mouse moved to an absolute position (in 640x400 screen pixels).
    pub fn mouse_move(&mut self, x: u16, y: u16) -> bool {
        let dx = (x as i32 - self.mouse_x as i32).clamp(-128, 127) as i8 as u8;
        let dy = (y as i32 - self.mouse_y as i32).clamp(-128, 127) as i8 as u8;
        self.mouse_x = x.min(639);
        self.mouse_y = y.min(399);
        self.push(Event { kind: 3, a: self.buttons, b: dx, c: dy, d: 0 })
    }

    pub fn mouse_buttons(&mut self, buttons: u8) -> bool {
        self.buttons = buttons;
        self.push(Event { kind: 4, a: buttons, b: 0, c: 0, d: 0 })
    }

    pub fn pending(&self) -> usize {
        self.fifo.len()
    }

    pub fn read(&self, port: u16) -> u8 {
        let head = self.fifo.front().copied().unwrap_or(Event { kind: 0, a: 0, b: 0, c: 0, d: 0 });
        match port - BASE_PORT {
            0x0 => self.fifo.len().min(255) as u8,
            0x1 => head.kind,
            0x2 => head.a,
            0x3 => head.b,
            0x4 => head.c,
            0x5 => head.d,
            0x6 => self.irq_enabled as u8,
            0x8 => self.select,
            0x9 => self.held[self.select as usize] as u8,
            0xA => self.mouse_x as u8,
            0xB => (self.mouse_x >> 8) as u8,
            0xC => self.mouse_y as u8,
            0xD => (self.mouse_y >> 8) as u8,
            0xE => self.buttons,
            _ => 0xFF,
        }
    }

    /// Port write. Returns true if IRQ1 should be raised (events still queued
    /// when interrupts are switched on).
    pub fn write(&mut self, port: u16, v: u8) -> bool {
        match port - BASE_PORT {
            0x1 => {
                self.fifo.pop_front();
                false
            }
            0x6 => {
                self.irq_enabled = v & 1 != 0;
                self.irq_enabled && !self.fifo.is_empty()
            }
            0x8 => {
                self.select = v;
                false
            }
            _ => false,
        }
    }
}
