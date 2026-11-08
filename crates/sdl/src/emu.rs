//! The emulator session owned by the UI thread: the machine plus everything
//! around it (pause state, disk files, debug log). The MCP server talks to it
//! by sending `Job`s that the main loop runs between frames.

use std::collections::VecDeque;
use std::path::PathBuf;

use rusty486_asm::AsmError;
use rusty486_core::devices::input::shift;

use crate::keymap::{KeyState, PcKey};
use crate::tabs::{self, Tabs};
use rusty486_core::devices::disk::DiskImage;
use rusty486_core::machine::{Drive, RunResult};
use rusty486_core::Machine;
use tokio::sync::oneshot;

use crate::editor::{self, Editor, Level};
use crate::settings::{CpuChoice, Effect, Menu, Settings};
use rusty486_core::cpu::CpuModel;
use std::time::{Duration, Instant};

/// The R1024's top clock speed.
pub const TURBO_MAX_HZ: u64 = 1_024_000_000;
/// The R1024's lowest clock speed (when the host can't keep up).
const TURBO_MIN_HZ: u64 = 33_000_000;
use crate::project::Project;
use crate::screen::Screen;

/// What the window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Machine,
    Editor,
}

/// Outcome of the last build, for status reports.
#[derive(Clone, Debug, Default)]
pub struct BuildSummary {
    pub ok: bool,
    pub bytes: usize,
    pub errors: Vec<AsmError>,
}

/// A unit of work from another thread, run on the UI thread with full access.
pub type Job = Box<dyn FnOnce(&mut Emu) + Send>;

/// Keep at most this much debug-port output for `debug_log`.
const LOG_LIMIT: usize = 64 * 1024;

/// Someone waiting for the machine to run until a cycle count (or to stop).
pub struct Waiter {
    pub until_cycle: u64,
    pub reply: oneshot::Sender<String>,
}

pub struct Emu {
    pub machine: Machine,
    pub screen: Screen,
    pub paused: bool,
    /// Why the machine last stopped, for status reports.
    pub stop_reason: String,
    /// Host files backing each drive (A, B, C, D), if any.
    pub disk_paths: [Option<PathBuf>; 4],
    pub log: String,
    pub waiters: Vec<Waiter>,
    pub blink_on: bool,
    pub view: View,
    pub project: Project,
    pub editor: Editor,
    editor_cells: Vec<u8>,
    pub last_build: Option<BuildSummary>,
    /// Host keyboard shift/lock state for the guest.
    pub keys: KeyState,
    /// Keystrokes queued by MCP `type_text`: (key, needs shift).
    pub typing: VecDeque<(PcKey, bool)>,
    /// Sprite, map, sound and disk tabs.
    pub tabs: Tabs,
    /// Host settings and the Settings window (Ctrl+F12).
    pub settings: Settings,
    pub menu: Option<Box<Menu>>,
    /// Window changes requested by Settings, applied by the main loop.
    pub effects: Vec<Effect>,
}

impl Emu {
    pub fn new(machine: Machine, project: Project, editor: Editor) -> Self {
        let tabs = Tabs::load(&project.dir);
        Emu {
            tabs,
            machine,
            screen: Screen::new(),
            paused: false,
            stop_reason: String::new(),
            disk_paths: Default::default(),
            log: String::new(),
            waiters: Vec::new(),
            blink_on: true,
            view: View::Machine,
            project,
            editor,
            editor_cells: vec![0; editor::COLS * editor::ROWS * 2],
            last_build: None,
            keys: KeyState::default(),
            typing: VecDeque::new(),
            settings: Settings::default(),
            menu: Some(Box::new(Menu::new(None, None))),
            effects: Vec::new(),
        }
    }

    /// Whether the Settings window is open.
    pub fn menu_open(&self) -> bool {
        self.menu.as_ref().is_some_and(|m| m.open)
    }

    /// Run `f` with the menu and settings borrowed alongside the session.
    pub fn with_menu<R>(&mut self, f: impl FnOnce(&mut Menu, &mut Emu, &mut Settings) -> R) -> R {
        let mut m = self.menu.take().expect("menu in use");
        let mut s = std::mem::take(&mut self.settings);
        let r = f(&mut m, self, &mut s);
        self.settings = s;
        self.menu = Some(m);
        r
    }

    /// The asset tab shown in the editor, if it isn't the code tab.
    pub fn which_tab(&self) -> Option<tabs::Which> {
        use crate::editor::Tab;
        match self.editor.tab {
            Tab::Code => None,
            Tab::Sprites => Some(tabs::Which::Sprites),
            Tab::Map => Some(tabs::Which::Map),
            Tab::Sound => Some(tabs::Which::Sound),
            Tab::Disks => Some(tabs::Which::Disks),
        }
    }

    /// A host key went down/up while the machine view has focus.
    pub fn host_key(&mut self, key: PcKey, down: bool, layout_char: Option<char>) {
        let modifier = self.keys.update(key, down);
        let ascii = if down && !modifier { self.keys.ascii(key, layout_char) } else { 0 };
        self.machine.key_event(key.code, key.ext, down, ascii, self.keys.flags, modifier);
    }

    /// Release held modifiers (when the machine view loses focus).
    pub fn release_modifiers(&mut self) {
        for (code, bit) in [(0x2A, shift::LSHIFT), (0x36, shift::RSHIFT), (0x1D, shift::CTRL), (0x38, shift::ALT)] {
            if self.keys.flags & bit != 0 {
                self.host_key(PcKey { code, ext: false }, false, None);
            }
        }
    }

    /// Press and release a key with the given extra shift flags (MCP input).
    pub fn tap_key(&mut self, key: PcKey, extra_flags: u8) {
        let state = KeyState { flags: self.keys.flags | extra_flags };
        let ascii = state.ascii(key, None);
        self.machine.key_event(key.code, key.ext, true, ascii, state.flags, false);
        self.machine.key_event(key.code, key.ext, false, 0, state.flags, false);
    }

    /// Feed queued MCP keystrokes without overflowing the guest's buffers.
    pub fn feed_typing(&mut self) {
        for _ in 0..2 {
            if self.machine.bus.input.pending() > 4 {
                break;
            }
            let Some((key, shifted)) = self.typing.pop_front() else { break };
            self.tap_key(key, if shifted { shift::LSHIFT } else { 0 });
        }
    }

    /// Draw the current view (or a specific one) into `self.screen`.
    pub fn render(&mut self, view: View) {
        match view {
            View::Machine => self.screen.render(&self.machine, self.blink_on),
            View::Editor => {
                let cursor = self.editor.draw(&mut self.editor_cells, &self.project.name);
                let which = self.which_tab();
                if let Some(w) = which {
                    let mut cells = std::mem::take(&mut self.editor_cells);
                    tabs::draw(w, self, &mut cells);
                    self.editor_cells = cells;
                }
                self.screen.render_cells(&self.editor_cells, cursor, self.blink_on, false);
                if let Some(w) = which {
                    tabs::overlay(w, &self.tabs, &mut self.screen.pixels);
                }
            }
        }
    }

    /// Save the editor buffer and assemble it. Errors are shown in the editor.
    pub fn build(&mut self) -> (BuildSummary, Option<Vec<u8>>) {
        if self.editor.dirty {
            if let Err(e) = self.editor.save() {
                self.editor.set_status(format!("Cannot save: {e}"), Level::Error);
            }
        }
        let build = self.project.build(&self.editor.text());
        let summary = match &build.output {
            Ok(out) => {
                self.editor.clear_errors();
                self.editor.set_status(
                    format!("Built {} bytes -> {}", out.bytes.len(), self.project.disk_path().display()),
                    Level::Info,
                );
                BuildSummary { ok: true, bytes: out.bytes.len(), errors: vec![] }
            }
            Err(errors) => {
                self.editor.set_errors(errors);
                BuildSummary { ok: false, bytes: 0, errors: errors.clone() }
            }
        };
        self.last_build = Some(summary.clone());
        (summary, build.disk)
    }

    /// Build, then boot the program's floppy in drive A. On errors, show the editor.
    pub fn build_and_run(&mut self) -> BuildSummary {
        let (summary, disk) = self.build();
        match disk {
            Some(disk) => {
                let path = self.project.disk_path();
                self.insert_disk(Drive::A, path, DiskImage::new(disk));
                self.machine.reset();
                self.resume();
                self.view = View::Machine;
            }
            None => self.view = View::Editor,
        }
        summary
    }

    /// Set the CPU model and clock from the settings (takes effect fully
    /// after a reset, when the BIOS sees the new CPU).
    pub fn apply_cpu(&mut self, s: &Settings) {
        match s.cpu {
            CpuChoice::I486Sx => {
                self.machine.cpu.model = CpuModel::I486Sx;
                self.machine.clock_hz = s.mhz as u64 * 1_000_000;
            }
            CpuChoice::R1024 => {
                self.machine.cpu.model = CpuModel::R1024;
                self.machine.clock_hz = self.machine.clock_hz.clamp(100_000_000, TURBO_MAX_HZ);
            }
        }
    }

    /// Whether the clock adapts to the host's speed (R1024 turbo).
    pub fn turbo(&self) -> bool {
        self.settings.cpu == CpuChoice::R1024
    }

    /// Turbo: emulate `dt` of real time, but spend at most about
    /// three quarters of it doing so. The clock rate follows what the host
    /// sustains, up to 1024 MHz, so emulated time keeps pace with real time.
    pub fn run_turbo(&mut self, dt: Duration) {
        let budget = dt.mul_f64(0.75).clamp(Duration::from_millis(4), Duration::from_millis(25));
        let start = Instant::now();
        let hz = self.machine.clock_hz;
        let want = (dt.as_secs_f64() * hz as f64) as u64;
        let chunk = (hz / 1000).max(10_000); // about 1 ms of emulated time
        let c0 = self.machine.cpu.cycles;
        let mut done = 0;
        let mut behind = false;
        while done < want && !self.paused {
            self.run(chunk.min(want - done));
            done = self.machine.cpu.cycles - c0;
            if start.elapsed() >= budget {
                behind = done < want;
                break;
            }
        }
        if self.paused || dt.is_zero() {
            return;
        }
        let new = if behind {
            // What the host managed per second of real time, smoothed.
            let sustained = done as f64 / dt.as_secs_f64();
            0.7 * hz as f64 + 0.3 * sustained
        } else {
            hz as f64 * 1.05
        };
        self.machine.clock_hz = (new as u64).clamp(TURBO_MIN_HZ, TURBO_MAX_HZ);
    }

    /// Advance emulation by `cycles` (unless paused), then service waiters.
    pub fn run(&mut self, cycles: u64) {
        if !self.paused {
            match self.machine.run_cycles(cycles) {
                RunResult::Completed => {}
                RunResult::Breakpoint(addr) => self.pause(format!("breakpoint at {addr:05X}h")),
                RunResult::Shutdown => self.pause("CPU shutdown (triple fault)".into()),
            }
        }
        let log = self.machine.drain_debug();
        if !log.is_empty() {
            let text = String::from_utf8_lossy(&log);
            print!("{text}");
            self.log.push_str(&text);
            if self.log.len() > LOG_LIMIT {
                let cut = self.log.len() - LOG_LIMIT;
                let cut = (cut..self.log.len()).find(|&i| self.log.is_char_boundary(i)).unwrap();
                self.log.drain(..cut);
            }
        }
        self.service_waiters();
    }

    pub fn pause(&mut self, reason: String) {
        self.paused = true;
        self.stop_reason = reason;
    }

    pub fn resume(&mut self) {
        self.paused = false;
        self.stop_reason.clear();
    }

    /// Answer waiters whose target was reached, or all of them if we stopped.
    fn service_waiters(&mut self) {
        if self.waiters.is_empty() {
            return;
        }
        let cycles = self.machine.cpu.cycles;
        let paused = self.paused;
        let reason = self.stop_reason.clone();
        let (done, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut self.waiters)
            .into_iter()
            .partition(|w| paused || cycles >= w.until_cycle);
        self.waiters = pending;
        for w in done {
            let msg = if paused && cycles < w.until_cycle {
                format!("stopped early: {reason}")
            } else {
                "completed".to_string()
            };
            let _ = w.reply.send(msg);
        }
    }

    /// Put back the disks remembered in the settings (A:, B:, C:, D:),
    /// except in drives listed in `skip`. Returns a line per drive tried.
    pub fn reattach_disks(&mut self, disks: &[Option<PathBuf>; 4], skip: &[Drive]) -> Vec<String> {
        let mut log = Vec::new();
        for (i, drive) in [Drive::A, Drive::B, Drive::C, Drive::D].into_iter().enumerate() {
            let Some(path) = disks[i].clone() else { continue };
            if skip.contains(&drive) {
                continue;
            }
            let letter = ["A", "B", "C", "D"][i];
            match std::fs::read(&path) {
                Ok(data) => {
                    log.push(format!("{letter}: {} (remembered)", path.display()));
                    self.insert_disk(drive, path, DiskImage::new(data));
                }
                Err(e) => log.push(format!("{letter}: {}: {e} (not inserted)", path.display())),
            }
        }
        log
    }

    pub fn insert_disk(&mut self, drive: Drive, path: PathBuf, image: DiskImage) {
        // Save the old disk only if it's a different file.
        if self.disk_paths[drive as usize].as_ref() != Some(&path) {
            self.save_disk(drive).ok();
        }
        self.machine.insert_disk(drive, image);
        self.disk_paths[drive as usize] = Some(path);
    }

    /// Write a drive's image back to its file if the guest changed it.
    /// Returns whether anything was written.
    pub fn save_disk(&mut self, drive: Drive) -> std::io::Result<bool> {
        let (Some(path), Some(disk)) = (
            self.disk_paths[drive as usize].as_ref(),
            self.machine.bus.disk.drives[drive as usize].as_mut(),
        ) else {
            return Ok(false);
        };
        if !disk.dirty {
            return Ok(false);
        }
        std::fs::write(path, &disk.data)?;
        disk.dirty = false;
        Ok(true)
    }

    pub fn save_all_disks(&mut self) {
        for drive in [Drive::A, Drive::B, Drive::C, Drive::D] {
            match self.save_disk(drive) {
                Ok(true) => println!("saved {}", self.disk_paths[drive as usize].as_ref().unwrap().display()),
                Ok(false) => {}
                Err(e) => eprintln!("failed to save drive {drive:?}: {e}"),
            }
        }
    }
}
