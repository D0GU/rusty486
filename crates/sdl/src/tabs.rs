//! The editor's asset tabs: Sprites, Map, Sound and Disks.
//!
//! Each draws into the editor's 80x25 cell grid (inside the window frame)
//! and, for pixel content, into an overlay on the 640x400 frame. Assets live
//! in the project's data/ folder, which is copied onto the program's disk:
//!
//! - data/SPRITES.BIN: 64 sprites of 16x16 bytes (palette indices).
//! - data/MAP.BIN: u16 width, u16 height, then width*height tile numbers.
//! - data/INSTR.BIN: 16 OPL2 instruments of 11 bytes (registers 20h, 40h,
//!   60h, 80h, E0h for modulator then carrier, then C0h).

use std::path::{Path, PathBuf};

use rusty486_core::devices::audio::RATE;
use rusty486_core::devices::opl::Opl2;
use rusty486_core::devices::video::default_palette;
use rusty486_core::fat::Fs;
use rusty486_core::machine::Drive;
use sdl2::keyboard::{Keycode, Mod};

use crate::emu::Emu;

pub const SPRITES: usize = 64;
pub const SPRITE_W: usize = 16;
const MAP_W: usize = 64;
const MAP_H: usize = 32;
const INSTRUMENTS: usize = 16;

/// Colours (ARGB) of the 256-entry default palette.
fn palette_argb() -> Vec<u32> {
    let p = default_palette();
    (0..256)
        .map(|i| {
            let c = |k: usize| (p[i * 3 + k] as u32 * 255 / 63) & 0xFF;
            0xFF00_0000 | c(0) << 16 | c(1) << 8 | c(2)
        })
        .collect()
}

/// Write `text` into the cell grid at (x, y) with attribute `a`.
fn puts(cells: &mut [u8], x: usize, y: usize, text: &str, a: u8) {
    for (k, b) in text.bytes().enumerate() {
        let i = (y * 80 + x + k) * 2;
        if x + k < 80 && i + 1 < cells.len() {
            cells[i] = b;
            cells[i + 1] = a;
        }
    }
}

fn rect(px: &mut [u32], x: usize, y: usize, w: usize, h: usize, c: u32) {
    for yy in y..(y + h).min(400) {
        for xx in x..(x + w).min(640) {
            px[yy * 640 + xx] = c;
        }
    }
}

fn outline(px: &mut [u32], x: usize, y: usize, w: usize, h: usize, c: u32) {
    rect(px, x, y, w, 1, c);
    rect(px, x, y + h - 1, w, 1, c);
    rect(px, x, y, 1, h, c);
    rect(px, x + w - 1, y, 1, h, c);
}

const ATTR: u8 = 0x17; // light grey on blue
const HILITE: u8 = 0x1E; // yellow on blue
const SELECT: u8 = 0x71; // blue on grey

// ------------------------------------------------------------------ sprites

pub struct SpriteTab {
    pub data: Vec<u8>,
    pub sprite: usize,
    pub color: u8,
    pub dirty: bool,
    pal: Vec<u32>,
}

const CANVAS: (usize, usize, usize) = (24, 48, 10); // x, y, pixel size
const PAL_POS: (usize, usize) = (216, 48);
const SHEET_POS: (usize, usize) = (376, 48);

impl SpriteTab {
    fn load(dir: &Path) -> SpriteTab {
        let mut data = std::fs::read(dir.join("data/SPRITES.BIN")).unwrap_or_default();
        data.resize(SPRITES * SPRITE_W * SPRITE_W, 0);
        SpriteTab { data, sprite: 0, color: 15, dirty: false, pal: palette_argb() }
    }

    fn save(&mut self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir.join("data"))?;
        std::fs::write(dir.join("data/SPRITES.BIN"), &self.data)?;
        self.dirty = false;
        Ok(())
    }

    fn px(&mut self, x: usize, y: usize) -> &mut u8 {
        &mut self.data[self.sprite * 256 + y * 16 + x]
    }

    fn draw(&self, cells: &mut [u8]) {
        puts(cells, 3, 2, "Sprite", HILITE);
        puts(cells, 27, 2, "Palette", HILITE);
        puts(cells, 47, 2, "Sheet (64 sprites)", HILITE);
        let info = format!("Sprite {:2}   Colour {:3}{}", self.sprite, self.color, if self.dirty { "   (unsaved)" } else { "" });
        puts(cells, 3, 15, &info, ATTR);
        let help = [
            "Left click: paint   Right click: pick colour   Click the sheet: choose sprite",
            "PgUp/PgDn: previous/next sprite    C: clear    F2: save data\\SPRITES.BIN",
        ];
        for (i, h) in help.iter().enumerate() {
            puts(cells, 3, 18 + i, h, ATTR);
        }
    }

    fn overlay(&self, px: &mut [u32]) {
        let (cx, cy, s) = CANVAS;
        let base = self.sprite * 256;
        for y in 0..16 {
            for x in 0..16 {
                let c = self.pal[self.data[base + y * 16 + x] as usize];
                rect(px, cx + x * s, cy + y * s, s, s, c);
            }
        }
        outline(px, cx - 1, cy - 1, 16 * s + 2, 16 * s + 2, 0xFFFFFFFF);
        let (pxp, pyp) = PAL_POS;
        for i in 0..256 {
            rect(px, pxp + (i % 16) * 8, pyp + (i / 16) * 8, 8, 8, self.pal[i]);
        }
        let c = self.color as usize;
        outline(px, pxp + (c % 16) * 8 - 1, pyp + (c / 16) * 8 - 1, 10, 10, 0xFFFFFFFF);
        let (sx, sy) = SHEET_POS;
        for n in 0..SPRITES {
            let (ox, oy) = (sx + (n % 8) * 17, sy + (n / 8) * 17);
            for y in 0..16 {
                for x in 0..16 {
                    px[(oy + y) * 640 + ox + x] = self.pal[self.data[n * 256 + y * 16 + x] as usize];
                }
            }
        }
        outline(px, sx + (self.sprite % 8) * 17 - 1, sy + (self.sprite / 8) * 17 - 1, 18, 18, 0xFFFFFF55);
    }

    fn mouse(&mut self, x: usize, y: usize, left: bool) {
        let (cx, cy, s) = CANVAS;
        if (cx..cx + 16 * s).contains(&x) && (cy..cy + 16 * s).contains(&y) {
            let (gx, gy) = ((x - cx) / s, (y - cy) / s);
            if left {
                let c = self.color;
                *self.px(gx, gy) = c;
                self.dirty = true;
            } else {
                self.color = *self.px(gx, gy);
            }
            return;
        }
        let (pxp, pyp) = PAL_POS;
        if (pxp..pxp + 128).contains(&x) && (pyp..pyp + 128).contains(&y) {
            self.color = (((y - pyp) / 8) * 16 + (x - pxp) / 8) as u8;
            return;
        }
        let (sx, sy) = SHEET_POS;
        if (sx..sx + 8 * 17).contains(&x) && (sy..sy + 8 * 17).contains(&y) {
            self.sprite = ((y - sy) / 17) * 8 + (x - sx) / 17;
        }
    }

    fn key(&mut self, k: Keycode) {
        match k {
            Keycode::PageUp => self.sprite = (self.sprite + SPRITES - 1) % SPRITES,
            Keycode::PageDown => self.sprite = (self.sprite + 1) % SPRITES,
            Keycode::C => {
                let base = self.sprite * 256;
                self.data[base..base + 256].fill(0);
                self.dirty = true;
            }
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ map

pub struct MapTab {
    pub w: usize,
    pub h: usize,
    pub tiles: Vec<u8>,
    pub tile: u8,
    scroll: (usize, usize),
    pub dirty: bool,
}

const VIEW: (usize, usize, usize, usize) = (8, 32, 38, 19); // x, y, tiles across, down

impl MapTab {
    fn load(dir: &Path) -> MapTab {
        let raw = std::fs::read(dir.join("data/MAP.BIN")).unwrap_or_default();
        let (w, h) = if raw.len() >= 4 {
            (u16::from_le_bytes([raw[0], raw[1]]) as usize, u16::from_le_bytes([raw[2], raw[3]]) as usize)
        } else {
            (MAP_W, MAP_H)
        };
        let mut tiles = raw.get(4..).map(<[u8]>::to_vec).unwrap_or_default();
        tiles.resize(w * h, 0);
        MapTab { w, h, tiles, tile: 1, scroll: (0, 0), dirty: false }
    }

    fn save(&mut self, dir: &Path) -> std::io::Result<()> {
        let mut out = Vec::with_capacity(4 + self.tiles.len());
        out.extend_from_slice(&(self.w as u16).to_le_bytes());
        out.extend_from_slice(&(self.h as u16).to_le_bytes());
        out.extend_from_slice(&self.tiles);
        std::fs::create_dir_all(dir.join("data"))?;
        std::fs::write(dir.join("data/MAP.BIN"), out)?;
        self.dirty = false;
        Ok(())
    }

    fn draw(&self, cells: &mut [u8]) {
        let info = format!(
            " Map {}x{}  view {},{}  tile {:2}  [ ] change tile  arrows scroll  F2 save{} ",
            self.w,
            self.h,
            self.scroll.0,
            self.scroll.1,
            self.tile,
            if self.dirty { "  *" } else { "" }
        );
        puts(cells, 2, 22, &info, SELECT);
    }

    fn overlay(&self, px: &mut [u32], sprites: &SpriteTab) {
        let (vx, vy, tw, th) = VIEW;
        for ty in 0..th {
            for tx in 0..tw {
                let (mx, my) = (tx + self.scroll.0, ty + self.scroll.1);
                let (ox, oy) = (vx + tx * 16, vy + ty * 16);
                if mx >= self.w || my >= self.h {
                    rect(px, ox, oy, 16, 16, 0xFF202020);
                    continue;
                }
                let t = self.tiles[my * self.w + mx] as usize % SPRITES;
                for y in 0..16 {
                    for x in 0..16 {
                        px[(oy + y) * 640 + ox + x] = sprites.pal[sprites.data[t * 256 + y * 16 + x] as usize];
                    }
                }
            }
        }
    }

    fn mouse(&mut self, x: usize, y: usize, left: bool) {
        let (vx, vy, tw, th) = VIEW;
        if x < vx || y < vy || x >= vx + tw * 16 || y >= vy + th * 16 {
            return;
        }
        let (mx, my) = ((x - vx) / 16 + self.scroll.0, (y - vy) / 16 + self.scroll.1);
        if mx < self.w && my < self.h {
            let cell = &mut self.tiles[my * self.w + mx];
            if left {
                *cell = self.tile;
                self.dirty = true;
            } else {
                self.tile = *cell;
            }
        }
    }

    fn key(&mut self, k: Keycode) {
        let (_, _, tw, th) = VIEW;
        match k {
            Keycode::Left => self.scroll.0 = self.scroll.0.saturating_sub(1),
            Keycode::Right => self.scroll.0 = (self.scroll.0 + 1).min(self.w.saturating_sub(tw)),
            Keycode::Up => self.scroll.1 = self.scroll.1.saturating_sub(1),
            Keycode::Down => self.scroll.1 = (self.scroll.1 + 1).min(self.h.saturating_sub(th)),
            Keycode::LeftBracket => self.tile = (self.tile as usize + SPRITES - 1) as u8 % SPRITES as u8,
            Keycode::RightBracket => self.tile = (self.tile + 1) % SPRITES as u8,
            _ => {}
        }
    }
}

// ------------------------------------------------------------------ sound

/// Editable instrument parameters: (label, byte index, shift, mask).
const PARAMS: [(&str, usize, u8, u8); 15] = [
    ("Mod multiplier", 0, 0, 15),
    ("Mod level (0=loud)", 2, 0, 63),
    ("Mod attack", 4, 4, 15),
    ("Mod decay", 4, 0, 15),
    ("Mod sustain level", 6, 4, 15),
    ("Mod release", 6, 0, 15),
    ("Mod waveform", 8, 0, 3),
    ("Car multiplier", 1, 0, 15),
    ("Car level (0=loud)", 3, 0, 63),
    ("Car attack", 5, 4, 15),
    ("Car decay", 5, 0, 15),
    ("Car sustain level", 7, 4, 15),
    ("Car release", 7, 0, 15),
    ("Car waveform", 9, 0, 3),
    ("Feedback", 10, 1, 7),
];

pub struct SoundTab {
    pub instr: Vec<[u8; 11]>,
    pub current: usize,
    param: usize,
    pub dirty: bool,
    opl: Opl2,
    /// Samples left before the preview note is released.
    note_left: usize,
}

impl SoundTab {
    fn load(dir: &Path) -> SoundTab {
        let raw = std::fs::read(dir.join("data/INSTR.BIN")).unwrap_or_default();
        let mut instr = vec![[0x01, 0x01, 0x10, 0x00, 0xF2, 0xF2, 0x77, 0x77, 0, 0, 0x06]; INSTRUMENTS];
        for (i, chunk) in raw.chunks(11).take(INSTRUMENTS).enumerate() {
            if chunk.len() == 11 {
                instr[i].copy_from_slice(chunk);
            }
        }
        SoundTab { instr, current: 0, param: 0, dirty: false, opl: Opl2::new(RATE as f32), note_left: 0 }
    }

    fn save(&mut self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir.join("data"))?;
        std::fs::write(dir.join("data/INSTR.BIN"), self.instr.concat())?;
        self.dirty = false;
        Ok(())
    }

    fn get(&self, p: usize) -> u8 {
        let (_, idx, shift, mask) = PARAMS[p];
        (self.instr[self.current][idx] >> shift) & mask
    }

    fn set(&mut self, p: usize, v: u8) {
        let (_, idx, shift, mask) = PARAMS[p];
        let b = &mut self.instr[self.current][idx];
        *b = (*b & !(mask << shift)) | ((v & mask) << shift);
        self.dirty = true;
    }

    fn draw(&self, cells: &mut [u8]) {
        puts(cells, 3, 2, "Instruments", HILITE);
        for i in 0..INSTRUMENTS {
            let a = if i == self.current { SELECT } else { ATTR };
            puts(cells, 3, 3 + i, &format!(" Instrument {:2} ", i), a);
        }
        puts(cells, 24, 2, "Parameters (Up/Down select, Left/Right change)", HILITE);
        for (p, (label, ..)) in PARAMS.iter().enumerate() {
            let a = if p == self.param { SELECT } else { ATTR };
            puts(cells, 24, 3 + p, &format!(" {label:<20} {:3} ", self.get(p)), a);
        }
        puts(cells, 24, 19, "Play notes with Z S X D C V G B H N J M (and Q-U row)", ATTR);
        puts(cells, 24, 20, "PgUp/PgDn: instrument   F2: save data\\INSTR.BIN", ATTR);
        if self.dirty {
            puts(cells, 3, 20, "(unsaved)", HILITE);
        }
    }

    fn key(&mut self, k: Keycode) {
        let piano = |k: Keycode| -> Option<i32> {
            const LOW: [Keycode; 12] = [
                Keycode::Z, Keycode::S, Keycode::X, Keycode::D, Keycode::C, Keycode::V, Keycode::G, Keycode::B,
                Keycode::H, Keycode::N, Keycode::J, Keycode::M,
            ];
            const HIGH: [Keycode; 12] = [
                Keycode::Q, Keycode::Num2, Keycode::W, Keycode::Num3, Keycode::E, Keycode::R, Keycode::Num5,
                Keycode::T, Keycode::Num6, Keycode::Y, Keycode::Num7, Keycode::U,
            ];
            LOW.iter().position(|x| *x == k).map(|i| i as i32).or_else(|| HIGH.iter().position(|x| *x == k).map(|i| i as i32 + 12))
        };
        match k {
            Keycode::Up => self.param = (self.param + PARAMS.len() - 1) % PARAMS.len(),
            Keycode::Down => self.param = (self.param + 1) % PARAMS.len(),
            Keycode::Left => {
                let v = self.get(self.param);
                self.set(self.param, v.saturating_sub(1));
            }
            Keycode::Right => {
                let v = self.get(self.param);
                self.set(self.param, v.saturating_add(1).min(PARAMS[self.param].3));
            }
            Keycode::PageUp => self.current = (self.current + INSTRUMENTS - 1) % INSTRUMENTS,
            Keycode::PageDown => self.current = (self.current + 1) % INSTRUMENTS,
            k => {
                if let Some(semi) = piano(k) {
                    self.play(semi);
                }
            }
        }
    }

    /// Play semitone `semi` above middle C on OPL channel 0.
    fn play(&mut self, semi: i32) {
        let ins = self.instr[self.current];
        let w = |o: &mut Opl2, r: u8, v: u8| {
            o.write_addr(r);
            o.write_data(v, 0, 33_000_000);
        };
        w(&mut self.opl, 0x01, 0x20);
        w(&mut self.opl, 0xB0, 0); // key off first
        for (i, &base) in [0x20u8, 0x40, 0x60, 0x80, 0xE0].iter().enumerate() {
            w(&mut self.opl, base, ins[i * 2]);
            w(&mut self.opl, base + 3, ins[i * 2 + 1]);
        }
        w(&mut self.opl, 0xC0, ins[10]);
        let freq = 261.63 * 2f32.powf(semi as f32 / 12.0);
        let block = 4u8;
        let fnum = (freq * (1u32 << (20 - block)) as f32 / 49_716.0).round() as u16;
        w(&mut self.opl, 0xA0, fnum as u8);
        w(&mut self.opl, 0xB0, 0x20 | block << 2 | (fnum >> 8) as u8 & 3);
        self.note_left = RATE as usize / 2;
    }

    /// Mix preview audio into `out`.
    pub fn render(&mut self, out: &mut [i16]) {
        for s in out.iter_mut() {
            if self.note_left > 0 {
                self.note_left -= 1;
                if self.note_left == 0 {
                    self.opl.write_addr(0xB0);
                    self.opl.write_data(0, 0, 33_000_000);
                }
            }
            let v = (self.opl.sample() * 0.5 * 32767.0) as i32;
            *s = (*s as i32 + v).clamp(-32768, 32767) as i16;
        }
    }
}

// ------------------------------------------------------------------ disks

pub struct DiskTab {
    pub drive: usize,
    pub path: String,
    pub file: usize,
    /// A text prompt being typed (e.g. an image path to insert).
    pub prompt: Option<String>,
    pub message: String,
}

impl DiskTab {
    fn new() -> DiskTab {
        DiskTab { drive: 0, path: String::new(), file: 0, prompt: None, message: String::new() }
    }

    fn draw(&self, cells: &mut [u8], emu: &Emu) {
        puts(cells, 3, 2, "Drives", HILITE);
        for (i, letter) in ["A", "B", "C", "D"].iter().enumerate() {
            let a = if i == self.drive { SELECT } else { ATTR };
            let line = match emu.machine.bus.disk.drives[i].as_ref() {
                Some(d) => {
                    let name = emu.disk_paths[i]
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .map_or("(memory)".to_string(), |n| n.to_string_lossy().into_owned());
                    format!(" {letter}: {name:<14} {:>6} KB{} ", d.data.len() / 1024, if d.dirty { "*" } else { " " })
                }
                None => format!(" {letter}: (empty)                    "),
            };
            puts(cells, 3, 3 + i, &line, a);
        }
        puts(cells, 3, 9, "Keys: Up/Down drive   N new floppy in A:", ATTR);
        puts(cells, 3, 10, "      I insert image   E eject   S save", ATTR);
        puts(cells, 3, 11, "      Enter open dir   Backspace up", ATTR);
        puts(cells, 3, 12, "      PgUp/PgDn file   Del delete file", ATTR);
        let title = format!("Files on {}:\\{}", ["A", "B", "C", "D"][self.drive], self.path.replace('/', "\\"));
        puts(cells, 42, 2, &title, HILITE);
        if let Some(entries) = self.list(emu) {
            for (row, e) in entries.iter().skip(self.file.saturating_sub(17)).take(18).enumerate() {
                let idx = self.file.saturating_sub(17) + row;
                let a = if idx == self.file { SELECT } else { ATTR };
                let size = if e.is_dir() { "<DIR>".to_string() } else { e.size.to_string() };
                puts(cells, 42, 3 + row, &format!(" {:<12} {:>9} ", e.name, size), a);
            }
        } else {
            puts(cells, 42, 3, "(no FAT file system)", ATTR);
        }
        if let Some(p) = &self.prompt {
            puts(cells, 3, 15, "Image path:", HILITE);
            let shown: String = p.chars().rev().take(36).collect::<Vec<_>>().into_iter().rev().collect();
            puts(cells, 3, 16, &format!("{shown:<37}"), SELECT);
        }
        puts(cells, 3, 19, &self.message, HILITE);
    }

    fn list(&self, emu: &Emu) -> Option<Vec<rusty486_core::fat::DirEntry>> {
        let d = emu.machine.bus.disk.drives[self.drive].as_ref()?;
        let mut data = d.data.clone();
        let fs = Fs::open(&mut data).ok()?;
        fs.list(&self.path).ok()
    }

    fn drive(&self) -> Drive {
        [Drive::A, Drive::B, Drive::C, Drive::D][self.drive]
    }

    fn key(&mut self, k: Keycode, emu: &mut Emu) {
        if let Some(p) = &mut self.prompt {
            match k {
                Keycode::Return => {
                    let path = PathBuf::from(p.trim());
                    self.prompt = None;
                    match std::fs::read(&path) {
                        Ok(data) => {
                            emu.insert_disk(self.drive(), path.clone(), rusty486_core::devices::disk::DiskImage::new(data));
                            self.message = format!("Inserted {}", path.display());
                        }
                        Err(e) => self.message = format!("Cannot read {}: {e}", path.display()),
                    }
                }
                Keycode::Escape => self.prompt = None,
                Keycode::Backspace => {
                    p.pop();
                }
                _ => {}
            }
            return;
        }
        match k {
            Keycode::Up => {
                self.drive = (self.drive + 3) % 4;
                self.path.clear();
                self.file = 0;
            }
            Keycode::Down => {
                self.drive = (self.drive + 1) % 4;
                self.path.clear();
                self.file = 0;
            }
            Keycode::PageUp => self.file = self.file.saturating_sub(1),
            Keycode::PageDown => self.file += 1,
            Keycode::Return => {
                if let Some(e) = self.list(emu).and_then(|l| l.get(self.file).cloned()) {
                    if e.is_dir() && e.name != "." {
                        if e.name == ".." {
                            self.up();
                        } else {
                            self.path = if self.path.is_empty() { e.name.clone() } else { format!("{}/{}", self.path, e.name) };
                        }
                        self.file = 0;
                    }
                }
            }
            Keycode::Backspace => self.up(),
            Keycode::Delete => {
                if let Some(e) = self.list(emu).and_then(|l| l.get(self.file).cloned()) {
                    let full = if self.path.is_empty() { e.name.clone() } else { format!("{}/{}", self.path, e.name) };
                    if let Some(d) = emu.machine.disk_mut(self.drive()) {
                        let r = Fs::open(&mut d.data).and_then(|mut fs| fs.remove(&full));
                        d.dirty |= r.is_ok();
                        self.message = match r {
                            Ok(()) => format!("Deleted {full}"),
                            Err(e) => e.to_string(),
                        };
                    }
                }
            }
            Keycode::N => {
                let dir = crate::project::hdd_path().parent().map(Path::to_path_buf).unwrap_or_default();
                let path = (1..1000).map(|n| dir.join(format!("floppy{n}.img"))).find(|p| !p.exists()).unwrap();
                match rusty486_firmware::tinydos_floppy("FLOPPY", &[]) {
                    Ok(data) => {
                        let _ = std::fs::write(&path, &data);
                        emu.insert_disk(Drive::A, path.clone(), rusty486_core::devices::disk::DiskImage::new(data));
                        self.drive = 0;
                        self.message = format!("New TinyDOS floppy in A: ({})", path.display());
                    }
                    Err(e) => self.message = e.to_string(),
                }
            }
            Keycode::I => self.prompt = Some(String::new()),
            Keycode::E => {
                let _ = emu.save_disk(self.drive());
                emu.machine.eject_disk(self.drive());
                emu.disk_paths[self.drive] = None;
                self.message = "Ejected".into();
            }
            Keycode::S => {
                self.message = match emu.save_disk(self.drive()) {
                    Ok(true) => "Saved".into(),
                    Ok(false) => "Nothing to save".into(),
                    Err(e) => e.to_string(),
                };
            }
            _ => {}
        }
    }

    fn up(&mut self) {
        match self.path.rsplit_once('/') {
            Some((parent, _)) => self.path = parent.to_string(),
            None => self.path.clear(),
        }
        self.file = 0;
    }

    fn text(&mut self, s: &str) {
        if let Some(p) = &mut self.prompt {
            p.push_str(s);
        }
    }
}

// ------------------------------------------------------------------ all tabs

pub struct Tabs {
    pub sprites: SpriteTab,
    pub map: MapTab,
    pub sound: SoundTab,
    pub disks: DiskTab,
}

impl Tabs {
    pub fn load(dir: &Path) -> Tabs {
        Tabs { sprites: SpriteTab::load(dir), map: MapTab::load(dir), sound: SoundTab::load(dir), disks: DiskTab::new() }
    }

    /// Save every modified asset. Returns a status message.
    pub fn save(&mut self, dir: &Path) -> String {
        let mut saved = Vec::new();
        let mut err = None;
        if self.sprites.dirty {
            match self.sprites.save(dir) {
                Ok(()) => saved.push("SPRITES.BIN"),
                Err(e) => err = Some(e),
            }
        }
        if self.map.dirty {
            match self.map.save(dir) {
                Ok(()) => saved.push("MAP.BIN"),
                Err(e) => err = Some(e),
            }
        }
        if self.sound.dirty {
            match self.sound.save(dir) {
                Ok(()) => saved.push("INSTR.BIN"),
                Err(e) => err = Some(e),
            }
        }
        match (err, saved.is_empty()) {
            (Some(e), _) => format!("Save failed: {e}"),
            (None, true) => "Nothing to save".into(),
            (None, false) => format!("Saved data\\{}", saved.join(", data\\")),
        }
    }
}

/// Which non-code tab is shown.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Sprites,
    Map,
    Sound,
    Disks,
}

pub fn draw(which: Which, emu: &Emu, cells: &mut [u8]) {
    let t = &emu.tabs;
    match which {
        Which::Sprites => t.sprites.draw(cells),
        Which::Map => t.map.draw(cells),
        Which::Sound => t.sound.draw(cells),
        Which::Disks => t.disks.draw(cells, emu),
    }
}

pub fn overlay(which: Which, tabs: &Tabs, px: &mut [u32]) {
    match which {
        Which::Sprites => tabs.sprites.overlay(px),
        Which::Map => tabs.map.overlay(px, &tabs.sprites),
        _ => {}
    }
}

pub fn key(which: Which, emu: &mut Emu, k: Keycode, _m: Mod) {
    match which {
        Which::Sprites => emu.tabs.sprites.key(k),
        Which::Map => emu.tabs.map.key(k),
        Which::Sound => emu.tabs.sound.key(k),
        Which::Disks => {
            let mut disks = std::mem::replace(&mut emu.tabs.disks, DiskTab::new());
            disks.key(k, emu);
            emu.tabs.disks = disks;
        }
    }
}

pub fn text(which: Which, emu: &mut Emu, s: &str) {
    if which == Which::Disks {
        emu.tabs.disks.text(s);
    }
}

/// Mouse at pixel (x, y) of the 640x400 frame.
pub fn mouse(which: Which, emu: &mut Emu, x: usize, y: usize, left: bool) {
    match which {
        Which::Sprites => emu.tabs.sprites.mouse(x, y, left),
        Which::Map => emu.tabs.map.mouse(x, y, left),
        _ => {}
    }
}
