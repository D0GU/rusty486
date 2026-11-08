//! The rusty486 video card.
//!
//! - 80x25 colour text mode at B8000 with the CRTC cursor registers at 3D4/3D5.
//! - Graphics through Bochs-compatible VBE "DISPI" registers (index 1CEh,
//!   data 1CFh): resolutions up to 640x480 at 8 (palette), 15/16 (RGB) and
//!   32 bits per pixel. VRAM (2 MB) is visible through a 64 KB window at
//!   A0000h (bank-switched) and as a linear framebuffer at E0000000h.
//! - A 256-colour DAC (3C7h-3C9h, 6-bit components) for 8-bit modes.
//! - Input status register 3DAh with vertical/horizontal retrace bits.

pub const TEXT_BASE: usize = 0xB8000;
pub const TEXT_COLS: usize = 80;
pub const TEXT_ROWS: usize = 25;

pub const VRAM_SIZE: usize = 2 * 1024 * 1024;
pub const LFB_BASE: u32 = 0xE000_0000;
pub const WINDOW_BASE: u32 = 0xA0000;
pub const MAX_WIDTH: u16 = 640;
pub const MAX_HEIGHT: u16 = 480;

// DISPI register indices.
pub const DISPI_ID: u16 = 0;
pub const DISPI_XRES: u16 = 1;
pub const DISPI_YRES: u16 = 2;
pub const DISPI_BPP: u16 = 3;
pub const DISPI_ENABLE: u16 = 4;
pub const DISPI_BANK: u16 = 5;
pub const DISPI_VIRT_WIDTH: u16 = 6;
pub const DISPI_VIRT_HEIGHT: u16 = 7;
pub const DISPI_X_OFFSET: u16 = 8;
pub const DISPI_Y_OFFSET: u16 = 9;
pub const DISPI_VIDEO_MEMORY_64K: u16 = 0xA;

pub const ENABLED: u16 = 0x01;
pub const LFB_ENABLED: u16 = 0x40;
pub const NOCLEARMEM: u16 = 0x80;

#[derive(Debug)]
pub struct Video {
    crtc_index: u8,
    crtc: [u8; 0x19],
    dispi_index: u16,
    pub xres: u16,
    pub yres: u16,
    pub bpp: u16,
    pub enable: u16,
    pub bank: u16,
    pub virt_width: u16,
    pub virt_height: u16,
    pub x_offset: u16,
    pub y_offset: u16,
    pub vram: Vec<u8>,
    /// 256 entries of 6-bit R, G, B.
    pub dac: [u8; 768],
    dac_read: u16,
    dac_write: u16,
    /// Bumped whenever the picture may have changed (for screenshot caching).
    pub generation: u64,
}

impl Default for Video {
    fn default() -> Self {
        let mut crtc = [0; 0x19];
        crtc[0x0A] = 0x0E; // cursor start scanline
        crtc[0x0B] = 0x0F; // cursor end scanline
        Video {
            crtc_index: 0,
            crtc,
            dispi_index: 0,
            xres: 640,
            yres: 480,
            bpp: 8,
            enable: 0,
            bank: 0,
            virt_width: 640,
            virt_height: 480,
            x_offset: 0,
            y_offset: 0,
            vram: vec![0; VRAM_SIZE],
            dac: default_palette(),
            dac_read: 0,
            dac_write: 0,
            generation: 0,
        }
    }
}

/// The default 256-colour palette: 16 EGA colours, 16 greys, a 6x6x6 colour
/// cube and 8 blacks (6-bit components).
pub fn default_palette() -> [u8; 768] {
    let mut p = [0u8; 768];
    const EGA: [(u8, u8, u8); 16] = [
        (0, 0, 0), (0, 0, 42), (0, 42, 0), (0, 42, 42), (42, 0, 0), (42, 0, 42), (42, 21, 0), (42, 42, 42),
        (21, 21, 21), (21, 21, 63), (21, 63, 21), (21, 63, 63), (63, 21, 21), (63, 21, 63), (63, 63, 21), (63, 63, 63),
    ];
    for (i, (r, g, b)) in EGA.iter().enumerate() {
        p[i * 3..i * 3 + 3].copy_from_slice(&[*r, *g, *b]);
    }
    for i in 0..16 {
        let v = (i * 63 / 15) as u8;
        p[(16 + i) * 3..(16 + i) * 3 + 3].copy_from_slice(&[v, v, v]);
    }
    let mut n = 32;
    for r in 0..6 {
        for g in 0..6 {
            for b in 0..6 {
                p[n * 3..n * 3 + 3].copy_from_slice(&[r * 63 / 5, g * 63 / 5, b * 63 / 5]);
                n += 1;
            }
        }
    }
    p
}

impl Video {
    pub fn handles(port: u16) -> bool {
        matches!(port, 0x3D4 | 0x3D5 | 0x3C7 | 0x3C8 | 0x3C9 | 0x3DA | 0x1CE | 0x1CF | 0x3C0..=0x3CF | 0x3B4 | 0x3B5 | 0x3BA)
    }

    /// Graphics mode active (the DISPI enable bit).
    pub fn graphics(&self) -> bool {
        self.enable & ENABLED != 0
    }

    /// Bytes per scanline in VRAM.
    pub fn pitch(&self) -> usize {
        self.virt_width.max(self.xres) as usize * self.bytes_per_pixel()
    }

    pub fn bytes_per_pixel(&self) -> usize {
        (self.bpp as usize).div_ceil(8)
    }

    /// Offset in VRAM of the first visible pixel.
    pub fn display_start(&self) -> usize {
        self.y_offset as usize * self.pitch() + self.x_offset as usize * self.bytes_per_pixel()
    }

    pub fn read(&mut self, port: u16, now: u64, clock_hz: u64) -> u8 {
        match port {
            0x3D4 => self.crtc_index,
            0x3D5 => self.crtc.get(self.crtc_index as usize).copied().unwrap_or(0xFF),
            0x3DA | 0x3BA => {
                // 70 Hz frames; vertical retrace for the last ~5% of each.
                let frame = (clock_hz / 70).max(1);
                let pos = now % frame;
                let vretrace = pos >= frame - frame / 20;
                let line = (frame / 525).max(1);
                let hretrace = (pos % line) >= line * 4 / 5;
                (vretrace as u8) << 3 | (hretrace || vretrace) as u8
            }
            0x3C7 => 3, // DAC state: read mode
            0x3C8 => (self.dac_write / 3) as u8,
            0x3C9 => {
                let v = self.dac[self.dac_read as usize % 768];
                self.dac_read = (self.dac_read + 1) % 768;
                v
            }
            0x1CE => self.dispi_index as u8,
            0x1CF => self.dispi_read() as u8,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, port: u16, v: u8) {
        match port {
            0x3D4 => self.crtc_index = v,
            0x3D5 => {
                if let Some(r) = self.crtc.get_mut(self.crtc_index as usize) {
                    *r = v;
                }
            }
            0x3C7 => self.dac_read = v as u16 * 3,
            0x3C8 => self.dac_write = v as u16 * 3,
            0x3C9 => {
                self.dac[self.dac_write as usize % 768] = v & 0x3F;
                self.dac_write = (self.dac_write + 1) % 768;
                self.generation += 1;
            }
            0x1CE => self.dispi_index = v as u16,
            0x1CF => self.dispi_write(v as u16),
            _ => {} // other VGA registers are accepted and ignored
        }
    }

    /// 16-bit access to the DISPI index/data ports.
    pub fn read16(&mut self, port: u16) -> u16 {
        match port {
            0x1CE => self.dispi_index,
            _ => self.dispi_read(),
        }
    }

    pub fn write16(&mut self, port: u16, v: u16) {
        match port {
            0x1CE => self.dispi_index = v,
            _ => self.dispi_write(v),
        }
    }

    fn dispi_read(&self) -> u16 {
        match self.dispi_index {
            DISPI_ID => 0xB0C5,
            DISPI_XRES => self.xres,
            DISPI_YRES => self.yres,
            DISPI_BPP => self.bpp,
            DISPI_ENABLE => self.enable,
            DISPI_BANK => self.bank,
            DISPI_VIRT_WIDTH => self.virt_width,
            DISPI_VIRT_HEIGHT => self.virt_height,
            DISPI_X_OFFSET => self.x_offset,
            DISPI_Y_OFFSET => self.y_offset,
            DISPI_VIDEO_MEMORY_64K => (VRAM_SIZE / 65536) as u16,
            _ => 0,
        }
    }

    fn dispi_write(&mut self, v: u16) {
        self.generation += 1;
        match self.dispi_index {
            DISPI_XRES => self.xres = v.clamp(1, MAX_WIDTH),
            DISPI_YRES => self.yres = v.clamp(1, MAX_HEIGHT),
            DISPI_BPP => self.bpp = if matches!(v, 8 | 15 | 16 | 24 | 32) { v } else { 8 },
            DISPI_ENABLE => {
                let was = self.enable & ENABLED != 0;
                self.enable = v;
                if v & ENABLED != 0 && !was {
                    self.virt_width = self.xres;
                    self.virt_height = self.yres;
                    self.x_offset = 0;
                    self.y_offset = 0;
                    self.bank = 0;
                    if v & NOCLEARMEM == 0 {
                        self.vram.fill(0);
                    }
                }
            }
            DISPI_BANK => self.bank = v,
            DISPI_VIRT_WIDTH => self.virt_width = v.max(self.xres),
            DISPI_VIRT_HEIGHT => self.virt_height = v,
            DISPI_X_OFFSET => self.x_offset = v,
            DISPI_Y_OFFSET => self.y_offset = v,
            _ => {}
        }
    }

    /// VRAM offset for an address in the A0000-AFFFF window.
    pub fn window_offset(&self, addr: u32) -> usize {
        (self.bank as usize * 65536 + (addr - WINDOW_BASE) as usize) % VRAM_SIZE
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

    /// Render the current graphics frame as 0xRRGGBB pixels (width, height).
    pub fn render_graphics(&self, out: &mut Vec<u32>) -> (usize, usize) {
        let (w, h) = (self.xres as usize, self.yres as usize);
        out.resize(w * h, 0);
        let pitch = self.pitch();
        let start = self.display_start();
        let bpp = self.bytes_per_pixel();
        let pal: Vec<u32> = (0..256)
            .map(|i| {
                let c = |k: usize| (self.dac[i * 3 + k] as u32 * 255 / 63) & 0xFF;
                c(0) << 16 | c(1) << 8 | c(2)
            })
            .collect();
        let vram = &self.vram;
        for y in 0..h {
            let row = start + y * pitch;
            for x in 0..w {
                let o = (row + x * bpp) % VRAM_SIZE;
                out[y * w + x] = match self.bpp {
                    8 => pal[vram[o] as usize],
                    15 => {
                        let p = u16::from_le_bytes([vram[o], vram[(o + 1) % VRAM_SIZE]]) as u32;
                        let (r, g, b) = ((p >> 10) & 31, (p >> 5) & 31, p & 31);
                        (r * 255 / 31) << 16 | (g * 255 / 31) << 8 | (b * 255 / 31)
                    }
                    16 => {
                        let p = u16::from_le_bytes([vram[o], vram[(o + 1) % VRAM_SIZE]]) as u32;
                        let (r, g, b) = ((p >> 11) & 31, (p >> 5) & 63, p & 31);
                        (r * 255 / 31) << 16 | (g * 255 / 63) << 8 | (b * 255 / 31)
                    }
                    24 => (vram[(o + 2) % VRAM_SIZE] as u32) << 16 | (vram[(o + 1) % VRAM_SIZE] as u32) << 8 | vram[o] as u32,
                    _ => u32::from_le_bytes([vram[o], vram[(o + 1) % VRAM_SIZE], vram[(o + 2) % VRAM_SIZE], 0]),
                };
            }
        }
        (w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispi_mode_and_window() {
        let mut v = Video::default();
        v.write16(0x1CE, DISPI_XRES);
        v.write16(0x1CF, 320);
        v.write16(0x1CE, DISPI_YRES);
        v.write16(0x1CF, 200);
        v.write16(0x1CE, DISPI_BPP);
        v.write16(0x1CF, 8);
        v.write16(0x1CE, DISPI_ENABLE);
        v.write16(0x1CF, ENABLED);
        assert!(v.graphics());
        assert_eq!(v.pitch(), 320);
        v.write16(0x1CE, DISPI_BANK);
        v.write16(0x1CF, 2);
        assert_eq!(v.window_offset(0xA0010), 2 * 65536 + 16);
        // Palette: write colour 1 as pure red and render a pixel.
        v.write(0x3C8, 1);
        for c in [63, 0, 0] {
            v.write(0x3C9, c);
        }
        v.vram[5] = 1;
        let mut out = Vec::new();
        assert_eq!(v.render_graphics(&mut out), (320, 200));
        assert_eq!(out[5], 0xFF0000);
    }
}
