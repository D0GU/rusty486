//! R++ types and their machine representation.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntTy {
    pub signed: bool,
    pub size: u32,
    /// isize/usize: same representation as i32/u32 but a distinct type.
    pub word: bool,
}

impl IntTy {
    pub const I32: IntTy = IntTy { signed: true, size: 4, word: false };
    pub const U32: IntTy = IntTy { signed: false, size: 4, word: false };
    pub const U8: IntTy = IntTy { signed: false, size: 1, word: false };
    pub const USIZE: IntTy = IntTy { signed: false, size: 4, word: true };

    pub fn from_name(n: &str) -> Option<IntTy> {
        let (signed, size, word) = match n {
            "i8" => (true, 1, false),
            "u8" => (false, 1, false),
            "i16" => (true, 2, false),
            "u16" => (false, 2, false),
            "i32" => (true, 4, false),
            "u32" => (false, 4, false),
            "isize" => (true, 4, true),
            "usize" => (false, 4, true),
            _ => return None,
        };
        Some(IntTy { signed, size, word })
    }

    pub fn name(self) -> &'static str {
        match (self.signed, self.size, self.word) {
            (true, 1, _) => "i8",
            (false, 1, _) => "u8",
            (true, 2, _) => "i16",
            (false, 2, _) => "u16",
            (true, _, true) => "isize",
            (false, _, true) => "usize",
            (true, _, _) => "i32",
            _ => "u32",
        }
    }

    /// Whether `v` fits this type.
    pub fn fits(self, v: i64) -> bool {
        let bits = self.size * 8;
        if self.signed {
            let lo = -(1i64 << (bits - 1));
            let hi = (1i64 << (bits - 1)) - 1;
            (lo..=hi).contains(&v)
        } else {
            (0..=((1i64 << bits) - 1)).contains(&v)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ty {
    Unit,
    Never,
    Bool,
    Char,
    Int(IntTy),
    /// References and raw pointers to sized types.
    Ptr(Box<Ty>),
    /// `&str`: pointer and length.
    Str,
    /// `&[T]`: pointer and length.
    Slice(Box<Ty>),
    Array(Box<Ty>, u32),
    Struct(usize),
    Enum(usize),
}

/// How a value of a type is held while computing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Class {
    /// No value.
    Void,
    /// In EAX, extended to 32 bits.
    Scalar { size: u32, signed: bool },
    /// EAX = pointer, EDX = length.
    Fat,
    /// EAX = address of the value in memory.
    Agg { size: u32 },
}

impl Ty {
    pub fn int(t: IntTy) -> Ty {
        Ty::Int(t)
    }
    pub fn usize() -> Ty {
        Ty::Int(IntTy::USIZE)
    }
    pub fn ptr(t: Ty) -> Ty {
        Ty::Ptr(Box::new(t))
    }
    pub fn is_int(&self) -> bool {
        matches!(self, Ty::Int(_))
    }
    pub fn signed(&self) -> bool {
        matches!(self, Ty::Int(IntTy { signed: true, .. }))
    }
}

/// Struct layouts are needed to size types; the code generator implements this.
pub trait Layouts {
    fn struct_size(&self, id: usize) -> (u32, u32);
    fn struct_name(&self, id: usize) -> &str;
    fn enum_name(&self, id: usize) -> &str;
}

pub fn class(t: &Ty, l: &dyn Layouts) -> Class {
    match t {
        Ty::Unit | Ty::Never => Class::Void,
        Ty::Bool => Class::Scalar { size: 1, signed: false },
        Ty::Char | Ty::Enum(_) | Ty::Ptr(_) => Class::Scalar { size: 4, signed: false },
        Ty::Int(i) => Class::Scalar { size: i.size, signed: i.signed },
        Ty::Str | Ty::Slice(_) => Class::Fat,
        Ty::Array(..) | Ty::Struct(_) => Class::Agg { size: size_of(t, l) },
    }
}

pub fn size_of(t: &Ty, l: &dyn Layouts) -> u32 {
    match t {
        Ty::Unit | Ty::Never => 0,
        Ty::Bool => 1,
        Ty::Char | Ty::Enum(_) | Ty::Ptr(_) => 4,
        Ty::Int(i) => i.size,
        Ty::Str | Ty::Slice(_) => 8,
        Ty::Array(e, n) => size_of(e, l) * n,
        Ty::Struct(id) => l.struct_size(*id).0,
    }
}

pub fn align_of(t: &Ty, l: &dyn Layouts) -> u32 {
    match t {
        Ty::Unit | Ty::Never => 1,
        Ty::Bool => 1,
        Ty::Int(i) => i.size,
        Ty::Array(e, _) => align_of(e, l),
        Ty::Struct(id) => l.struct_size(*id).1,
        _ => 4,
    }
}

pub fn name(t: &Ty, l: &dyn Layouts) -> String {
    match t {
        Ty::Unit => String::from("()"),
        Ty::Never => String::from("!"),
        Ty::Bool => String::from("bool"),
        Ty::Char => String::from("char"),
        Ty::Int(i) => String::from(i.name()),
        Ty::Ptr(t) => format!("&{}", name(t, l)),
        Ty::Str => String::from("&str"),
        Ty::Slice(t) => format!("&[{}]", name(t, l)),
        Ty::Array(t, n) => format!("[{}; {n}]", name(t, l)),
        Ty::Struct(id) => String::from(l.struct_name(*id)),
        Ty::Enum(id) => String::from(l.enum_name(*id)),
    }
}

/// The name methods on a type are registered under (`Point::area`, `u8::is_ascii_digit`).
pub fn method_owner(t: &Ty, l: &dyn Layouts) -> Option<String> {
    Some(match t {
        Ty::Bool => String::from("bool"),
        Ty::Char => String::from("char"),
        Ty::Int(i) => String::from(i.name()),
        Ty::Str => String::from("str"),
        Ty::Struct(id) => String::from(l.struct_name(*id)),
        Ty::Enum(id) => String::from(l.enum_name(*id)),
        _ => return None,
    })
}
