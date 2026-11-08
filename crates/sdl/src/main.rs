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
//! project, F11 = reset, Pause = pause/resume, Ctrl+F12 = Settings,
//! Alt+Enter = fullscreen. Other keys go to the machine.

mod editor;
mod emu;
mod font;
mod keymap;
mod mcp;
mod project;
mod screen;
mod settings;
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
use sdl2::render::{BlendMode, Canvas, Texture, TextureCreator};
use sdl2::video::{FullscreenType, Window, WindowContext};

use editor::{Action, Editor, Level};
use emu::{Emu, Job, View};
use project::Project;
use screen::{SCREEN_H, SCREEN_W};
use settings::{Aspect, Effect, Menu, Settings, UI_H, UI_W};

use rusty486_firmware::BIOS;

const USAGE: &str = "usage: rusty486 [--project DIR] [--fda IMG] [--fdb IMG] [--hda IMG] [--hdb IMG] \
                     [--mhz N] [--mcp-port N] [--no-mcp] [--mute] [IMG]";

/// Logical display width; the height depends on the aspect setting.
const VIEW_W: u32 = SCREEN_W;

struct Args {
    disks: Vec<(Drive, PathBuf)>,
    mhz: Option<u64>,
    mute: bool,
    mcp_port: Option<u16>,
    project: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        disks: Vec::new(),
        mhz: None,
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
            "--mhz" => args.mhz = Some(value()?.parse().map_err(|e| format!("--mhz: {e}"))?),
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

/// Apply fullscreen, aspect, integer scaling and the scaling filter.
fn apply_video(canvas: &mut Canvas<Window>, s: &Settings) {
    let mode = if s.fullscreen { FullscreenType::Desktop } else { FullscreenType::Off };
    if canvas.window().fullscreen_state() != mode {
        let _ = canvas.window_mut().set_fullscreen(mode);
    }
    match s.aspect {
        Aspect::Stretch => {
            let _ = canvas.set_logical_size(0, 0);
        }
        _ => {
            let _ = canvas.set_logical_size(VIEW_W, s.view_h());
        }
    }
    let _ = canvas.set_integer_scale(s.integer_scale && s.aspect != Aspect::Stretch);
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", if s.smooth { "1" } else { "0" });
}

fn set_window_scale(canvas: &mut Canvas<Window>, s: &Settings) {
    if !s.fullscreen {
        let _ = canvas.window_mut().set_size(VIEW_W * s.window_scale, s.view_h() * s.window_scale);
        canvas.window_mut().set_position(sdl2::video::WindowPos::Centered, sdl2::video::WindowPos::Centered);
    }
}

fn new_texture(creator: &TextureCreator<WindowContext>, w: u32, h: u32) -> Result<Texture<'_>, String> {
    creator.create_texture_streaming(PixelFormatEnum::ARGB8888, w, h).map_err(|e| e.to_string())
}

fn run(args: Args) -> Result<(), String> {
    let mut settings = Settings::load();
    if let Some(mhz) = args.mhz {
        settings.mhz = mhz as u32;
    }
    let mut machine = Machine::new(BIOS.to_vec());
    machine.clock_hz = settings.mhz as u64 * 1_000_000;
    let project = Project::open_or_create(&args.project)
        .map_err(|e| format!("project {}: {e}", args.project.display()))?;
    let editor = Editor::open(project.entry_path())
        .map_err(|e| format!("{}: {e}", project.entry_path().display()))?;
    println!("Project: {} ({})", project.name, project.dir.display());
    machine.set_time(chrono::Local::now().naive_local().and_utc().timestamp());
    let mut emu = Emu::new(machine, project, editor);
    emu.settings = settings;

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
    let mcp_info = args.mcp_port.and_then(|port| start_mcp(port, job_tx));
    emu.menu = Some(Box::new(Menu::new(mcp_info.as_ref().map(|m| m.0.clone()), mcp_info.map(|m| m.1))));

    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let window = video
        .window("rusty486", VIEW_W * emu.settings.window_scale, emu.settings.view_h() * emu.settings.window_scale)
        .position_centered()
        .resizable()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas().present_vsync().build().map_err(|e| e.to_string())?;
    // Text mode pixels are taller than wide: by default 640x400 is shown at 4:3.
    apply_video(&mut canvas, &emu.settings);
    let creator = canvas.texture_creator();
    let mut tex_size = (SCREEN_W, SCREEN_H);
    let mut texture = new_texture(&creator, SCREEN_W, SCREEN_H)?;
    // The Settings overlay is always drawn with sharp pixels.
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "0");
    let mut overlay = new_texture(&creator, UI_W as u32, UI_H as u32)?;
    overlay.set_blend_mode(BlendMode::Blend);
    apply_video(&mut canvas, &emu.settings);
    let mut smooth_now = emu.settings.smooth;
    let mut frame: Vec<u32> = Vec::new();
    let mut events = sdl.event_pump()?;
    let clipboard = video.clipboard();
    // Audio: a 44.1 kHz mono queue fed from the machine each frame. --mute
    // silences this session without changing the saved setting.
    let force_mute = args.mute;
    let audio_queue = {
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
    'main: loop {
        // Mouse coordinates arrive in logical pixels (SDL scales them), except
        // when stretching, where they are window pixels: map both to a
        // 640 x view_h view.
        let view_h = emu.settings.view_h();
        let out = canvas.output_size().unwrap_or((VIEW_W, view_h));
        let stretch = emu.settings.aspect == Aspect::Stretch;
        let to_view = |x: i32, y: i32| -> (i32, i32) {
            if stretch {
                (x * VIEW_W as i32 / out.0.max(1) as i32, y * view_h as i32 / out.1.max(1) as i32)
            } else {
                (x, y)
            }
        };
        // View position -> text cell, and -> 640x400 frame pixel.
        let cell = |x: i32, y: i32| {
            let (x, y) = to_view(x, y);
            let col = (x.max(0) as u32 / 8).min(79) as usize;
            let row = ((y.max(0) as u32 * SCREEN_H / view_h) / 16).min(24) as usize;
            (col, row)
        };
        let pixel = |x: i32, y: i32| {
            let (x, y) = to_view(x, y);
            let px = (x.max(0) as u32).min(639) as usize;
            let py = ((y.max(0) as u32) * SCREEN_H / view_h).min(399) as usize;
            (px, py)
        };
        let to_ui = |x: i32, y: i32| {
            let (x, y) = to_view(x, y);
            (x * UI_W as i32 / VIEW_W as i32, y * UI_H as i32 / view_h as i32)
        };
        let mut effects: Vec<Effect> = Vec::new();

        for event in events.poll_iter() {
            // Host shortcuts: Settings and fullscreen.
            if let Event::KeyDown { keycode: Some(k), keymod, .. } = &event {
                let ctrl = keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
                let alt = keymod.intersects(Mod::LALTMOD | Mod::RALTMOD);
                if ctrl && *k == Keycode::F12 {
                    emu.release_modifiers();
                    if let Some(m) = emu.menu.as_mut() {
                        m.toggle();
                    }
                    continue;
                }
                if alt && matches!(*k, Keycode::Return | Keycode::KpEnter) {
                    emu.release_modifiers();
                    emu.settings.fullscreen = !emu.settings.fullscreen;
                    emu.settings.save();
                    effects.push(Effect::Video);
                    continue;
                }
            }
            // While Settings is open it gets all input.
            if emu.menu_open() {
                match event {
                    Event::Quit { .. } => break 'main,
                    Event::KeyDown { keycode: Some(k), .. } => {
                        let fx = emu.with_menu(|m, e, s| m.key(k, e, s));
                        effects.extend(fx);
                    }
                    Event::TextInput { text, .. } => emu.with_menu(|m, _, _| m.text(&text)),
                    Event::MouseMotion { x, y, mousestate, .. } => {
                        let (ux, uy) = to_ui(x, y);
                        emu.with_menu(|m, e, s| m.mouse_move(ux, uy, mousestate.left(), e, s));
                    }
                    Event::MouseButtonDown { mouse_btn: MouseButton::Left, x, y, .. } => {
                        let (ux, uy) = to_ui(x, y);
                        let fx = emu.with_menu(|m, e, s| m.click(ux, uy, e, s));
                        effects.extend(fx);
                    }
                    Event::MouseWheel { .. } => {}
                    _ => {}
                }
                continue;
            }
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
                    let (px, py) = pixel(x, y);
                    emu.machine.mouse_move(px as u16, py as u16);
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

        // Effects of Settings changes that need the window (also from MCP).
        effects.append(&mut emu.effects);
        for fx in effects {
            match fx {
                Effect::Video => apply_video(&mut canvas, &emu.settings),
                Effect::WindowScale => set_window_scale(&mut canvas, &emu.settings),
                Effect::Copy(text) => {
                    let _ = clipboard.set_clipboard_text(&text);
                }
            }
        }
        if emu.settings.smooth != smooth_now {
            // The filter is fixed when a texture is created.
            sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", if emu.settings.smooth { "1" } else { "0" });
            texture = new_texture(&creator, tex_size.0, tex_size.1)?;
            smooth_now = emu.settings.smooth;
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
        if !(emu.menu_open() && emu.settings.pause_in_menu) {
            emu.run((dt.as_secs_f64() * emu.machine.clock_hz as f64) as u64);
        }
        let mut samples = emu.machine.take_audio();
        if emu.view == View::Editor && emu.which_tab() == Some(tabs::Which::Sound) {
            if samples.is_empty() {
                samples = vec![0; (dt.as_secs_f64() * 44_100.0) as usize];
            }
            emu.tabs.sound.render(&mut samples);
        }
        if let Some(q) = &audio_queue {
            // Keep latency bounded: drop audio if more than ~200 ms is queued.
            let silent = emu.settings.muted || force_mute || (emu.menu_open() && emu.settings.pause_in_menu);
            if q.size() < 44_100 * 2 / 5 && !emu.paused && !silent {
                if emu.settings.volume < 100 {
                    let v = emu.settings.volume as i32;
                    for s in &mut samples {
                        *s = (*s as i32 * v / 100) as i16;
                    }
                }
                let _ = q.queue_audio(&samples);
            }
        }

        emu.blink_on = (start.elapsed().as_millis() / 266) % 2 == 0;
        emu.render(emu.view);
        let (w, h) = (emu.screen.width, emu.screen.height);
        // Scanlines: every source line twice, the second one darkened.
        let size = if emu.settings.scanlines { (w, h * 2) } else { (w, h) };
        if size != tex_size {
            texture = new_texture(&creator, size.0, size.1)?;
            tex_size = size;
        }
        if emu.settings.scanlines {
            frame.resize((size.0 * size.1) as usize, 0);
            let w = w as usize;
            for y in 0..h as usize {
                let src = &emu.screen.pixels[y * w..(y + 1) * w];
                frame[2 * y * w..(2 * y + 1) * w].copy_from_slice(src);
                for (d, p) in frame[(2 * y + 1) * w..(2 * y + 2) * w].iter_mut().zip(src) {
                    *d = 0xFF00_0000 | (p >> 1) & 0x7F7F7F;
                }
            }
            let bytes = unsafe { std::slice::from_raw_parts(frame.as_ptr().cast::<u8>(), frame.len() * 4) };
            texture.update(None, bytes, (size.0 * 4) as usize).map_err(|e| e.to_string())?;
        } else {
            texture.update(None, emu.screen.bytes(), (size.0 * 4) as usize).map_err(|e| e.to_string())?;
        }
        let dest = if stretch { None } else { Some(Rect::new(0, 0, VIEW_W, view_h)) };
        canvas.set_draw_color(sdl2::pixels::Color::RGB(0, 0, 0));
        canvas.clear();
        canvas.copy(&texture, None, dest)?;
        if emu.menu_open() {
            let mut m = emu.menu.take().unwrap();
            let pixels = m.render(&emu, &emu.settings).to_vec();
            emu.menu = Some(m);
            let bytes = unsafe { std::slice::from_raw_parts(pixels.as_ptr().cast::<u8>(), pixels.len() * 4) };
            overlay.update(None, bytes, UI_W * 4).map_err(|e| e.to_string())?;
            canvas.copy(&overlay, None, dest)?;
        }
        canvas.present();

        // Show the effective emulated clock speed in the title once a second.
        let elapsed = speed_window.0.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let mhz =
                (emu.machine.cpu.cycles - speed_window.1) as f64 / elapsed.as_secs_f64() / 1e6;
            let view = if emu.view == View::Editor { " - editor" } else { "" };
            let new_title = if emu.paused {
                format!("rusty486 - paused ({}){view} - Ctrl+F12 settings", emu.stop_reason)
            } else {
                format!("rusty486 - {mhz:.1} MHz{view} - Ctrl+F12 settings")
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

/// Start the MCP server; returns its URL and the command that connects
/// Claude Code to it.
fn start_mcp(port: u16, jobs: mpsc::Sender<Job>) -> Option<(String, String)> {
    let (token, token_path) = match mcp::load_or_create_token() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("MCP disabled: cannot create token: {e}");
            return None;
        }
    };
    match mcp::start(port, token.clone(), jobs) {
        Ok(url) => {
            println!("MCP server listening on {url} (token in {})", token_path.display());
            let command = format!(
                "claude mcp add --scope user --transport http rusty486 {url} --header \"Authorization: Bearer {token}\""
            );
            println!("Connect Claude Code with:\n  {command}");
            Some((url.to_string(), command))
        }
        Err(e) => {
            eprintln!("MCP disabled: {e}");
            None
        }
    }
}
