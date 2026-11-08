//! 80x25 text screen with a back buffer, and Borland-style widgets.

pub const W: usize = 80;
pub const H: usize = 25;

// Colours (background << 4 | foreground).
pub const DESKTOP: u8 = 0x71;
pub const MENU: u8 = 0x70;
pub const MENU_HOT: u8 = 0x74;
pub const MENU_SEL: u8 = 0x20;
pub const MENU_SEL_HOT: u8 = 0x24;
pub const EDIT_TEXT: u8 = 0x1E;
pub const EDIT_FRAME: u8 = 0x1F;
pub const EDIT_FRAME_ICON: u8 = 0x1A;
pub const EDIT_KEYWORD: u8 = 0x1F;
pub const EDIT_COMMENT: u8 = 0x17;
pub const EDIT_STRING: u8 = 0x1B;
pub const EDIT_NUMBER: u8 = 0x1D;
pub const EDIT_SEL: u8 = 0x71;
pub const SCROLL: u8 = 0x31;
pub const MSG_FRAME: u8 = 0x3F;
pub const MSG_TEXT: u8 = 0x30;
pub const MSG_SEL: u8 = 0x2F;
pub const DLG: u8 = 0x70;
pub const BUTTON: u8 = 0x20;
pub const BUTTON_FOCUS: u8 = 0x2F;
pub const BUTTON_HOT: u8 = 0x2E;
pub const INPUT: u8 = 0x1F;
pub const LIST: u8 = 0x30;
pub const LIST_SEL: u8 = 0x2F;
pub const SHADOW: u8 = 0x08;
pub const ERROR_DLG: u8 = 0x4F;

pub struct Screen {
    buf: [u16; W * H],
}

impl Screen {
    pub const fn new() -> Screen {
        Screen { buf: [0x0720; W * H] }
    }

    pub fn put(&mut self, x: usize, y: usize, c: u8, attr: u8) {
        if x < W && y < H {
            self.buf[y * W + x] = (attr as u16) << 8 | c as u16;
        }
    }

    pub fn attr_at(&mut self, x: usize, y: usize, attr: u8) {
        if x < W && y < H {
            let c = self.buf[y * W + x] & 0xFF;
            self.buf[y * W + x] = (attr as u16) << 8 | c;
        }
    }

    pub fn text(&mut self, x: usize, y: usize, s: &str, attr: u8) -> usize {
        let mut i = 0;
        for &c in s.as_bytes() {
            self.put(x + i, y, c, attr);
            i += 1;
        }
        x + i
    }

    /// Text where `~` toggles the highlight colour (Borland hotkeys: "~F~ile").
    pub fn hot_text(&mut self, x: usize, y: usize, s: &str, attr: u8, hot: u8) -> usize {
        let mut i = 0;
        let mut on = false;
        for &c in s.as_bytes() {
            if c == b'~' {
                on = !on;
                continue;
            }
            self.put(x + i, y, c, if on { hot } else { attr });
            i += 1;
        }
        x + i
    }

    pub fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, c: u8, attr: u8) {
        for j in y..y + h {
            for i in x..x + w {
                self.put(i, j, c, attr);
            }
        }
    }

    /// A frame (double or single lines) filling its inside with spaces.
    pub fn frame(&mut self, x: usize, y: usize, w: usize, h: usize, attr: u8, double: bool, fill: Option<u8>) {
        let (tl, tr, bl, br, hz, vt) = if double {
            (0xC9, 0xBB, 0xC8, 0xBC, 0xCD, 0xBA)
        } else {
            (0xDA, 0xBF, 0xC0, 0xD9, 0xC4, 0xB3)
        };
        if let Some(a) = fill {
            self.fill(x + 1, y + 1, w - 2, h - 2, b' ', a);
        }
        self.put(x, y, tl, attr);
        self.put(x + w - 1, y, tr, attr);
        self.put(x, y + h - 1, bl, attr);
        self.put(x + w - 1, y + h - 1, br, attr);
        for i in x + 1..x + w - 1 {
            self.put(i, y, hz, attr);
            self.put(i, y + h - 1, hz, attr);
        }
        for j in y + 1..y + h - 1 {
            self.put(x, j, vt, attr);
            self.put(x + w - 1, j, vt, attr);
        }
    }

    /// Darken the cells right of and below a box.
    pub fn shadow(&mut self, x: usize, y: usize, w: usize, h: usize) {
        for j in y + 1..=y + h {
            self.attr_at(x + w, j, SHADOW);
            self.attr_at(x + w + 1, j, SHADOW);
        }
        for i in x + 2..x + w + 2 {
            self.attr_at(i, y + h, SHADOW);
        }
    }

    /// A dialog box with a title and a shadow.
    pub fn dialog(&mut self, x: usize, y: usize, w: usize, h: usize, title: &str, attr: u8) {
        self.frame(x, y, w, h, attr | 0x0F, true, Some(attr));
        self.put(x + 3, y, b'[', attr | 0x0F);
        self.put(x + 4, y, 0xFE, (attr & 0xF0) | 0x0A);
        self.put(x + 5, y, b']', attr | 0x0F);
        let t = title.len() + 2;
        let tx = x + (w.saturating_sub(t)) / 2;
        self.put(tx, y, b' ', attr | 0x0F);
        self.text(tx + 1, y, title, attr | 0x0F);
        self.put(tx + t - 1, y, b' ', attr | 0x0F);
        self.shadow(x, y, w, h);
    }

    /// A Borland button: green with a black shadow.
    pub fn button(&mut self, x: usize, y: usize, label: &str, focused: bool) {
        let w = label.len() + 2;
        let attr = if focused { BUTTON_FOCUS } else { BUTTON };
        self.fill(x, y, w, 1, b' ', attr);
        self.hot_text(x + 1, y, label, attr, if focused { BUTTON_FOCUS } else { BUTTON_HOT });
        // Shadow.
        let bg = DLG & 0xF0;
        self.put(x + w, y, 0xDC, bg);
        for i in x + 1..=x + w {
            self.put(i, y + 1, 0xDF, bg);
        }
    }

    /// Copy the back buffer to text video memory.
    pub fn flush(&self) {
        let vram = 0xB8000 as *mut u16;
        for (i, &c) in self.buf.iter().enumerate() {
            unsafe { core::ptr::write_volatile(vram.add(i), c) };
        }
    }

    /// Snapshot of the visible text screen (for the user screen).
    pub fn grab() -> [u16; W * H] {
        let mut s = [0u16; W * H];
        let vram = 0xB8000 as *const u16;
        for (i, c) in s.iter_mut().enumerate() {
            *c = unsafe { core::ptr::read_volatile(vram.add(i)) };
        }
        s
    }

    pub fn show(cells: &[u16; W * H]) {
        let vram = 0xB8000 as *mut u16;
        for (i, &c) in cells.iter().enumerate() {
            unsafe { core::ptr::write_volatile(vram.add(i), c) };
        }
    }
}
