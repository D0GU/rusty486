//! rusty486 desktop frontend.
//!
//! ```text
//! rusty486 [--fda IMG] [--fdb IMG] [--hda IMG] [--hdb IMG] [--mhz N]
//!          [--mcp-port N] [--no-mcp] [IMG]
//! ```
//!
//! With no disks, it boots the built-in milestone 1 demo floppy. Disk images
//! the guest writes to are saved back to their files on exit.
//!
//! Keys: F11 = reset, Pause = pause/resume. Close the window to quit.

mod emu;
mod font;
mod mcp;
mod screen;

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::machine::{Drive, Machine};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;

use emu::{Emu, Job};
use screen::{SCREEN_H, SCREEN_W};

const BIOS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bios.bin"));
const HELLO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hello.bin"));

const USAGE: &str = "usage: rusty486 [--fda IMG] [--fdb IMG] [--hda IMG] [--hdb IMG] [--mhz N] \
                     [--mcp-port N] [--no-mcp] [IMG]";

struct Args {
    disks: Vec<(Drive, PathBuf)>,
    mhz: u64,
    mcp_port: Option<u16>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { disks: Vec::new(), mhz: 33, mcp_port: Some(mcp::DEFAULT_PORT) };
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
    let mut emu = Emu::new(machine);

    if args.disks.is_empty() {
        let mut floppy = DiskImage::blank_floppy();
        floppy.data[..512].copy_from_slice(HELLO);
        floppy.read_only = true;
        emu.machine.insert_disk(Drive::A, floppy);
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
    canvas.set_logical_size(SCREEN_W, SCREEN_W * 3 / 4).map_err(|e| e.to_string())?;
    let creator = canvas.texture_creator();
    let mut texture = creator
        .create_texture_streaming(PixelFormatEnum::ARGB8888, SCREEN_W, SCREEN_H)
        .map_err(|e| e.to_string())?;
    let mut events = sdl.event_pump()?;

    let start = Instant::now();
    let mut last = Instant::now();
    let mut speed_window = (Instant::now(), emu.machine.cpu.cycles);
    let mut title = String::new();

    'main: loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'main,
                Event::KeyDown { keycode: Some(Keycode::F11), .. } => {
                    emu.machine.reset();
                    emu.resume();
                }
                Event::KeyDown { keycode: Some(Keycode::Pause), .. } => {
                    if emu.paused {
                        emu.resume();
                    } else {
                        emu.pause("paused from keyboard".into());
                    }
                }
                _ => {}
            }
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

        emu.blink_on = (start.elapsed().as_millis() / 266) % 2 == 0;
        emu.screen.render(&emu.machine, emu.blink_on);
        texture
            .update(None, emu.screen.bytes(), (SCREEN_W * 4) as usize)
            .map_err(|e| e.to_string())?;
        canvas.clear();
        canvas.copy(&texture, None, Rect::new(0, 0, SCREEN_W, SCREEN_W * 3 / 4))?;
        canvas.present();

        // Show the effective emulated clock speed in the title once a second.
        let elapsed = speed_window.0.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let mhz =
                (emu.machine.cpu.cycles - speed_window.1) as f64 / elapsed.as_secs_f64() / 1e6;
            let new_title = if emu.paused {
                format!("rusty486 - paused ({})", emu.stop_reason)
            } else {
                format!("rusty486 - {mhz:.1} MHz")
            };
            if new_title != title {
                let _ = canvas.window_mut().set_title(&new_title);
                title = new_title;
            }
            speed_window = (Instant::now(), emu.machine.cpu.cycles);
        }
    }

    emu.save_all_disks();
    Ok(())
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
