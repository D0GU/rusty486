//! The Settings window (Ctrl+F12): a graphical overlay drawn by the host for
//! video, emulation, drive, audio and connection settings. Settings persist
//! in %APPDATA%\rusty486\settings.json.

use std::path::{Path, PathBuf};

use font8x8::legacy::BASIC_LEGACY;
use rusty486_core::devices::disk::DiskImage;
use rusty486_core::machine::Drive;
use serde::{Deserialize, Serialize};

use crate::emu::Emu;

// ------------------------------------------------------------------ settings

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aspect {
    /// 640x400 shown at 4:3, like a CRT.
    Crt,
    /// Square pixels (640x400 at 16:10).
    Square,
    /// Fill the window.
    Stretch,
}

/// The emulated CPU.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CpuChoice {
    /// The classic 486SX at a fixed clock (the `mhz` setting).
    #[default]
    #[serde(rename = "486SX")]
    I486Sx,
    /// The fantasy rusty R1024: FPU, and turbo clocking up to 1024 MHz.
    R1024,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub cpu: CpuChoice,
    pub fullscreen: bool,
    pub aspect: Aspect,
    pub integer_scale: bool,
    pub smooth: bool,
    pub scanlines: bool,
    pub window_scale: u32,
    pub mhz: u32,
    pub pause_in_menu: bool,
    pub muted: bool,
    pub volume: u32,
    /// Disk images inserted from the Drives page (A:, B:, C:, D:), put back
    /// in at the next start. None: empty (for C:, the standard hdd.img).
    pub disks: [Option<PathBuf>; 4],
    /// Images inserted lately, newest first, offered by the image picker.
    pub recent_images: Vec<PathBuf>,
}

/// How many recent images the picker remembers.
const RECENT_IMAGES: usize = 10;

impl Settings {
    /// Note that `path` is now in `drive` (None: ejected), for next time.
    pub fn remember_disk(&mut self, drive: usize, path: Option<PathBuf>) {
        if let Some(p) = &path {
            self.recent_images.retain(|r| r != p);
            self.recent_images.insert(0, p.clone());
            self.recent_images.truncate(RECENT_IMAGES);
        }
        self.disks[drive] = path;
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            cpu: CpuChoice::I486Sx,
            fullscreen: false,
            aspect: Aspect::Crt,
            integer_scale: false,
            smooth: false,
            scanlines: false,
            window_scale: 2,
            mhz: 33,
            pause_in_menu: false,
            muted: false,
            volume: 80,
            disks: Default::default(),
            recent_images: Vec::new(),
        }
    }
}

pub const SPEEDS: &[u32] = &[5, 8, 12, 16, 20, 25, 33, 40, 50, 66, 75, 100];

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("HOME"))
        .map(|d| PathBuf::from(d).join("rusty486").join("settings.json"))
}

impl Settings {
    pub fn load() -> Settings {
        settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        // Tests change settings freely; never let them touch the user's file.
        if cfg!(test) {
            return;
        }
        if let Some(p) = settings_path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(s) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, s);
            }
        }
    }

    /// Height of the view in logical pixels (the width is always 640).
    pub fn view_h(&self) -> u32 {
        match self.aspect {
            Aspect::Square => 400,
            _ => 480,
        }
    }
}

/// Things the menu asks the main loop to do with SDL objects.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// Re-apply fullscreen, scaling and filtering from the settings.
    Video,
    WindowScale,
    Copy(String),
}

// ------------------------------------------------------------------ drawing

pub const UI_W: usize = 640;
pub const UI_H: usize = 400;

const DIM: u32 = 0xB0_05070C;
const PANEL: u32 = 0xFF_1E2430;
const EDGE: u32 = 0xFF_0B0E14;
const TITLE_A: u32 = 0xFF_2F64BE;
const TITLE_B: u32 = 0xFF_1B3A72;
const SIDEBAR: u32 = 0xFF_161A23;
const SIDE_SEL: u32 = 0xFF_2A3346;
const ACCENT: u32 = 0xFF_4C8DFF;
const TEXT: u32 = 0xFF_E6E9EF;
const TEXT_DIM: u32 = 0xFF_8B93A5;
const ROW_SEL: u32 = 0xFF_283044;
const ROW_HOVER: u32 = 0xFF_222937;
const BUTTON: u32 = 0xFF_34405A;
const BUTTON_HI: u32 = 0xFF_4D6290;
const BUTTON_FOCUS: u32 = 0xFF_3D6FD8;
const SWITCH_OFF: u32 = 0xFF_454B5A;
const GOOD: u32 = 0xFF_3DBA6F;
const WARN: u32 = 0xFF_E0A030;
const FIELD: u32 = 0xFF_10131A;

pub struct Px {
    pub buf: Vec<u32>,
}

impl Px {
    fn new() -> Px {
        Px { buf: vec![0; UI_W * UI_H] }
    }
    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        let alpha = c >> 24;
        for yy in y.max(0)..(y + h).min(UI_H as i32) {
            for xx in x.max(0)..(x + w).min(UI_W as i32) {
                let i = yy as usize * UI_W + xx as usize;
                self.buf[i] = if alpha == 0xFF { c } else { blend(self.buf[i], c) };
            }
        }
    }
    /// A rectangle with its corners cut by one pixel (a soft rounded look).
    fn rounded(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        self.fill(x + 1, y, w - 2, h, c);
        self.fill(x, y + 1, 1, h - 2, c);
        self.fill(x + w - 1, y + 1, 1, h - 2, c);
    }
    fn outline(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        self.fill(x, y, w, 1, c);
        self.fill(x, y + h - 1, w, 1, c);
        self.fill(x, y, 1, h, c);
        self.fill(x + w - 1, y, 1, h, c);
    }
    fn glyph(&mut self, x: i32, y: i32, g: &[u8; 8], c: u32, tall: bool) {
        for (row, bits) in g.iter().enumerate() {
            for col in 0..8 {
                if bits & (1 << col) != 0 {
                    if tall {
                        self.fill(x + col, y + row as i32 * 2, 1, 2, c);
                    } else {
                        self.fill(x + col, y + row as i32, 1, 1, c);
                    }
                }
            }
        }
    }
    fn text(&mut self, x: i32, y: i32, s: &str, c: u32) -> i32 {
        let mut cx = x;
        for ch in s.chars() {
            let g = BASIC_LEGACY.get(ch as usize).copied().unwrap_or([0; 8]);
            self.glyph(cx, y, &g, c, false);
            cx += 8;
        }
        cx
    }
    /// Text at double height, bold.
    fn title(&mut self, x: i32, y: i32, s: &str, c: u32) {
        let mut cx = x;
        for ch in s.chars() {
            let g = BASIC_LEGACY.get(ch as usize).copied().unwrap_or([0; 8]);
            self.glyph(cx, y, &g, c, true);
            self.glyph(cx + 1, y, &g, c, true);
            cx += 9;
        }
    }
    fn icon(&mut self, x: i32, y: i32, which: usize, c: u32) {
        let g: [u8; 8] = match which {
            0 => [0x7E, 0x42, 0x5A, 0x5A, 0x42, 0x7E, 0x18, 0x3C], // monitor
            1 => [0x24, 0x7E, 0xC3, 0x5A, 0x5A, 0xC3, 0x7E, 0x24], // chip
            2 => [0x7E, 0x81, 0xBD, 0xA5, 0xBD, 0x81, 0x99, 0x7E], // disk
            3 => [0x08, 0x0C, 0xEA, 0xA9, 0xA9, 0xEA, 0x0C, 0x08], // speaker
            4 => [0x3C, 0x42, 0x99, 0x24, 0x00, 0x18, 0x18, 0x00], // signal
            _ => [0x3C, 0x42, 0x81, 0x99, 0x99, 0x81, 0x42, 0x3C], // info
        };
        self.glyph(x, y, &g, c, false);
    }
}

fn blend(dst: u32, src: u32) -> u32 {
    let a = (src >> 24) & 0xFF;
    let mix = |s: u32, d: u32| (s * a + d * (255 - a)) / 255;
    let r = mix((src >> 16) & 0xFF, (dst >> 16) & 0xFF);
    let g = mix((src >> 8) & 0xFF, (dst >> 8) & 0xFF);
    let b = mix(src & 0xFF, dst & 0xFF);
    0xFF00_0000 | r << 16 | g << 8 | b
}

// ------------------------------------------------------------------ menu model

const CATEGORIES: &[&str] = &["Video", "Emulation", "Drives", "Audio", "Connection", "About"];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Id {
    Fullscreen,
    Aspect,
    IntegerScale,
    Smooth,
    Scanlines,
    WindowScale,
    Cpu,
    Speed,
    PauseResume,
    Reset,
    PauseInMenu,
    Drive(usize),
    NewFloppy,
    SaveDisks,
    Mute,
    Volume,
    CopyCommand,
    Info,
}

enum Kind {
    Toggle(bool),
    Choice(String),
    Slider(u32),
    Buttons(Vec<&'static str>),
    Info(String),
}

struct Item {
    id: Id,
    label: String,
    detail: String,
    kind: Kind,
}

/// Something under the mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Category(usize),
    Row(usize, usize),
    Close,
    PickerFile(usize),
    PickerButton(usize),
    PickerField,
}

struct Picker {
    drive: usize,
    files: Vec<PathBuf>,
    sel: usize,
    top: usize,
    path: String,
    typing: bool,
    focus_button: usize,
}

pub struct Menu {
    pub open: bool,
    cat: usize,
    row: usize,
    col: usize,
    in_sidebar: bool,
    hover: Option<Hit>,
    hits: Vec<(i32, i32, i32, i32, Hit)>,
    message: String,
    picker: Option<Picker>,
    px: Px,
    mcp_url: Option<String>,
    mcp_command: Option<String>,
}

impl Menu {
    pub fn new(mcp_url: Option<String>, mcp_command: Option<String>) -> Menu {
        Menu {
            open: false,
            cat: 0,
            row: 0,
            col: 0,
            in_sidebar: true,
            hover: None,
            hits: Vec::new(),
            message: String::new(),
            picker: None,
            px: Px::new(),
            mcp_url,
            mcp_command,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.picker = None;
        self.message.clear();
    }

    fn items(&self, emu: &Emu, s: &Settings) -> Vec<Item> {
        let it = |id, label: &str, detail: &str, kind| Item { id, label: label.into(), detail: detail.into(), kind };
        match self.cat {
            0 => vec![
                it(Id::Fullscreen, "Fullscreen", "Also Alt+Enter, anywhere", Kind::Toggle(s.fullscreen)),
                it(
                    Id::Aspect,
                    "Aspect ratio",
                    "Shape of the 640x400 image",
                    Kind::Choice(
                        match s.aspect {
                            Aspect::Crt => "4:3 CRT",
                            Aspect::Square => "Square pixels",
                            Aspect::Stretch => "Stretch to fill",
                        }
                        .into(),
                    ),
                ),
                it(Id::IntegerScale, "Integer scaling", "Whole-pixel sizes only", Kind::Toggle(s.integer_scale)),
                it(Id::Smooth, "Smooth filtering", "Soft instead of sharp", Kind::Toggle(s.smooth)),
                it(Id::Scanlines, "CRT scanlines", "Darken every other line", Kind::Toggle(s.scanlines)),
                it(Id::WindowScale, "Window size", "When not fullscreen", Kind::Choice(format!("{}x", s.window_scale))),
            ],
            1 => vec![
                it(
                    Id::Cpu,
                    "CPU",
                    "Changing it resets the machine",
                    Kind::Choice(
                        match s.cpu {
                            CpuChoice::I486Sx => "486SX",
                            CpuChoice::R1024 => "R1024 (fantasy, FPU)",
                        }
                        .into(),
                    ),
                ),
                if s.cpu == CpuChoice::R1024 {
                    it(Id::Speed, "CPU speed", "As fast as this PC allows", Kind::Choice("Turbo, up to 1024 MHz".into()))
                } else {
                    it(Id::Speed, "CPU speed", "Emulated 486 clock", Kind::Choice(format!("{} MHz", s.mhz)))
                },
                it(
                    Id::PauseResume,
                    if emu.paused { "Paused" } else { "Running" },
                    if emu.paused { &emu.stop_reason } else { "Pause key also pauses" },
                    Kind::Buttons(vec![if emu.paused { "Resume" } else { "Pause" }]),
                ),
                it(Id::Reset, "Reset", "Restart the machine (F11)", Kind::Buttons(vec!["Reset"])),
                it(Id::PauseInMenu, "Pause in this menu", "While Settings is open", Kind::Toggle(s.pause_in_menu)),
            ],
            2 => {
                let mut v = Vec::new();
                for d in 0..4 {
                    let name = ["A:  Floppy", "B:  Floppy", "C:  Hard disk", "D:  Hard disk"][d];
                    let detail = match (&emu.disk_paths[d], emu.machine.bus.disk.drives[d].as_ref()) {
                        (Some(p), Some(img)) => {
                            let f = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                            format!("{f} ({} KB){}", img.data.len() / 1024, if img.dirty { " *" } else { "" })
                        }
                        (None, Some(img)) => format!("built-in image ({} KB)", img.data.len() / 1024),
                        _ => "empty".into(),
                    };
                    v.push(it(Id::Drive(d), name, &detail, Kind::Buttons(vec!["Insert", "Eject", "Save"])));
                }
                v.push(it(Id::NewFloppy, "New floppy", "Blank TinyDOS disk in A:", Kind::Buttons(vec!["Create"])));
                v.push(it(Id::SaveDisks, "Save all disks", "Write changes to files", Kind::Buttons(vec!["Save all"])));
                v
            }
            3 => vec![
                it(Id::Mute, "Mute", "All sound devices", Kind::Toggle(s.muted)),
                it(Id::Volume, "Volume", "Left/Right or drag", Kind::Slider(s.volume)),
            ],
            4 => {
                let mut v = vec![it(
                    Id::Info,
                    "MCP server",
                    "For Claude Code",
                    Kind::Info(self.mcp_url.clone().unwrap_or_else(|| "disabled (--no-mcp)".into())),
                )];
                if self.mcp_command.is_some() {
                    v.push(it(Id::CopyCommand, "Connect Claude Code", "Copies 'claude mcp add'", Kind::Buttons(vec!["Copy command"])));
                }
                v
            }
            _ => vec![
                it(Id::Info, "rusty486", "A fantasy 486 PC in Rust", Kind::Info(env!("CARGO_PKG_VERSION").into())),
                it(Id::Info, "Ctrl+F12", "Settings (this window)", Kind::Info(String::new())),
                it(Id::Info, "Alt+Enter", "Fullscreen on/off", Kind::Info(String::new())),
                it(Id::Info, "F12", "Editor (Esc returns)", Kind::Info(String::new())),
                it(Id::Info, "Ctrl+F9", "Build and run the project", Kind::Info(String::new())),
                it(Id::Info, "F11", "Reset", Kind::Info(String::new())),
                it(Id::Info, "Pause", "Pause / resume", Kind::Info(String::new())),
            ],
        }
    }

    fn selectable(items: &[Item]) -> Vec<usize> {
        (0..items.len()).filter(|&i| !matches!(items[i].kind, Kind::Info(_))).collect()
    }

    // ---------------------------------------------------------------- actions

    /// Change a value (Left = -1, Right = +1) or press a button (d = 0).
    fn act(&mut self, idx: usize, col: usize, d: i32, emu: &mut Emu, s: &mut Settings) -> Vec<Effect> {
        let items = self.items(emu, s);
        let Some(item) = items.get(idx) else { return vec![] };
        let mut fx = Vec::new();
        let cycle = |i: usize, n: usize| -> usize { ((i as i32 + if d < 0 { -1 } else { 1 }).rem_euclid(n as i32)) as usize };
        match item.id {
            Id::Fullscreen => {
                s.fullscreen = !s.fullscreen;
                fx.push(Effect::Video);
            }
            Id::Aspect => {
                let all = [Aspect::Crt, Aspect::Square, Aspect::Stretch];
                let i = all.iter().position(|a| *a == s.aspect).unwrap_or(0);
                s.aspect = all[cycle(i, 3)];
                fx.push(Effect::Video);
                // Fit the window to the new shape.
                fx.push(Effect::WindowScale);
            }
            Id::IntegerScale => {
                s.integer_scale = !s.integer_scale;
                fx.push(Effect::Video);
            }
            Id::Smooth => {
                s.smooth = !s.smooth;
                fx.push(Effect::Video);
            }
            Id::Scanlines => s.scanlines = !s.scanlines,
            Id::WindowScale => {
                s.window_scale = (cycle(s.window_scale as usize - 1, 4) + 1) as u32;
                fx.push(Effect::WindowScale);
            }
            Id::Cpu => {
                s.cpu = match s.cpu {
                    CpuChoice::I486Sx => CpuChoice::R1024,
                    CpuChoice::R1024 => CpuChoice::I486Sx,
                };
                emu.apply_cpu(s);
                emu.machine.reset();
                emu.resume();
                self.message = "CPU changed: machine reset".into();
            }
            Id::Speed if s.cpu == CpuChoice::R1024 => {}
            Id::Speed => {
                let i = SPEEDS.iter().position(|&m| m == s.mhz).unwrap_or(6);
                s.mhz = SPEEDS[cycle(i, SPEEDS.len())];
                emu.machine.clock_hz = s.mhz as u64 * 1_000_000;
            }
            Id::PauseResume => {
                if emu.paused {
                    emu.resume();
                } else {
                    emu.pause("paused from Settings".into());
                }
            }
            Id::Reset => {
                emu.machine.reset();
                emu.resume();
                self.message = "Machine reset".into();
            }
            Id::PauseInMenu => s.pause_in_menu = !s.pause_in_menu,
            Id::Drive(drv) => {
                let drive = [Drive::A, Drive::B, Drive::C, Drive::D][drv];
                match col {
                    0 => self.open_picker(drv, emu, s),
                    1 => {
                        let _ = emu.save_disk(drive);
                        emu.machine.eject_disk(drive);
                        emu.disk_paths[drv] = None;
                        s.remember_disk(drv, None);
                        self.message = format!("Ejected {}:", ["A", "B", "C", "D"][drv]);
                    }
                    _ => {
                        self.message = match emu.save_disk(drive) {
                            Ok(true) => "Saved".into(),
                            Ok(false) => "Nothing to save".into(),
                            Err(e) => format!("Cannot save: {e}"),
                        }
                    }
                }
            }
            Id::NewFloppy => {
                let dir = image_dir();
                let path = (1..1000).map(|n| dir.join(format!("floppy{n}.img"))).find(|p| !p.exists()).unwrap();
                self.message = match rusty486_firmware::tinydos_floppy("FLOPPY", &[]) {
                    Ok(data) => match std::fs::write(&path, &data) {
                        Ok(()) => {
                            emu.insert_disk(Drive::A, path.clone(), DiskImage::new(data));
                            s.remember_disk(0, Some(path.clone()));
                            format!("New floppy in A: {}", path.file_name().unwrap().to_string_lossy())
                        }
                        Err(e) => format!("Cannot write {}: {e}", path.display()),
                    },
                    Err(e) => e.to_string(),
                };
            }
            Id::SaveDisks => {
                emu.save_all_disks();
                self.message = "All changed disks saved".into();
            }
            Id::Mute => s.muted = !s.muted,
            Id::Volume => s.volume = (s.volume as i32 + d * 10).clamp(0, 100) as u32,
            Id::CopyCommand => {
                if let Some(c) = &self.mcp_command {
                    fx.push(Effect::Copy(c.clone()));
                    self.message = "Copied to the clipboard".into();
                }
            }
            Id::Info => {}
        }
        s.save();
        fx
    }

    fn open_picker(&mut self, drive: usize, emu: &Emu, s: &Settings) {
        // Recent images first, then images in the usual folders and in the
        // folders of recent and mounted images.
        let mut files: Vec<PathBuf> = s.recent_images.iter().filter(|p| p.is_file()).cloned().collect();
        let recent = files.len();
        let mut dirs = vec![image_dir(), emu.project.dir.clone(), emu.project.dir.join("build")];
        if let Ok(cwd) = std::env::current_dir() {
            dirs.push(cwd);
        }
        for p in s.recent_images.iter().chain(emu.disk_paths.iter().flatten()) {
            if let Some(d) = p.parent() {
                if !dirs.iter().any(|x| x == d) {
                    dirs.push(d.to_path_buf());
                }
            }
        }
        for dir in dirs {
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    let ext = p.extension().map(|x| x.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                    if matches!(ext.as_str(), "img" | "ima" | "vfd" | "flp") && !files.contains(&p) {
                        files.push(p);
                    }
                }
            }
        }
        files[recent..].sort();
        let path = emu.disk_paths[drive].as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        self.picker = Some(Picker { drive, files, sel: 0, top: 0, path, typing: false, focus_button: 0 });
    }

    fn picker_insert(&mut self, emu: &mut Emu, s: &mut Settings) {
        let Some(p) = self.picker.take() else { return };
        let path = if p.typing || p.files.is_empty() { PathBuf::from(p.path.trim()) } else { p.files[p.sel].clone() };
        let drive = [Drive::A, Drive::B, Drive::C, Drive::D][p.drive];
        self.message = match std::fs::read(&path) {
            Ok(data) => {
                emu.insert_disk(drive, path.clone(), DiskImage::new(data));
                s.remember_disk(p.drive, Some(path.clone()));
                s.save();
                format!("Inserted {} in {}:", path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(), ["A", "B", "C", "D"][p.drive])
            }
            Err(e) => format!("Cannot read {}: {e}", path.display()),
        };
    }

    // ---------------------------------------------------------------- input

    /// A key while the menu is open. Returns effects for the main loop.
    pub fn key(&mut self, key: sdl2::keyboard::Keycode, emu: &mut Emu, s: &mut Settings) -> Vec<Effect> {
        use sdl2::keyboard::Keycode as K;
        if let Some(p) = &mut self.picker {
            match key {
                K::Escape => self.picker = None,
                K::Return | K::KpEnter => {
                    if p.focus_button == 1 {
                        self.picker = None;
                    } else {
                        self.picker_insert(emu, s);
                    }
                }
                K::Tab => {
                    p.typing = !p.typing;
                }
                K::Up if !p.typing => {
                    p.sel = p.sel.saturating_sub(1);
                }
                K::Down if !p.typing => {
                    if p.sel + 1 < p.files.len() {
                        p.sel += 1;
                    }
                }
                K::Left => p.focus_button = 0,
                K::Right => p.focus_button = 1,
                K::Backspace if p.typing => {
                    p.path.pop();
                }
                _ => {}
            }
            if let Some(p) = &mut self.picker {
                if p.sel < p.top {
                    p.top = p.sel;
                }
                if p.sel >= p.top + 8 {
                    p.top = p.sel - 7;
                }
            }
            return vec![];
        }
        let items = self.items(emu, s);
        let sel = Self::selectable(&items);
        match key {
            K::Escape => {
                self.open = false;
                return vec![];
            }
            K::Tab => {
                self.in_sidebar = !self.in_sidebar;
            }
            K::Up if self.in_sidebar => {
                self.cat = (self.cat + CATEGORIES.len() - 1) % CATEGORIES.len();
                self.row = 0;
            }
            K::Down if self.in_sidebar => {
                self.cat = (self.cat + 1) % CATEGORIES.len();
                self.row = 0;
            }
            K::Right | K::Return if self.in_sidebar => {
                if !sel.is_empty() {
                    self.in_sidebar = false;
                    self.row = sel[0];
                    self.col = 0;
                }
            }
            K::Up => {
                if let Some(i) = sel.iter().position(|&r| r == self.row) {
                    if i > 0 {
                        self.row = sel[i - 1];
                        self.col = 0;
                    }
                }
            }
            K::Down => {
                if let Some(i) = sel.iter().position(|&r| r == self.row) {
                    if i + 1 < sel.len() {
                        self.row = sel[i + 1];
                        self.col = 0;
                    }
                }
            }
            K::Left | K::Right => {
                let d = if key == K::Left { -1 } else { 1 };
                match items.get(self.row).map(|i| &i.kind) {
                    Some(Kind::Buttons(b)) => {
                        if d < 0 && self.col == 0 {
                            self.in_sidebar = true;
                        } else {
                            self.col = (self.col as i32 + d).clamp(0, b.len() as i32 - 1) as usize;
                        }
                    }
                    Some(Kind::Choice(_)) | Some(Kind::Slider(_)) => return self.act(self.row, 0, d, emu, s),
                    _ => {
                        if d < 0 {
                            self.in_sidebar = true;
                        }
                    }
                }
            }
            K::Return | K::KpEnter | K::Space => return self.act(self.row, self.col, 1, emu, s),
            _ => {}
        }
        vec![]
    }

    pub fn text(&mut self, t: &str) {
        if let Some(p) = &mut self.picker {
            if p.typing {
                p.path.push_str(t);
            }
        }
    }

    fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        self.hits.iter().rev().find(|(hx, hy, w, h, _)| x >= *hx && x < hx + w && y >= *hy && y < hy + h).map(|h| h.4)
    }

    pub fn mouse_move(&mut self, x: i32, y: i32, left_down: bool, emu: &mut Emu, s: &mut Settings) {
        self.hover = self.hit(x, y);
        // Drag the volume slider.
        if left_down {
            if let Some(Hit::Row(r, _)) = self.hover {
                self.slider_at(r, x, emu, s);
            }
        }
    }

    fn slider_at(&mut self, r: usize, x: i32, emu: &Emu, s: &mut Settings) {
        let items = self.items(emu, s);
        if let Some(Item { id: Id::Volume, .. }) = items.get(r) {
            let v = ((x - SLIDER_X) * 100 / SLIDER_W).clamp(0, 100);
            s.volume = (v as u32 + 2) / 5 * 5;
            s.save();
        }
    }

    pub fn click(&mut self, x: i32, y: i32, emu: &mut Emu, s: &mut Settings) -> Vec<Effect> {
        match self.hit(x, y) {
            Some(Hit::Close) => self.open = false,
            Some(Hit::Category(c)) => {
                self.cat = c;
                self.row = 0;
                self.in_sidebar = true;
            }
            Some(Hit::Row(r, col)) => {
                self.in_sidebar = false;
                self.row = r;
                self.col = col;
                let items = self.items(emu, s);
                match items.get(r).map(|i| &i.kind) {
                    Some(Kind::Slider(_)) => self.slider_at(r, x, emu, s),
                    // Choices: the left half steps back, the right half forward.
                    Some(Kind::Choice(_)) => {
                        let d = if x < CONTROL_X + CONTROL_W / 2 { -1 } else { 1 };
                        return self.act(r, 0, d, emu, s);
                    }
                    Some(Kind::Info(_)) | None => {}
                    _ => return self.act(r, col, 1, emu, s),
                }
            }
            Some(Hit::PickerFile(i)) => {
                if let Some(p) = &mut self.picker {
                    if p.sel == i && !p.typing {
                        self.picker_insert(emu, s);
                    } else {
                        p.sel = i;
                        p.typing = false;
                    }
                }
            }
            Some(Hit::PickerField) => {
                if let Some(p) = &mut self.picker {
                    p.typing = true;
                }
            }
            Some(Hit::PickerButton(b)) => {
                if b == 0 {
                    self.picker_insert(emu, s);
                } else {
                    self.picker = None;
                }
            }
            None => {}
        }
        vec![]
    }

    // ---------------------------------------------------------------- render

    /// Draw the menu; returns the ARGB pixels (UI_W x UI_H) to blend over the view.
    pub fn render(&mut self, emu: &Emu, s: &Settings) -> &[u32] {
        self.hits.clear();
        let items = self.items(emu, s);
        let px = &mut self.px;
        px.buf.fill(0);
        px.fill(0, 0, UI_W as i32, UI_H as i32, DIM);
        let (x0, y0, w, h) = (40, 22, 560, 356);
        // Drop shadow, panel and title bar.
        px.fill(x0 + 6, y0 + 6, w, h, 0x80_000000);
        px.rounded(x0 - 1, y0 - 1, w + 2, h + 2, EDGE);
        px.rounded(x0, y0, w, h, PANEL);
        for i in 0..24 {
            let t = i as u32 * 255 / 23;
            let c = lerp(TITLE_A, TITLE_B, t);
            px.fill(x0, y0 + i, w, 1, c);
        }
        px.icon(x0 + 10, y0 + 8, 1, TEXT);
        px.title(x0 + 26, y0 + 4, "rusty486 Settings", TEXT);
        // Close button.
        let (cx, cy) = (x0 + w - 26, y0 + 4);
        let hov = self.hover == Some(Hit::Close);
        px.rounded(cx, cy, 20, 16, if hov { 0xFF_C0392B } else { 0xFF_2A4A88 });
        px.text(cx + 6, cy + 4, "x", TEXT);
        self.hits.push((cx, cy, 20, 16, Hit::Close));

        // Sidebar.
        let sy = y0 + 24;
        px.fill(x0, sy, 140, h - 24 - 26, SIDEBAR);
        for (i, name) in CATEGORIES.iter().enumerate() {
            let ry = sy + 10 + i as i32 * 30;
            let selected = i == self.cat;
            let hov = self.hover == Some(Hit::Category(i));
            if selected || hov {
                px.fill(x0 + 6, ry, 128, 24, if selected { SIDE_SEL } else { ROW_HOVER });
            }
            if selected {
                px.fill(x0 + 6, ry, 3, 24, if self.in_sidebar { ACCENT } else { TEXT_DIM });
            }
            px.icon(x0 + 16, ry + 8, i, if selected { ACCENT } else { TEXT_DIM });
            px.text(x0 + 32, ry + 8, name, if selected { TEXT } else { TEXT_DIM });
            self.hits.push((x0 + 6, ry, 128, 24, Hit::Category(i)));
        }

        // Content.
        let cx0 = x0 + 156;
        px.title(cx0, sy + 10, CATEGORIES[self.cat], TEXT);
        px.fill(cx0, sy + 30, w - 172, 1, 0xFF_323A4C);
        for (r, item) in items.iter().enumerate() {
            let ry = sy + 40 + r as i32 * 34;
            let selected = !self.in_sidebar && r == self.row && !matches!(item.kind, Kind::Info(_));
            let hov = matches!(self.hover, Some(Hit::Row(hr, _)) if hr == r) && !matches!(item.kind, Kind::Info(_));
            if selected || hov {
                px.rounded(cx0 - 6, ry - 3, w - 160, 32, if selected { ROW_SEL } else { ROW_HOVER });
            }
            if selected {
                px.fill(cx0 - 6, ry - 3, 2, 32, ACCENT);
            }
            px.text(cx0, ry + 3, &item.label, TEXT);
            px.text(cx0, ry + 15, &clip(&item.detail, 27), TEXT_DIM);
            let ctl_y = ry + 2;
            match &item.kind {
                Kind::Toggle(on) => {
                    let (tx, ty) = (CONTROL_X + CONTROL_W - 40, ctl_y + 4);
                    px.rounded(tx, ty, 34, 16, if *on { GOOD } else { SWITCH_OFF });
                    let kx = if *on { tx + 19 } else { tx + 3 };
                    px.rounded(kx, ty + 3, 12, 10, TEXT);
                    px.text(tx - 28, ty + 4, if *on { "On" } else { "Off" }, if *on { TEXT } else { TEXT_DIM });
                    self.hits.push((cx0 - 6, ry - 3, w - 160, 32, Hit::Row(r, 0)));
                }
                Kind::Choice(v) => {
                    px.rounded(CONTROL_X, ctl_y, CONTROL_W, 22, FIELD);
                    px.outline(CONTROL_X, ctl_y, CONTROL_W, 22, if selected { ACCENT } else { 0xFF_3A4356 });
                    px.text(CONTROL_X + 6, ctl_y + 7, "<", ACCENT);
                    px.text(CONTROL_X + CONTROL_W - 14, ctl_y + 7, ">", ACCENT);
                    let tw = v.len() as i32 * 8;
                    px.text(CONTROL_X + (CONTROL_W - tw) / 2, ctl_y + 7, v, TEXT);
                    self.hits.push((cx0 - 6, ry - 3, w - 160, 32, Hit::Row(r, 0)));
                }
                Kind::Slider(v) => {
                    px.rounded(SLIDER_X, ctl_y + 9, SLIDER_W, 5, SWITCH_OFF);
                    px.rounded(SLIDER_X, ctl_y + 9, SLIDER_W * *v as i32 / 100, 5, ACCENT);
                    let kx = SLIDER_X + SLIDER_W * *v as i32 / 100 - 5;
                    px.rounded(kx, ctl_y + 4, 10, 15, TEXT);
                    px.text(SLIDER_X + SLIDER_W + 8, ctl_y + 7, &format!("{v}%"), TEXT);
                    self.hits.push((cx0 - 6, ry - 3, w - 160, 32, Hit::Row(r, 0)));
                }
                Kind::Buttons(labels) => {
                    let total: i32 = labels.iter().map(|l| l.len() as i32 * 8 + 16).sum::<i32>() + (labels.len() as i32 - 1) * 6;
                    let mut bx = CONTROL_X + CONTROL_W - total;
                    for (c, l) in labels.iter().enumerate() {
                        let bw = l.len() as i32 * 8 + 16;
                        let focus = selected && self.col == c;
                        let bh = self.hover == Some(Hit::Row(r, c));
                        px.rounded(bx, ctl_y + 1, bw, 20, if focus { BUTTON_FOCUS } else if bh { BUTTON_HI } else { BUTTON });
                        px.fill(bx + 1, ctl_y + 20, bw - 2, 1, 0xFF_0E1118);
                        px.text(bx + 8, ctl_y + 7, l, TEXT);
                        self.hits.push((bx, ctl_y + 1, bw, 20, Hit::Row(r, c)));
                        bx += bw + 6;
                    }
                }
                Kind::Info(v) => {
                    let tw = v.len() as i32 * 8;
                    px.text(CONTROL_X + CONTROL_W - tw, ctl_y + 7, v, GOOD);
                }
            }
        }

        // Footer: hints or the last message.
        let fy = y0 + h - 26;
        px.fill(x0, fy, w, 1, 0xFF_323A4C);
        if self.message.is_empty() {
            px.text(x0 + 12, fy + 9, "Arrows move  Enter/Space choose  Tab switch side  Esc close", TEXT_DIM);
        } else {
            px.text(x0 + 12, fy + 9, &clip(&self.message, 66), WARN);
        }

        // Image picker dialog.
        if let Some(p) = &self.picker {
            let (dx, dy, dw, dh) = (110, 70, 420, 260);
            px.fill(0, 0, UI_W as i32, UI_H as i32, 0x60_000000);
            px.fill(dx + 6, dy + 6, dw, dh, 0x80_000000);
            px.rounded(dx - 1, dy - 1, dw + 2, dh + 2, EDGE);
            px.rounded(dx, dy, dw, dh, PANEL);
            for i in 0..22 {
                px.fill(dx, dy + i, dw, 1, lerp(TITLE_A, TITLE_B, i as u32 * 255 / 21));
            }
            px.icon(dx + 10, dy + 7, 2, TEXT);
            px.text(dx + 26, dy + 7, &format!("Insert a disk image into {}:", ["A", "B", "C", "D"][p.drive]), TEXT);
            px.fill(dx + 12, dy + 32, dw - 24, 8 * 18 + 6, FIELD);
            if p.files.is_empty() {
                px.text(dx + 20, dy + 38, "No .img files found; type a path below.", TEXT_DIM);
            }
            for (i, f) in p.files.iter().enumerate().skip(p.top).take(8) {
                let ry = dy + 35 + (i - p.top) as i32 * 18;
                let sel = i == p.sel && !p.typing;
                let hov = self.hover == Some(Hit::PickerFile(i));
                if sel || hov {
                    px.fill(dx + 14, ry, dw - 28, 17, if sel { ACCENT } else { ROW_HOVER });
                }
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                px.icon(dx + 20, ry + 5, 2, if sel { TEXT } else { TEXT_DIM });
                px.text(dx + 34, ry + 5, &clip(&name, 22), TEXT);
                let dir = f.parent().map(|d| d.display().to_string()).unwrap_or_default();
                px.text(dx + 220, ry + 5, &clip_left(&dir, 22), if sel { TEXT } else { TEXT_DIM });
                self.hits.push((dx + 14, ry, dw - 28, 17, Hit::PickerFile(i)));
            }
            let fy = dy + 190;
            px.text(dx + 12, fy - 12, "Or type a path (Tab):", TEXT_DIM);
            px.rounded(dx + 12, fy, dw - 24, 20, FIELD);
            px.outline(dx + 12, fy, dw - 24, 20, if p.typing { ACCENT } else { 0xFF_3A4356 });
            let shown = clip_left(&p.path, 48);
            let end = px.text(dx + 18, fy + 6, &shown, TEXT);
            if p.typing && emu.blink_on {
                px.fill(end, fy + 4, 2, 12, ACCENT);
            }
            self.hits.push((dx + 12, fy, dw - 24, 20, Hit::PickerField));
            for (b, l) in ["Insert", "Cancel"].iter().enumerate() {
                let bx = dx + dw - 180 + b as i32 * 86;
                let by = dy + dh - 34;
                let focus = p.focus_button == b;
                let hov = self.hover == Some(Hit::PickerButton(b));
                px.rounded(bx, by, 80, 22, if focus { BUTTON_FOCUS } else if hov { BUTTON_HI } else { BUTTON });
                px.text(bx + 40 - l.len() as i32 * 4, by + 7, l, TEXT);
                self.hits.push((bx, by, 80, 22, Hit::PickerButton(b)));
            }
        }
        &self.px.buf
    }
}

const CONTROL_X: i32 = 420;
const CONTROL_W: i32 = 168;
const SLIDER_X: i32 = 440;
const SLIDER_W: i32 = 110;

fn lerp(a: u32, b: u32, t: u32) -> u32 {
    let ch = |s: u32| {
        let x = (a >> s) & 0xFF;
        let y = (b >> s) & 0xFF;
        (x * (255 - t) + y * t) / 255
    };
    0xFF00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let t: String = s.chars().take(n - 1).collect();
        format!("{t}~")
    }
}

fn clip_left(s: &str, n: usize) -> String {
    let c = s.chars().count();
    if c <= n {
        s.to_string()
    } else {
        let t: String = s.chars().skip(c - n + 1).collect();
        format!("~{t}")
    }
}

/// Blend the Settings overlay (UI_W x UI_H) over a rendered frame of any size.
pub fn composite(screen: &mut crate::screen::Screen, ui: &[u32]) {
    let (w, h) = (screen.width as usize, screen.height as usize);
    for y in 0..h {
        let uy = y * UI_H / h;
        for x in 0..w {
            let ux = x * UI_W / w;
            let src = ui[uy * UI_W + ux];
            if src >> 24 != 0 {
                let i = y * w + x;
                screen.pixels[i] = blend(screen.pixels[i], src);
            }
        }
    }
}

/// Where new floppy images go: the rusty486 folder next to hdd.img.
fn image_dir() -> PathBuf {
    crate::project::hdd_path().parent().map(Path::to_path_buf).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_with_defaults_for_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"fullscreen": true, "mhz": 66}"#).unwrap();
        assert!(s.fullscreen);
        assert_eq!(s.mhz, 66);
        assert_eq!(s.volume, 80);
        assert_eq!(s.aspect, Aspect::Crt);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    fn test_emu() -> Emu {
        let dir = std::env::temp_dir().join(format!("rusty486-settings-test-{}", std::process::id()));
        let project = crate::project::Project::open_or_create(&dir).unwrap();
        let editor = crate::editor::Editor::open(project.entry_path()).unwrap();
        let mut machine = rusty486_core::Machine::new(rusty486_firmware::BIOS.to_vec());
        machine.insert_disk(Drive::A, DiskImage::new(rusty486_firmware::tinydos_floppy("T", &[]).unwrap()));
        Emu::new(machine, project, editor)
    }

    fn save_png(px: &[u32], name: &str) {
        let Some(dir) = std::env::var_os("RUSTY486_SETTINGS_PNG") else { return };
        let rgb: Vec<u8> = px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8]).collect();
        let f = std::fs::File::create(PathBuf::from(dir).join(name)).unwrap();
        let mut enc = png::Encoder::new(f, UI_W as u32, UI_H as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgb).unwrap();
    }

    /// A small FAT image file for drive tests.
    fn image_file(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rusty486-drives-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, rusty486_firmware::tinydos_floppy("DATA", &[("HI.TXT", b"hi")]).unwrap()).unwrap();
        path
    }

    #[test]
    fn remember_disk_keeps_recent_images_unique() {
        let mut s = Settings::default();
        let (a, b) = (PathBuf::from("a.img"), PathBuf::from("b.img"));
        s.remember_disk(3, Some(a.clone()));
        s.remember_disk(1, Some(b.clone()));
        s.remember_disk(3, Some(a.clone()));
        assert_eq!(s.disks[3], Some(a.clone()));
        assert_eq!(s.disks[1], Some(b.clone()));
        assert_eq!(s.recent_images, vec![a.clone(), b.clone()]);
        s.remember_disk(3, None);
        assert_eq!(s.disks[3], None);
        assert_eq!(s.recent_images.len(), 2, "ejecting keeps the image in the recent list");
        for i in 0..20 {
            s.remember_disk(0, Some(PathBuf::from(format!("{i}.img"))));
        }
        assert_eq!(s.recent_images.len(), RECENT_IMAGES);
        // Old settings files without the new fields still load.
        let old: Settings = serde_json::from_str(r#"{"mhz": 50}"#).unwrap();
        assert_eq!(old.disks, [None, None, None, None]);
        let round: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(round, s);
    }

    #[test]
    fn insert_into_d_from_the_menu_is_remembered_and_reattached() {
        use sdl2::keyboard::Keycode as K;
        let img = image_file("hdd-d.img");
        let mut emu = test_emu();
        let mut s = Settings::default();
        let mut m = Menu::new(None, None);
        m.toggle();
        // Drives page, row D: (the fourth drive), Insert.
        m.cat = 2;
        m.in_sidebar = false;
        m.row = 3;
        m.col = 0;
        m.key(K::Return, &mut emu, &mut s);
        assert_eq!(m.picker.as_ref().unwrap().drive, 3);
        // Type the path into the field and insert.
        m.key(K::Tab, &mut emu, &mut s);
        m.picker.as_mut().unwrap().path.clear();
        m.text(&img.display().to_string());
        m.key(K::Return, &mut emu, &mut s);
        assert!(m.picker.is_none());
        assert!(emu.machine.bus.disk.drives[3].is_some(), "{}", m.message);
        assert_eq!(emu.disk_paths[3], Some(img.clone()));
        assert_eq!(s.disks[3], Some(img.clone()));
        assert_eq!(s.recent_images.first(), Some(&img));

        // Next time the picker offers it first.
        m.key(K::Return, &mut emu, &mut s);
        assert_eq!(m.picker.as_ref().unwrap().files.first(), Some(&img));
        m.key(K::Escape, &mut emu, &mut s);

        // A fresh session puts it back in D:, unless the command line took D:.
        let mut emu2 = test_emu();
        let log = emu2.reattach_disks(&s.disks, &[]);
        assert!(log[0].contains("remembered"), "{log:?}");
        assert_eq!(emu2.disk_paths[3], Some(img.clone()));
        let mut emu3 = test_emu();
        assert!(emu3.reattach_disks(&s.disks, &[Drive::D]).is_empty());
        assert!(emu3.disk_paths[3].is_none());

        // Eject forgets it; a missing file is reported, not fatal.
        m.col = 1;
        m.key(K::Return, &mut emu, &mut s);
        assert!(emu.machine.bus.disk.drives[3].is_none());
        assert_eq!(s.disks[3], None);
        let gone = [None, None, None, Some(PathBuf::from("no/such/disk.img"))];
        let log = test_emu().reattach_disks(&gone, &[]);
        assert!(log[0].contains("not inserted"), "{log:?}");
    }

    #[test]
    fn menu_navigation_and_rendering() {
        use sdl2::keyboard::Keycode as K;
        let mut emu = test_emu();
        let mut s = Settings::default();
        let mut m = Menu::new(Some("http://127.0.0.1:8486/mcp".into()), Some("claude mcp add ...".into()));
        m.toggle();
        assert!(m.open);
        // Video page: Right enters the page, Enter toggles fullscreen.
        m.render(&emu, &s);
        save_png(&m.render(&emu, &s).to_vec(), "video.png");
        m.key(K::Right, &mut emu, &mut s);
        let fx = m.key(K::Return, &mut emu, &mut s);
        assert!(s.fullscreen && fx == vec![Effect::Video]);
        // Aspect: Right cycles.
        m.key(K::Down, &mut emu, &mut s);
        m.key(K::Right, &mut emu, &mut s);
        assert_eq!(s.aspect, Aspect::Square);
        // Emulation: the CPU switches to the fantasy R1024 and back.
        m.key(K::Tab, &mut emu, &mut s);
        m.key(K::Down, &mut emu, &mut s);
        m.key(K::Right, &mut emu, &mut s);
        m.key(K::Right, &mut emu, &mut s);
        assert_eq!(s.cpu, CpuChoice::R1024);
        assert_eq!(emu.machine.cpu.model, rusty486_core::cpu::CpuModel::R1024);
        assert!(emu.machine.clock_hz >= 100_000_000);
        m.key(K::Right, &mut emu, &mut s);
        assert_eq!(s.cpu, CpuChoice::I486Sx);
        assert_eq!(emu.machine.clock_hz, 33_000_000);
        // Speed up.
        m.key(K::Down, &mut emu, &mut s);
        m.key(K::Right, &mut emu, &mut s);
        assert_eq!(s.mhz, 40);
        assert_eq!(emu.machine.clock_hz, 40_000_000);
        save_png(&m.render(&emu, &s).to_vec(), "emulation.png");
        // Drives: the picker opens from Insert and Esc closes it.
        m.key(K::Tab, &mut emu, &mut s);
        m.key(K::Down, &mut emu, &mut s);
        m.key(K::Right, &mut emu, &mut s);
        save_png(&m.render(&emu, &s).to_vec(), "drives.png");
        m.key(K::Return, &mut emu, &mut s);
        assert!(m.picker.is_some());
        save_png(&m.render(&emu, &s).to_vec(), "picker.png");
        m.key(K::Escape, &mut emu, &mut s);
        assert!(m.picker.is_none());
        // Eject A: with the second button.
        m.key(K::Right, &mut emu, &mut s);
        m.key(K::Return, &mut emu, &mut s);
        assert!(emu.machine.bus.disk.drives[0].is_none());
        // Audio: volume by mouse on the slider.
        m.key(K::Tab, &mut emu, &mut s);
        m.key(K::Down, &mut emu, &mut s);
        m.render(&emu, &s);
        assert_eq!(m.cat, 3);
        let row_y = 22 + 24 + 40 + 34 + 10;
        m.click(SLIDER_X + SLIDER_W / 2, row_y, &mut emu, &mut s);
        assert_eq!(s.volume, 50);
        save_png(&m.render(&emu, &s).to_vec(), "audio.png");
        // Connection page and the close button.
        m.key(K::Tab, &mut emu, &mut s);
        m.key(K::Down, &mut emu, &mut s);
        save_png(&m.render(&emu, &s).to_vec(), "connection.png");
        m.click(40 + 560 - 20, 22 + 8, &mut emu, &mut s);
        assert!(!m.open);
    }

    #[test]
    fn blend_and_lerp() {
        assert_eq!(blend(0xFF000000, 0xFFFFFFFF), 0xFFFFFFFF);
        assert_eq!(lerp(0xFF000000, 0xFFFFFFFF, 255), 0xFFFFFFFF);
        assert_eq!(clip("abcdef", 4), "abc~");
        assert_eq!(clip_left("abcdef", 4), "~def");
    }
}
