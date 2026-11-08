//! The rusty486 video card. Milestone 1 provides 80x25 colour text mode at
//! B8000h plus the standard CRTC cursor registers at 3D4h/3D5h. Graphics modes
//! and the VBE interface come later.

pub const TEXT_BASE: usize = 0xB8000;
pub const TEXT_COLS: usize = 80;
pub const TEXT_ROWS: usize = 25;

#[derive(Debug)]
pub struct Video {
    crtc_index: u8,
    crtc: [u8; 0x19],
}

impl Default for Video {
    fn default() -> Self {
        let mut crtc = [0; 0x19];
        crtc[0x0A] = 0x0E; // cursor start scanline
        crtc[0x0B] = 0x0F; // cursor end scanline
        Video { crtc_index: 0, crtc }
    }
}

impl Video {
    pub fn handles(port: u16) -> bool {
        matches!(port, 0x3D4 | 0x3D5)
    }

    pub fn read(&self, port: u16) -> u8 {
        match port {
            0x3D4 => self.crtc_index,
            _ => self.crtc.get(self.crtc_index as usize).copied().unwrap_or(0xFF),
        }
    }

    pub fn write(&mut self, port: u16, v: u8) {
        match port {
            0x3D4 => self.crtc_index = v,
            _ => {
                if let Some(r) = self.crtc.get_mut(self.crtc_index as usize) {
                    *r = v;
                }
            }
        }
    }

    /// Cursor position as a character offset (row * 80 + col).
    pub fn cursor_offset(&self) -> u16 {
        (self.crtc[0x0E] as u16) << 8 | self.crtc[0x0F] as u16
    }

    /// Cursor is hidden when bit 5 of the cursor-start register is set.
    pub fn cursor_visible(&self) -> bool {
        self.crtc[0x0A] & 0x20 == 0
    }

    /// Cursor scanline range (start, end) within the 16-line character cell.
    pub fn cursor_shape(&self) -> (u8, u8) {
        (self.crtc[0x0A] & 0x1F, self.crtc[0x0B] & 0x1F)
    }
}
