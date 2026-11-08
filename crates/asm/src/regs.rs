//! Register names and classes.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reg {
    /// General register: size in bits (8/16/32) and encoding number 0-7.
    Gpr(u8, u8),
    /// Segment register ES CS SS DS FS GS = 0-5.
    Seg(u8),
    Cr(u8),
    Dr(u8),
}

const R8: [&str; 8] = ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"];
const R16: [&str; 8] = ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"];
const R32: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
const SEGS: [&str; 6] = ["es", "cs", "ss", "ds", "fs", "gs"];

pub fn reg(name: &str) -> Option<Reg> {
    let n = name.to_ascii_lowercase();
    let pos = |t: &[&str]| t.iter().position(|r| *r == n).map(|i| i as u8);
    if let Some(i) = pos(&R8) {
        return Some(Reg::Gpr(8, i));
    }
    if let Some(i) = pos(&R16) {
        return Some(Reg::Gpr(16, i));
    }
    if let Some(i) = pos(&R32) {
        return Some(Reg::Gpr(32, i));
    }
    if let Some(i) = pos(&SEGS) {
        return Some(Reg::Seg(i));
    }
    let num = |p: &str| n.strip_prefix(p).and_then(|d| d.parse::<u8>().ok());
    if let Some(i) = num("cr").filter(|i| matches!(i, 0 | 2 | 3 | 4)) {
        return Some(Reg::Cr(i));
    }
    if let Some(i) = num("dr").filter(|i| *i < 8) {
        return Some(Reg::Dr(i));
    }
    None
}

pub fn is_reg(name: &str) -> bool {
    reg(name).is_some()
}
