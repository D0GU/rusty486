//! Rustland R++: a Borland-style IDE for R++ (a Rust subset), running in
//! 32-bit protected mode under TinyDOS via the rusty486 PM stub.

#![no_std]
#![no_main]

extern crate alloc;

mod app;
mod edit;
mod help;
mod rt;
mod scr;

use core::fmt::Write;

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text._start")]
pub extern "C" fn _start(systab: *const rt::SysTab) -> u32 {
    rt::set_systab(systab);
    let mut app = app::App::new();
    let arg = rt::command_tail();
    if !arg.is_empty() {
        app.open_path(&arg);
    }
    app.run();
    0
}

struct VramWriter {
    x: usize,
    y: usize,
}

impl Write for VramWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            if b == b'\n' || self.x >= 80 {
                self.x = 0;
                self.y += 1;
                if b == b'\n' {
                    continue;
                }
            }
            if self.y < 25 {
                let p = (0xB8000 + (self.y * 80 + self.x) * 2) as *mut u16;
                unsafe { core::ptr::write_volatile(p, 0x4F00 | b as u16) };
            }
            self.x += 1;
        }
        Ok(())
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let mut w = VramWriter { x: 0, y: 20 };
    let _ = write!(w, "Rustland R++ internal error: {}\nPress any key to exit.", info);
    rt::read_key();
    rt::set_text_mode();
    rt::exit(255)
}
