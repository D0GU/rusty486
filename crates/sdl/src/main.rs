//! rusty486 desktop frontend.
//!
//! ```text
//! rusty486 [--fda IMG] [--fdb IMG] [--hda IMG] [--hdb IMG] [--mhz N]
//!          [--mcp-port N] [--no-mcp] [IMG]
//! ```
//!
//! Drive C: is a persistent TinyDOS hard disk (%USERPROFILE%\rusty486\hdd.img,
//! created on first run) unless --hda is given. Disk images the guest writes
//! to are saved back to their files on exit.
//!
//! Keys: F12 = machine -> editor (Esc goes back), Ctrl+F9 = build and run the
//! project, F11 = reset, Pause = pause/resume. Other keys go to the machine.

mod editor;
mod emu;
mod font;
mod keymap;
mod mcp;
mod project;
mod screen;
mod tabs;

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::machine::{Drive, Machine};
use sdl2::event::{Event, WindowEvent};
use sdl2::keyboard::{Keycode, Mod};
use sdl2::mouse::MouseButton;
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;

use editor::{Action, Editor, Level};
use emu::{Emu, Job, View};
use project::Project;
use screen::{SCREEN_H, SCREEN_W};

use rusty486_firmware::BIOS;

const USAGE: &str = "usage: rusty486 [--project DIR] [--fda IMG] [--fdb IMG] [--hda IMG] [--hdb IMG] \
                     [--mhz N] [--mcp-port N] [--no-mcp] [--mute] [IMG]";

/// Logical display size: 640x400 text shown at 4:3.
const VIEW_W: u32 = SCREEN_W;
const VIEW_H: u32 = SCREEN_W * 3 / 4;

struct Args {
    disks: Vec<(Drive, PathBuf)>,
    mhz: u64,
    mute: bool,
    mcp_port: Option<u16>,
    project: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        disks: Vec::new(),
        mhz: 33,
        mcp_port: Some(mcp::DEFAULT_PORT),
        project: project::default_dir(),
        mute: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--fda" => args.disks.push((Drive::A, value()?.into())),
            "--fdb" => args.disks.push((Drive::B, value()?.into())),
            "--hda" => args.disks.push((Drive::C, value()?.into())),
            "--hdb" => args.disks.push((Drive::D, value()?.into())),
            "--mhz" => args.mhz = value()?.parse().map_err(|e| format!("--mhz: {e}"))?,
            "--mcp-port" => {
                args.mcp_port = Some(value()?.parse().map_err(|e| format!("--mcp-port: {e}"))?)
            }
            "--no-mcp" => args.mcp_port = None,
            "--mute" => args.mute = true,
            "--project" => args.project = value()?.into(),
            "-h" | "--help" => return Err(USAGE.into()),
            s if s.starts_with('-') => return Err(format!("unknown option {s}\n{USAGE}")),
            path => args.disks.push((Drive::A, path.into())),
        }
    }
    Ok(args)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(args) {
        eprintln!("rusty486: {e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    let mut machine = Machine::new(BIOS.to_vec());
    machine.clock_hz = args.mhz * 1_000_000;
    let project = Project::open_or_create(&args.project)
        .map_err(|e| format!("project {}: {e}", args.project.display()))?;
    let editor = Editor::open(project.entry_path())
        .map_err(|e| format!("{}: {e}", project.entry_path().display()))?;
    println!("Project: {} ({})", project.name, project.dir.display());
    machine.set_time(chrono::Local::now().naive_local().and_utc().timestamp());
    let mut emu = Emu::new(machine, project, editor);

    if !args.disks.iter().any(|(d, _)| *d == Drive::C) {
        // The persistent TinyDOS hard disk.
        let path = project::hdd_path();
        let data = project::open_or_create_hdd(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("C: {}", path.display());
        emu.insert_disk(Drive::C, path, DiskImage::new(data));
    }
    for (drive, path) in &args.disks {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        emu.insert_disk(*drive, path.clone(), DiskImage::new(data));
    }

    let (job_tx, job_rx) = mpsc::channel::<Job>();
    if let Some(port) = args.mcp_port {
        start_mcp(port, job_tx);
    }

    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let window = video
        .window("rusty486", SCREEN_W * 2, SCREEN_H * 2 * 6 / 5)
        .position_centered()
        .resizable()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas().present_vsync().build().map_err(|e| e.to_string())?;
    // Text mode pixels are taller than wide: show 640x400 at 4:3.
    canvas.set_logical_size(VIEW_W, VIEW_H).map_err(|e| e.to_string())?;
    let creator = canvas.texture_creator();
    let mut tex_size = (SCREEN_W, SCREEN_H);
    let mut texture = creator
        .create_texture_streaming(PixelFormatEnum::ARGB8888, SCREEN_W, SCREEN_H)
        .map_err(|e| e.to_string())?;
    let mut events = sdl.event_pump()?;
    let clipboard = video.clipboard();
    // Audio: a 44.1 kHz mono queue fed from the machine each frame.
    let audio_queue = if args.mute {
        None
    } else {
        sdl.audio().ok().and_then(|a| {
            let spec = sdl2::audio::AudioSpecDesired { freq: Some(44_100), channels: Some(1), samples: Some(1024) };
            let q: Option<sdl2::audio::AudioQueue<i16>> = a.open_queue(None, &spec).ok();
            if let Some(q) = &q {
                q.resume();
            }
            q
        })
    };
    video.text_input().start();

    let start = Instant::now();
    let mut last = Instant::now();
    let mut last_disk_check = Instant::now();
    let mut speed_window = (Instant::now(), emu.machine.cpu.cycles);
    let mut title = String::new();
    let mut mouse_buttons = 0u8;
    // Mouse position (logical pixels) -> text cell.
    let cell = |x: i32, y: i32| {
        let col = (x.max(0) as u32 / 8).min(79) as usize;
        let row = ((y.max(0) as u32 * SCREEN_H / VIEW_H) / 16).min(24) as usize;
        (col, row)
    };
    // Mouse position (logical pixels) -> 640x400 frame pixel.
    let pixel = |x: i32, y: i32| {
        let px = (x.max(0) as u32).min(639) as usize;
        let py = ((y.max(0) as u32) * SCREEN_H / VIEW_H).min(399) as usize;
        (px, py)
    };

    'main: loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'main,
                Event::KeyDown { keycode, scancode, keymod, .. } => {
                    let ctrl = keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
                    match (emu.view, keycode) {
                        (_, Some(Keycode::F11)) => {
                            emu.machine.reset();
                            emu.resume();
                        }
                        (_, Some(Keycode::Pause)) => {
                            if emu.paused {
                                emu.resume();
                            } else {
                                emu.pause("paused from keyboard".into());
                            }
                        }
                        (View::Machine, Some(Keycode::F12)) => {
                            emu.release_modifiers();
                            emu.view = View::Editor;
                        }
                        (View::Machine, Some(Keycode::F9)) if ctrl => {
                            emu.release_modifiers();
                            emu.build_and_run();
                        }
                        (View::Machine, _) => {
                            if let Some(k) = scancode.and_then(keymap::pc_key) {
                                emu.host_key(k, true, keymap::layout_char(keycode));
                            }
                        }
                        (View::Editor, Some(key)) => {
                            let action = emu.editor.key(key, keymod, &clipboard);
                            if action == Action::None {
                                if let Some(w) = emu.which_tab() {
                                    tabs::key(w, &mut emu, key, keymod);
                                }
                            }
                            handle_action(&mut emu, action);
                        }
                        _ => {}
                    }
                }
                Event::KeyUp { scancode: Some(sc), .. } if emu.view == View::Machine => {
                    if let Some(k) = keymap::pc_key(sc) {
                        emu.host_key(k, false, None);
                    }
                }
                Event::Window { win_event: WindowEvent::FocusLost, .. } => emu.release_modifiers(),
                Event::MouseMotion { x, y, .. } if emu.view == View::Machine => {
                    let px = (x.max(0) as u32).min(SCREEN_W - 1) as u16;
                    let py = ((y.max(0) as u32) * SCREEN_H / VIEW_H).min(SCREEN_H - 1) as u16;
                    emu.machine.mouse_move(px, py);
                }
                Event::MouseButtonDown { mouse_btn, .. } if emu.view == View::Machine => {
                    mouse_buttons |= button_bit(mouse_btn);
                    emu.machine.mouse_buttons(mouse_buttons);
                }
                Event::MouseButtonUp { mouse_btn, .. } if emu.view == View::Machine => {
                    mouse_buttons &= !button_bit(mouse_btn);
                    emu.machine.mouse_buttons(mouse_buttons);
                }
                Event::TextInput { text, .. } if emu.view == View::Editor => {
                    match emu.which_tab() {
                        Some(w) => tabs::text(w, &mut emu, &text),
                        None => emu.editor.text_input(&text),
                    }
                }
                Event::MouseButtonDown { mouse_btn, x, y, .. } if emu.view == View::Editor => {
                    let (col, row) = cell(x, y);
                    match emu.which_tab() {
                        Some(w) if row > 0 => {
                            let (px, py) = pixel(x, y);
                            tabs::mouse(w, &mut emu, px, py, mouse_btn == MouseButton::Left);
                        }
                        _ if mouse_btn == MouseButton::Left => {
                            let shift = sdl.keyboard().mod_state().intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
                            emu.editor.mouse_down(col, row, shift);
                        }
                        _ => {}
                    }
                }
                Event::MouseMotion { x, y, mousestate, .. } if emu.view == View::Editor => {
                    let (col, row) = cell(x, y);
                    match emu.which_tab() {
                        Some(w) if mousestate.left() && row > 0 => {
                            let (px, py) = pixel(x, y);
                            tabs::mouse(w, &mut emu, px, py, true);
                        }
                        Some(_) => {}
                        None => emu.editor.mouse_drag(col, row),
                    }
                }
                Event::MouseButtonUp { mouse_btn: MouseButton::Left, .. } => emu.editor.mouse_up(),
                Event::MouseWheel { y, .. } if emu.view == View::Editor => emu.editor.wheel(y),
                _ => {}
            }
        }

        emu.feed_typing();

        // Pick up source edits made outside rusty486 (e.g. by Claude Code).
        if last_disk_check.elapsed() >= Duration::from_millis(500) {
            emu.editor.check_disk();
            last_disk_check = Instant::now();
        }

        // Requests from the MCP server.
        while let Ok(job) = job_rx.try_recv() {
            job(&mut emu);
        }

        // Run as many cycles as real time has elapsed (capped to avoid a
        // spiral of death if the host falls behind).
        let now = Instant::now();
        let dt = now.duration_since(last).min(Duration::from_millis(100));
        last = now;
        emu.run((dt.as_secs_f64() * emu.machine.clock_hz as f64) as u64);
        let mut samples = emu.machine.take_audio();
        if emu.view == View::Editor && emu.which_tab() == Some(tabs::Which::Sound) {
            if samples.is_empty() {
                samples = vec![0; (dt.as_secs_f64() * 44_100.0) as usize];
            }
            emu.tabs.sound.render(&mut samples);
        }
        if let Some(q) = &audio_queue {
            // Keep latency bounded: drop audio if more than ~200 ms is queued.
            if q.size() < 44_100 * 2 / 5 && !emu.paused {
                let _ = q.queue_audio(&samples);
            }
        }

        emu.blink_on = (start.elapsed().as_millis() / 266) % 2 == 0;
        emu.render(emu.view);
        let size = (emu.screen.width, emu.screen.height);
        if size != tex_size {
            texture = creator
                .create_texture_streaming(PixelFormatEnum::ARGB8888, size.0, size.1)
                .map_err(|e| e.to_string())?;
            tex_size = size;
        }
        texture
            .update(None, emu.screen.bytes(), (size.0 * 4) as usize)
            .map_err(|e| e.to_string())?;
        canvas.clear();
        canvas.copy(&texture, None, Rect::new(0, 0, VIEW_W, VIEW_H))?;
        canvas.present();

        // Show the effective emulated clock speed in the title once a second.
        let elapsed = speed_window.0.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let mhz =
                (emu.machine.cpu.cycles - speed_window.1) as f64 / elapsed.as_secs_f64() / 1e6;
            let view = if emu.view == View::Editor { " - editor" } else { "" };
            let new_title = if emu.paused {
                format!("rusty486 - paused ({}){view}", emu.stop_reason)
            } else {
                format!("rusty486 - {mhz:.1} MHz{view}")
            };
            if new_title != title {
                let _ = canvas.window_mut().set_title(&new_title);
                title = new_title;
            }
            speed_window = (Instant::now(), emu.machine.cpu.cycles);
        }
    }

    emu.save_all_disks();
    if emu.editor.dirty {
        match emu.editor.save() {
            Ok(()) => println!("saved {}", emu.editor.path.display()),
            Err(e) => eprintln!("failed to save {}: {e}", emu.editor.path.display()),
        }
    }
    Ok(())
}

/// Input-device button bit (1 left, 2 right, 4 middle).
fn button_bit(b: MouseButton) -> u8 {
    match b {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 4,
        _ => 0,
    }
}

fn handle_action(emu: &mut Emu, action: Action) {
    match action {
        Action::None => {}
        Action::Machine => emu.view = View::Machine,
        Action::Save if emu.which_tab().is_some() => {
            let dir = emu.project.dir.clone();
            let msg = emu.tabs.save(&dir);
            emu.editor.set_status(msg, Level::Info);
        }
        Action::Save => match emu.editor.save() {
            Ok(()) => emu.editor.set_status(format!("Saved {}", emu.editor.path.display()), Level::Info),
            Err(e) => emu.editor.set_status(format!("Cannot save: {e}"), Level::Error),
        },
        Action::Build => {
            emu.build();
        }
        Action::Run => {
            emu.build_and_run();
        }
        Action::Reload => match emu.editor.reload() {
            Ok(()) => emu.editor.set_status("Reloaded from disk", Level::Info),
            Err(e) => emu.editor.set_status(format!("Cannot reload: {e}"), Level::Error),
        },
    }
}

fn start_mcp(port: u16, jobs: mpsc::Sender<Job>) {
    let (token, token_path) = match mcp::load_or_create_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("MCP disabled: cannot create token: {e}");
            return;
        }
    };
    match mcp::start(port, token.clone(), jobs) {
        Ok(url) => {
            println!("MCP server listening on {url} (token in {})", token_path.display());
            println!(
                "Connect Claude Code with:\n  claude mcp add --scope user --transport http rusty486 {url} \
                 --header \"Authorization: Bearer {token}\""
            );
        }
        Err(e) => eprintln!("MCP disabled: {e}"),
    }
}
