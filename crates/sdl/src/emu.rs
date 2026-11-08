//! The emulator session owned by the UI thread: the machine plus everything
//! around it (pause state, disk files, debug log). The MCP server talks to it
//! by sending `Job`s that the main loop runs between frames.

use std::path::PathBuf;

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::machine::{Drive, RunResult};
use rusty486_core::Machine;
use tokio::sync::oneshot;

use crate::screen::Screen;

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
}

impl Emu {
    pub fn new(machine: Machine) -> Self {
        Emu {
            machine,
            screen: Screen::new(),
            paused: false,
            stop_reason: String::new(),
            disk_paths: Default::default(),
            log: String::new(),
            waiters: Vec::new(),
            blink_on: true,
        }
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

    pub fn insert_disk(&mut self, drive: Drive, path: PathBuf, image: DiskImage) {
        self.save_disk(drive).ok();
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
