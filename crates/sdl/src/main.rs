//! rusty486 desktop frontend.
//!
//! ```text
//! rusty486 [--fda IMG] [--fdb IMG] [--hda IMG] [--hdb IMG] [--mhz N] [IMG]
//! ```
//!
//! With no disks, it boots the built-in milestone 1 demo floppy. Disk images
//! the guest writes to are saved back to their files on exit.
//!
//! Keys: F11 = reset, Pause = pause/resume. Close the window to quit.

mod font;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::devices::video::{TEXT_COLS, TEXT_ROWS};
use rusty486_core::machine::{Drive, Machine};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::PixelFormatEnum;
use sdl2::rect::Rect;

const BIOS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bios.bin"));
const HELLO: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/hello.bin"));

const SCREEN_W: u32 = (TEXT_COLS * 8) as u32; // 640
const SCREEN_H: u32 = (TEXT_ROWS * 16) as u32; // 400

/// The 16 CGA/VGA text colours (ARGB).
const PALETTE: [u32; 16] = [
    0xFF000000, 0xFF0000AA, 0xFF00AA00, 0xFF00AAAA, 0xFFAA0000, 0xFFAA00AA, 0xFFAA5500, 0xFFAAAAAA,
    0xFF555555, 0xFF5555FF, 0xFF55FF55, 0xFF55FFFF, 0xFFFF5555, 0xFFFF55FF, 0xFFFFFF55, 0xFFFFFFFF,
];

struct Args {
    disks: Vec<(Drive, PathBuf)>,
    mhz: u64,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { disks: Vec::new(), mhz: 33 };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = || it.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--fda" => args.disks.push((Drive::A, value()?.into())),
            "--fdb" => args.disks.push((Drive::B, value()?.into())),
            "--hda" => args.disks.push((Drive::C, value()?.into())),
            "--hdb" => args.disks.push((Drive::D, value()?.into())),
            "--mhz" => args.mhz = value()?.parse().map_err(|e| format!("--mhz: {e}"))?,
            "-h" | "--help" => {
                return Err("usage: rusty486 [--fda IMG] [--fdb IMG] [--hda IMG] [--hdb IMG] [--mhz N] [IMG]".into())
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
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

    if args.disks.is_empty() {
        let mut floppy = DiskImage::blank_floppy();
        floppy.data[..512].copy_from_slice(HELLO);
        floppy.read_only = true;
        machine.insert_disk(Drive::A, floppy);
    }
    for (drive, path) in &args.disks {
        let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        machine.insert_disk(*drive, DiskImage::new(data));
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

    let glyphs = font::cp437();
    let mut pixels = vec![0u32; (SCREEN_W * SCREEN_H) as usize];
    let start = Instant::now();
    let mut last = Instant::now();
    let mut paused = false;
    let mut speed_window = (Instant::now(), machine.cpu.cycles);

    'main: loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } => break 'main,
                Event::KeyDown { keycode: Some(Keycode::F11), .. } => machine.reset(),
                Event::KeyDown { keycode: Some(Keycode::Pause), .. } => paused = !paused,
                _ => {}
            }
        }

        // Run as many cycles as real time has elapsed (capped to avoid a
        // spiral of death if the host falls behind).
        let now = Instant::now();
        let dt = now.duration_since(last).min(Duration::from_millis(100));
        last = now;
        if !paused {
            machine.run_cycles((dt.as_secs_f64() * machine.clock_hz as f64) as u64);
        }

        let log = machine.drain_debug();
        if !log.is_empty() {
            print!("{}", String::from_utf8_lossy(&log));
        }

        let blink_on = (start.elapsed().as_millis() / 266) % 2 == 0;
        render_text(&machine, &glyphs, &mut pixels, blink_on);
        texture
            .update(None, bytemuck_u8(&pixels), (SCREEN_W * 4) as usize)
            .map_err(|e| e.to_string())?;
        canvas.clear();
        canvas.copy(&texture, None, Rect::new(0, 0, SCREEN_W, SCREEN_W * 3 / 4))?;
        canvas.present();

        // Show the effective emulated clock speed in the title once a second.
        let elapsed = speed_window.0.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let mhz = (machine.cpu.cycles - speed_window.1) as f64 / elapsed.as_secs_f64() / 1e6;
            let state = if paused { " [paused]" } else { "" };
            let _ = canvas.window_mut().set_title(&format!("rusty486 - {mhz:.1} MHz{state}"));
            speed_window = (Instant::now(), machine.cpu.cycles);
        }
    }

    save_disks(&machine, &args.disks);
    Ok(())
}

/// Write back any disk image the guest modified.
fn save_disks(machine: &Machine, disks: &[(Drive, PathBuf)]) {
    for (drive, path) in disks {
        if let Some(d) = machine.disk(*drive).filter(|d| d.dirty) {
            match std::fs::write(path, &d.data) {
                Ok(()) => println!("saved {}", path.display()),
                Err(e) => eprintln!("failed to save {}: {e}", path.display()),
            }
        }
    }
}

/// Draw the 80x25 text buffer into an ARGB pixel buffer.
fn render_text(machine: &Machine, glyphs: &[[u8; 8]], out: &mut [u32], blink_on: bool) {
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
            let glyph = &glyphs[ch as usize];
            let show_cursor = i == cursor && blink_on && video.cursor_visible();

            for y in 0..16 {
                let bits = if show_fg { glyph[y / 2] } else { 0 };
                let cursor_line =
                    show_cursor && (cur_start as usize..=cur_end as usize).contains(&y);
                let base = (row * 16 + y) * SCREEN_W as usize + col * 8;
                for x in 0..8 {
                    let on = bits & (1 << x) != 0 || cursor_line;
                    out[base + x] = if on { fg } else { bg };
                }
            }
        }
    }
}

/// View a u32 pixel buffer as bytes for SDL.
fn bytemuck_u8(px: &[u32]) -> &[u8] {
    // SAFETY: u32 has no padding and u8 has alignment 1; the length covers
    // exactly the same memory.
    unsafe { std::slice::from_raw_parts(px.as_ptr().cast::<u8>(), px.len() * 4) }
}
