//! A small 32-bit x86 machine code emitter with labels and fixups.

use alloc::vec::Vec;

pub const EAX: u8 = 0;
pub const ECX: u8 = 1;
pub const EDX: u8 = 2;
pub const EBX: u8 = 3;
pub const ESP: u8 = 4;
pub const EBP: u8 = 5;
pub const ESI: u8 = 6;
pub const EDI: u8 = 7;

/// Condition codes (the low nibble of Jcc/SETcc).
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub enum Cc {
    B = 2,
    Ae = 3,
    E = 4,
    Ne = 5,
    Be = 6,
    A = 7,
    L = 0xC,
    Ge = 0xD,
    Le = 0xE,
    G = 0xF,
}

/// ALU operations with their /r opcode (two-register form) and /digit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alu {
    Add,
    Or,
    And,
    Sub,
    Xor,
    Cmp,
}

impl Alu {
    fn op(self) -> u8 {
        match self {
            Alu::Add => 0x01,
            Alu::Or => 0x09,
            Alu::And => 0x21,
            Alu::Sub => 0x29,
            Alu::Xor => 0x31,
            Alu::Cmp => 0x39,
        }
    }
    fn digit(self) -> u8 {
        match self {
            Alu::Add => 0,
            Alu::Or => 1,
            Alu::And => 4,
            Alu::Sub => 5,
            Alu::Xor => 6,
            Alu::Cmp => 7,
        }
    }
}

/// What an absolute address refers to; resolved once the layout is known.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Abs {
    Data(u32),
    Bss(u32),
    Label(u32),
}

pub type Label = u32;

#[derive(Default)]
pub struct Asm {
    pub code: Vec<u8>,
    labels: Vec<Option<u32>>,
    rel: Vec<(u32, Label)>,
    pub abs: Vec<(u32, Abs)>,
}

impl Asm {
    pub fn pos(&self) -> u32 {
        self.code.len() as u32
    }
    fn b(&mut self, x: u8) {
        self.code.push(x);
    }
    fn d(&mut self, x: u32) {
        self.code.extend_from_slice(&x.to_le_bytes());
    }
    pub fn patch32(&mut self, at: u32, v: u32) {
        self.code[at as usize..at as usize + 4].copy_from_slice(&v.to_le_bytes());
    }

    pub fn label(&mut self) -> Label {
        self.labels.push(None);
        self.labels.len() as Label - 1
    }
    pub fn bind(&mut self, l: Label) {
        self.labels[l as usize] = Some(self.pos());
    }
    pub fn label_pos(&self, l: Label) -> Option<u32> {
        self.labels[l as usize]
    }

    /// ModRM (and SIB) for [base + disp].
    fn mem(&mut self, reg: u8, base: u8, disp: i32) {
        let (md, small) = if disp == 0 && base != EBP {
            (0, None)
        } else if (-128..128).contains(&disp) {
            (1, Some(true))
        } else {
            (2, Some(false))
        };
        self.b(md << 6 | (reg & 7) << 3 | (base & 7));
        if base == ESP {
            self.b(0x24);
        }
        match small {
            Some(true) => self.b(disp as i8 as u8),
            Some(false) => self.d(disp as u32),
            None => {}
        }
    }
    fn rr(&mut self, reg: u8, rm: u8) {
        self.b(0xC0 | reg << 3 | rm);
    }

    pub fn mov_rr(&mut self, dst: u8, src: u8) {
        if dst != src {
            self.b(0x89);
            self.rr(src, dst);
        }
    }
    pub fn mov_ri(&mut self, r: u8, imm: u32) {
        if imm == 0 {
            self.alu_rr(Alu::Xor, r, r);
        } else {
            self.b(0xB8 + r);
            self.d(imm);
        }
    }
    /// `mov r, imm32` with an absolute address resolved at link time.
    pub fn mov_r_abs(&mut self, r: u8, a: Abs) {
        self.b(0xB8 + r);
        self.abs.push((self.pos(), a));
        self.d(0);
    }
    /// Load `size` bytes from [base+disp] into `dst`, sign- or zero-extending.
    pub fn load(&mut self, size: u32, signed: bool, dst: u8, base: u8, disp: i32) {
        match size {
            1 => {
                self.b(0x0F);
                self.b(if signed { 0xBE } else { 0xB6 });
            }
            2 => {
                self.b(0x0F);
                self.b(if signed { 0xBF } else { 0xB7 });
            }
            _ => self.b(0x8B),
        }
        self.mem(dst, base, disp);
    }
    /// Store the low `size` bytes of `src` (EAX, ECX, EDX or EBX for bytes).
    pub fn store(&mut self, size: u32, src: u8, base: u8, disp: i32) {
        match size {
            1 => self.b(0x88),
            2 => {
                self.b(0x66);
                self.b(0x89);
            }
            _ => self.b(0x89),
        }
        self.mem(src, base, disp);
    }
    pub fn lea(&mut self, dst: u8, base: u8, disp: i32) {
        self.b(0x8D);
        self.mem(dst, base, disp);
    }
    pub fn push(&mut self, r: u8) {
        self.b(0x50 + r);
    }
    pub fn pop(&mut self, r: u8) {
        self.b(0x58 + r);
    }
    pub fn push_i(&mut self, imm: u32) {
        if (imm as i32) >= -128 && (imm as i32) < 128 {
            self.b(0x6A);
            self.b(imm as u8);
        } else {
            self.b(0x68);
            self.d(imm);
        }
    }
    pub fn alu_rr(&mut self, op: Alu, dst: u8, src: u8) {
        self.b(op.op());
        self.rr(src, dst);
    }
    pub fn alu_ri(&mut self, op: Alu, r: u8, imm: u32) {
        if (imm as i32) >= -128 && (imm as i32) < 128 {
            self.b(0x83);
            self.rr(op.digit(), r);
            self.b(imm as u8);
        } else {
            self.b(0x81);
            self.rr(op.digit(), r);
            self.d(imm);
        }
    }
    /// `sub esp, imm32` with the immediate patched later; returns its offset.
    pub fn sub_esp_patch(&mut self) -> u32 {
        self.b(0x81);
        self.rr(5, ESP);
        let at = self.pos();
        self.d(0);
        at
    }
    pub fn imul_rr(&mut self, dst: u8, src: u8) {
        self.b(0x0F);
        self.b(0xAF);
        self.rr(dst, src);
    }
    pub fn imul_rri(&mut self, dst: u8, src: u8, imm: u32) {
        self.b(0x69);
        self.rr(dst, src);
        self.d(imm);
    }
    pub fn cdq(&mut self) {
        self.b(0x99);
    }
    pub fn idiv(&mut self, r: u8) {
        self.b(0xF7);
        self.rr(7, r);
    }
    pub fn div(&mut self, r: u8) {
        self.b(0xF7);
        self.rr(6, r);
    }
    pub fn neg(&mut self, r: u8) {
        self.b(0xF7);
        self.rr(3, r);
    }
    pub fn not(&mut self, r: u8) {
        self.b(0xF7);
        self.rr(2, r);
    }
    /// shl (4), shr (5) or sar (7) by CL.
    pub fn shift_cl(&mut self, digit: u8, r: u8) {
        self.b(0xD3);
        self.rr(digit, r);
    }
    pub fn test_rr(&mut self, a: u8, b: u8) {
        self.b(0x85);
        self.rr(b, a);
    }
    /// `setcc al; movzx eax, al`.
    pub fn setcc_eax(&mut self, cc: Cc) {
        self.b(0x0F);
        self.b(0x90 + cc as u8);
        self.b(0xC0);
        self.movzx_eax(1);
    }
    pub fn movzx_eax(&mut self, size: u32) {
        self.b(0x0F);
        self.b(if size == 1 { 0xB6 } else { 0xB7 });
        self.b(0xC0);
    }
    pub fn movsx_eax(&mut self, size: u32) {
        self.b(0x0F);
        self.b(if size == 1 { 0xBE } else { 0xBF });
        self.b(0xC0);
    }
    pub fn jcc(&mut self, cc: Cc, l: Label) {
        self.b(0x0F);
        self.b(0x80 + cc as u8);
        self.rel.push((self.pos(), l));
        self.d(0);
    }
    pub fn jmp(&mut self, l: Label) {
        self.b(0xE9);
        self.rel.push((self.pos(), l));
        self.d(0);
    }
    pub fn call(&mut self, l: Label) {
        self.b(0xE8);
        self.rel.push((self.pos(), l));
        self.d(0);
    }
    /// `call [r]`.
    pub fn call_mem(&mut self, r: u8) {
        self.b(0xFF);
        self.mem(2, r, 0);
    }
    pub fn ret(&mut self) {
        self.b(0xC3);
    }
    pub fn rep_movsb(&mut self) {
        self.b(0xF3);
        self.b(0xA4);
    }
    pub fn rep_stosb(&mut self) {
        self.b(0xF3);
        self.b(0xAA);
    }
    pub fn cld(&mut self) {
        self.b(0xFC);
    }
    /// `out dx, al`.
    pub fn out_dx_al(&mut self) {
        self.b(0xEE);
    }
    /// `in al, dx`.
    pub fn in_al_dx(&mut self) {
        self.b(0xEC);
    }
    pub fn raw(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    pub fn rel_len(&self) -> usize {
        self.rel.len()
    }
    pub fn rel_truncate(&mut self, n: usize) {
        self.rel.truncate(n);
    }

    /// Resolve relative jumps and calls. Returns the first unbound label.
    pub fn resolve(&mut self) -> Result<(), Label> {
        for &(at, l) in &self.rel {
            let target = self.labels[l as usize].ok_or(l)?;
            let rel = target.wrapping_sub(at + 4);
            self.code[at as usize..at as usize + 4].copy_from_slice(&rel.to_le_bytes());
        }
        Ok(())
    }
}
