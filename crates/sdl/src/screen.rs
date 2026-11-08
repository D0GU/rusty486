//! Renders the machine's display into an ARGB pixel buffer, for both the
//! window and MCP screenshots.

use rusty486_core::devices::video::{TEXT_COLS, TEXT_ROWS};
use rusty486_core::Machine;

use crate::font;

pub const SCREEN_W: u32 = (TEXT_COLS * 8) as u32; // 640
pub const SCREEN_H: u32 = (TEXT_ROWS * 16) as u32; // 400

/// The 16 CGA/VGA text colours (ARGB).
const PALETTE: [u32; 16] = [
    0xFF000000, 0xFF0000AA, 0xFF00AA00, 0xFF00AAAA, 0xFFAA0000, 0xFFAA00AA, 0xFFAA5500, 0xFFAAAAAA,
    0xFF555555, 0xFF5555FF, 0xFF55FF55, 0xFF55FFFF, 0xFFFF5555, 0xFFFF55FF, 0xFFFFFF55, 0xFFFFFFFF,
];

pub struct Screen {
    glyphs: Vec<[u8; 8]>,
    pub pixels: Vec<u32>,
}

impl Screen {
    pub fn new() -> Self {
        Screen {
            glyphs: font::cp437(),
            pixels: vec![0; (SCREEN_W * SCREEN_H) as usize],
        }
    }

    /// Draw the 80x25 text buffer. `blink_on` drives cursor and attribute blink.
    pub fn render(&mut self, machine: &Machine, blink_on: bool) {
        let text = machine.text_buffer();
        let video = &machine.bus.video;
        let cursor = video.cursor_offset() as usize;
        let (cur_start, cur_end) = video.cursor_shape();

        for row in 0..TEXT_ROWS {
            for col in 0..TEXT_COLS {
                let i = row * TEXT_COLS + col;
                let (ch, attr) = (text[i * 2], text[i * 2 + 1]);
                let fg = PALETTE[(attr & 0x0F) as usize];
                let bg = PALETTE[((attr >> 4) & 0x07) as usize];
                // Attribute bit 7 blinks the character.
                let show_fg = attr & 0x80 == 0 || blink_on;
                let glyph = &self.glyphs[ch as usize];
                let show_cursor = i == cursor && blink_on && video.cursor_visible();

                for y in 0..16 {
                    let bits = if show_fg { glyph[y / 2] } else { 0 };
                    let cursor_line =
                        show_cursor && (cur_start as usize..=cur_end as usize).contains(&y);
                    let base = (row * 16 + y) * SCREEN_W as usize + col * 8;
                    for x in 0..8 {
                        let on = bits & (1 << x) != 0 || cursor_line;
                        self.pixels[base + x] = if on { fg } else { bg };
                    }
                }
            }
        }
    }

    /// The pixel buffer as bytes (for SDL texture upload).
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: u32 has no padding and u8 has alignment 1; the length covers
        // exactly the same memory.
        unsafe { std::slice::from_raw_parts(self.pixels.as_ptr().cast::<u8>(), self.pixels.len() * 4) }
    }

    /// Encode the current frame as a PNG.
    pub fn png(&self) -> Vec<u8> {
        let rgb: Vec<u8> = self
            .pixels
            .iter()
            .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
            .collect();
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, SCREEN_W, SCREEN_H);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().expect("PNG header");
            w.write_image_data(&rgb).expect("PNG data");
        }
        out
    }
}
