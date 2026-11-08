//! R++: a compiler for a subset of Rust that produces 32-bit protected-mode
//! TinyDOS programs. `no_std` + `alloc`, so the same compiler runs on the host
//! (tests) and inside rusty486 (the Rustland R++ IDE).
//!
//! Supported: functions, `let`/`mut`, i8-u32/isize/usize, bool, char, &str,
//! arrays, slices, references and raw pointers, structs with `impl` blocks and
//! methods, C-like enums, `const`/`static`, `if`/`while`/`loop`/`for`/`match`,
//! `print!`/`println!`/`panic!`/`assert!` with `{}`/`{:?}`/`{:x}` formatting,
//! and a prelude of DOS, keyboard, screen and sound functions.
//!
//! Not supported: generics, traits, closures, lifetimes, tuples, Option and
//! Result, String/Vec/Box, floating point and 64-bit integers, modules, and
//! borrow checking.

#![no_std]

extern crate alloc;

pub mod ast;
pub mod codegen;
pub mod exe;
pub mod lexer;
pub mod parser;
mod prelude;
pub mod types;
pub mod x86;

use alloc::string::String;
use alloc::vec::Vec;

pub use exe::{pm_exe, PmImage, PM_STUB};
use lexer::Span;
use x86::Abs;

/// Where compiled programs are linked.
pub const LOAD_ADDR: u32 = 0x200_0000;

/// Prelude line numbers are offset so errors in it are recognisable.
const PRELUDE_LINE: u32 = 100_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

impl Error {
    pub fn new(span: Span, msg: &str) -> Error {
        Error { line: span.line, col: span.col, msg: String::from(msg) }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        if self.line >= PRELUDE_LINE {
            write!(f, "prelude:{}:{}: {}", self.line - PRELUDE_LINE, self.col, self.msg)
        } else {
            write!(f, "{}:{}: {}", self.line, self.col, self.msg)
        }
    }
}

/// A linked flat image.
pub struct Image {
    pub code: Vec<u8>,
    pub load: u32,
    pub bss: u32,
    pub stack_top: u32,
    pub code_len: u32,
}

fn parse(src: &str, line_offset: u32) -> Result<Vec<ast::Item>, Error> {
    let mut toks = lexer::lex(src).map_err(|mut e| {
        e.line += line_offset;
        e
    })?;
    for t in &mut toks {
        t.span.line += line_offset;
    }
    parser::Parser::new(&toks).program()
}

/// Compile R++ source to a flat image linked at `LOAD_ADDR`.
pub fn compile_image(src: &str) -> Result<Image, Error> {
    let prelude = parse(prelude::SOURCE, PRELUDE_LINE)?;
    let user = parse(src, 0)?;
    let mut g = codegen::Gen::new();
    g.declare(prelude, true)?;
    g.declare(user, false)?;
    g.finish()?;
    if let Err(l) = g.asm.resolve() {
        return Err(Error { line: 0, col: 0, msg: alloc::format!("internal error: unbound label {l}") });
    }
    let code_len = g.asm.code.len() as u32;
    let data_off = code_len.div_ceil(4) * 4;
    let image_len = data_off + g.data.len() as u32;
    let bss_off = image_len.div_ceil(16) * 16;
    let mut image = g.asm.code.clone();
    image.resize(data_off as usize, 0x90);
    image.extend_from_slice(&g.data);
    let resolve = |a: Abs, asm: &x86::Asm| -> u32 {
        LOAD_ADDR
            + match a {
                Abs::Data(o) => data_off + o,
                Abs::Bss(o) => bss_off + o,
                Abs::Label(l) => asm.label_pos(l).unwrap_or(0),
            }
    };
    for &(at, a) in &g.asm.abs {
        let v = resolve(a, &g.asm);
        image[at as usize..at as usize + 4].copy_from_slice(&v.to_le_bytes());
    }
    for &(at, a) in &g.data_relocs {
        let v = resolve(a, &g.asm);
        let at = (data_off + at) as usize;
        image[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    let bss = (bss_off - image_len) + g.bss.div_ceil(16) * 16 + codegen::STACK_SIZE;
    let stack_top = LOAD_ADDR + image_len + bss;
    Ok(Image { code: image, load: LOAD_ADDR, bss, stack_top, code_len })
}

/// Compile R++ source to a TinyDOS .EXE.
pub fn compile(src: &str) -> Result<Vec<u8>, Error> {
    let img = compile_image(src)?;
    Ok(pm_exe(&PmImage {
        image: &img.code,
        load: img.load,
        bss: img.bss,
        entry: img.load,
        stack_top: img.stack_top,
        heap_end: 0,
    }))
}
