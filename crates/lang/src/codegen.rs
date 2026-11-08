//! Type checking and x86 code generation in a single pass over the syntax
//! tree. Values live in registers (see `types::Class`); intermediate results
//! go on the machine stack; locals and temporaries live in the EBP frame.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::ast::*;
use crate::types::{self, align_of, class, size_of, Class, IntTy, Layouts, Ty};
use crate::x86::*;
use crate::Error;

type R<T> = Result<T, Error>;

/// Bytes reserved for the stack of a compiled program.
pub const STACK_SIZE: u32 = 256 * 1024;

struct StructDef {
    name: String,
    fields: Vec<(String, Ty, u32)>,
    size: u32,
    align: u32,
    decl: Option<StructDecl>,
    state: u8,
    prelude: bool,
}

struct EnumDef {
    name: String,
    variants: Vec<(String, i64)>,
}

#[derive(Clone)]
struct FnSig {
    label: Label,
    params: Vec<Ty>,
    ret: Ty,
    self_kind: Option<SelfKind>,
}

struct FnBody {
    decl: FnDecl,
    sig: FnSig,
    self_ty: Option<Ty>,
    prelude: bool,
}

#[derive(Clone)]
enum Global {
    Const(Ty, Expr),
    Static(Ty, u32),
}

#[derive(Clone)]
struct Local {
    name: String,
    off: i32,
    ty: Ty,
    mutable: bool,
}

struct LoopCtx {
    brk: Label,
    cont: Label,
    broke: bool,
}

/// A parsed piece of a format string.
enum Piece {
    Lit(Vec<u8>),
    Arg { expr: Expr, debug: bool, radix: u32, upper: bool, width: u32, flags: u32, ptr: bool },
}

pub struct Gen {
    pub asm: Asm,
    pub data: Vec<u8>,
    pub data_relocs: Vec<(u32, Abs)>,
    pub bss: u32,
    structs: Vec<StructDef>,
    struct_ids: BTreeMap<String, usize>,
    enums: Vec<EnumDef>,
    enum_ids: BTreeMap<String, usize>,
    ufns: BTreeMap<String, FnSig>,
    pfns: BTreeMap<String, FnSig>,
    globals: BTreeMap<String, Global>,
    strings: BTreeMap<Vec<u8>, u32>,
    systab: u32,
    bodies: Vec<FnBody>,
    // Per function.
    scopes: Vec<Vec<Local>>,
    frame: u32,
    ret_ty: Ty,
    ret_hidden: bool,
    ret_label: Label,
    loops: Vec<LoopCtx>,
    self_ty: Option<Ty>,
    in_prelude: bool,
    const_depth: u32,
}

impl Layouts for Gen {
    fn struct_size(&self, id: usize) -> (u32, u32) {
        (self.structs[id].size, self.structs[id].align)
    }
    fn struct_name(&self, id: usize) -> &str {
        &self.structs[id].name
    }
    fn enum_name(&self, id: usize) -> &str {
        &self.enums[id].name
    }
}

fn err<T>(span: Span, msg: impl Into<String>) -> R<T> {
    Err(Error::new(span, &msg.into()))
}

fn align_up(v: u32, a: u32) -> u32 {
    v.div_ceil(a.max(1)) * a.max(1)
}

fn is_untyped_lit(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_, None) => true,
        ExprKind::Unary(UnOp::Neg, x) => is_untyped_lit(x),
        ExprKind::Binary(op, a, b) if !is_cmp(*op) && !matches!(op, BinOp::LAnd | BinOp::LOr) => {
            is_untyped_lit(a) && is_untyped_lit(b)
        }
        _ => false,
    }
}

/// Whether `e` is `x` or arithmetic directly on `x` (so it has x's type).
fn mentions(e: &Expr, x: &str) -> bool {
    match &e.kind {
        ExprKind::Path(p) => p.len() == 1 && p[0] == x,
        ExprKind::Binary(op, a, b) if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem | BinOp::And | BinOp::Or | BinOp::Xor) => {
            mentions(a, x) || mentions(b, x)
        }
        ExprKind::Binary(BinOp::Shl | BinOp::Shr, a, _) => mentions(a, x),
        ExprKind::Unary(_, a) => mentions(a, x),
        ExprKind::Block(b) if b.stmts.is_empty() => b.tail.as_ref().is_some_and(|t| mentions(t, x)),
        _ => false,
    }
}

fn is_cmp(op: BinOp) -> bool {
    matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
}

fn op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::And => "&",
        BinOp::Or => "|",
        BinOp::Xor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::LAnd => "&&",
        BinOp::LOr => "||",
    }
}

impl Gen {
    pub fn new() -> Gen {
        Gen {
            asm: Asm::default(),
            data: Vec::new(),
            data_relocs: Vec::new(),
            bss: 0,
            structs: Vec::new(),
            struct_ids: BTreeMap::new(),
            enums: Vec::new(),
            enum_ids: BTreeMap::new(),
            ufns: BTreeMap::new(),
            pfns: BTreeMap::new(),
            globals: BTreeMap::new(),
            strings: BTreeMap::new(),
            systab: 0,
            bodies: Vec::new(),
            scopes: Vec::new(),
            frame: 0,
            ret_ty: Ty::Unit,
            ret_hidden: false,
            ret_label: 0,
            loops: Vec::new(),
            self_ty: None,
            in_prelude: false,
            const_depth: 0,
        }
    }

    fn tn(&self, t: &Ty) -> String {
        types::name(t, self)
    }
    fn class(&self, t: &Ty) -> Class {
        class(t, self)
    }
    fn size(&self, t: &Ty) -> u32 {
        size_of(t, self)
    }
    fn mismatch<T>(&self, span: Span, want: &Ty, have: &Ty) -> R<T> {
        err(span, format!("mismatched types: expected `{}`, found `{}`", self.tn(want), self.tn(have)))
    }

    // ------------------------------------------------------------ declarations

    /// Register all items of a source file (structs, enums, consts, statics
    /// and function signatures). Bodies are compiled by `finish`.
    pub fn declare(&mut self, items: &[Item], prelude: bool) -> R<()> {
        self.in_prelude = prelude;
        for it in items {
            match it {
                Item::Struct(s) => {
                    if self.struct_ids.contains_key(&s.name) || self.enum_ids.contains_key(&s.name) {
                        return err(s.span, format!("the name `{}` is defined multiple times", s.name));
                    }
                    self.struct_ids.insert(s.name.clone(), self.structs.len());
                    self.structs.push(StructDef {
                        name: s.name.clone(),
                        fields: Vec::new(),
                        size: 0,
                        align: 1,
                        decl: Some(s.clone()),
                        state: 0,
                        prelude,
                    });
                }
                Item::Enum(e) => {
                    if self.struct_ids.contains_key(&e.name) || self.enum_ids.contains_key(&e.name) {
                        return err(e.span, format!("the name `{}` is defined multiple times", e.name));
                    }
                    self.enum_ids.insert(e.name.clone(), self.enums.len());
                    self.enums.push(EnumDef { name: e.name.clone(), variants: Vec::new() });
                }
                Item::Const(n, ty, e, span) => {
                    if self.globals.contains_key(n) {
                        return err(*span, format!("the name `{n}` is defined multiple times"));
                    }
                    // Type resolved later, once all structs are known.
                    let _ = ty;
                    self.globals.insert(n.clone(), Global::Const(Ty::Unit, e.clone()));
                }
                _ => {}
            }
        }
        // Enum discriminants.
        for it in items {
            if let Item::Enum(e) = it {
                let id = self.enum_ids[&e.name];
                let mut next = 0i64;
                let mut vs = Vec::new();
                for (v, val) in &e.variants {
                    if let Some(x) = val {
                        next = self.const_eval(x)?;
                    }
                    if vs.iter().any(|(n, _): &(String, i64)| n == v) {
                        return err(e.span, format!("variant `{v}` is defined multiple times"));
                    }
                    vs.push((v.clone(), next));
                    next += 1;
                }
                self.enums[id].variants = vs;
            }
        }
        // Struct layouts.
        for id in 0..self.structs.len() {
            self.layout_struct(id)?;
        }
        // Const types.
        for it in items {
            if let Item::Const(n, ty, e, _) = it {
                let t = self.resolve_ty(ty)?;
                self.globals.insert(n.clone(), Global::Const(t, e.clone()));
            }
        }
        // Statics.
        for it in items {
            if let Item::Static(n, ty, e, span) = it {
                if self.globals.contains_key(n) {
                    return err(*span, format!("the name `{n}` is defined multiple times"));
                }
                let t = self.resolve_ty(ty)?;
                let off = align_up(self.data.len() as u32, align_of(&t, self).max(4));
                self.data.resize((off + self.size(&t)) as usize, 0);
                self.static_init(&t, e, off)?;
                self.globals.insert(n.clone(), Global::Static(t, off));
            }
        }
        // Function signatures.
        for it in items {
            match it {
                Item::Fn(f) => self.declare_fn(f, f.name.clone(), None, prelude)?,
                Item::Impl(owner, fns, span) => {
                    let self_ty = if owner == "str" {
                        Ty::Str
                    } else {
                        self.resolve_ty(&TypeExpr::Name(owner.clone(), *span))?
                    };
                    if matches!(self_ty, Ty::Ptr(_) | Ty::Slice(_) | Ty::Array(..)) {
                        return err(*span, "impl blocks are only supported for named types");
                    }
                    for f in fns {
                        self.declare_fn(f, format!("{owner}::{}", f.name), Some(self_ty.clone()), prelude)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn declare_fn(&mut self, f: &FnDecl, key: String, self_ty: Option<Ty>, prelude: bool) -> R<()> {
        let saved = core::mem::replace(&mut self.self_ty, self_ty.clone());
        let mut params = Vec::new();
        if let Some(k) = f.self_kind {
            let st = self_ty.clone().unwrap();
            params.push(if k == SelfKind::Ref { Ty::ptr(st) } else { st });
        }
        for p in &f.params {
            params.push(self.resolve_ty(&p.ty)?);
        }
        let ret = match &f.ret {
            Some(t) => self.resolve_ty(t)?,
            None => Ty::Unit,
        };
        self.self_ty = saved;
        let sig = FnSig { label: self.asm.label(), params, ret, self_kind: f.self_kind };
        let map = if prelude { &mut self.pfns } else { &mut self.ufns };
        if map.contains_key(&key) {
            return err(f.span, format!("the name `{key}` is defined multiple times"));
        }
        map.insert(key, sig.clone());
        self.bodies.push(FnBody { decl: f.clone(), sig, self_ty, prelude });
        Ok(())
    }

    fn layout_struct(&mut self, id: usize) -> R<()> {
        match self.structs[id].state {
            2 => return Ok(()),
            1 => {
                let d = self.structs[id].decl.as_ref().unwrap();
                return err(d.span, format!("recursive type `{}` has infinite size", d.name));
            }
            _ => {}
        }
        self.structs[id].state = 1;
        let decl = self.structs[id].decl.clone().unwrap();
        let saved = self.in_prelude;
        self.in_prelude = self.structs[id].prelude;
        let mut fields = Vec::new();
        let (mut off, mut align) = (0u32, 1u32);
        for (n, te) in &decl.fields {
            let t = self.resolve_ty(te)?;
            self.ensure_layout(&t)?;
            let a = align_of(&t, self);
            off = align_up(off, a);
            if fields.iter().any(|(f, _, _): &(String, Ty, u32)| f == n) {
                return err(decl.span, format!("field `{n}` is already declared"));
            }
            fields.push((n.clone(), t.clone(), off));
            off += self.size(&t);
            align = align.max(a);
        }
        self.in_prelude = saved;
        let s = &mut self.structs[id];
        s.fields = fields;
        s.align = align;
        s.size = align_up(off, align);
        s.state = 2;
        Ok(())
    }

    fn ensure_layout(&mut self, t: &Ty) -> R<()> {
        match t {
            Ty::Struct(id) => self.layout_struct(*id),
            Ty::Array(e, _) => self.ensure_layout(e),
            _ => Ok(()),
        }
    }

    fn resolve_ty(&self, te: &TypeExpr) -> R<Ty> {
        Ok(match te {
            TypeExpr::Unit => Ty::Unit,
            TypeExpr::Never => Ty::Never,
            TypeExpr::Ptr(inner) => match &**inner {
                TypeExpr::Name(n, _) if n == "str" => Ty::Str,
                TypeExpr::Slice(e) => Ty::Slice(Box::new(self.resolve_ty(e)?)),
                other => Ty::ptr(self.resolve_ty(other)?),
            },
            TypeExpr::Slice(_) => {
                return err(Span::default(), "slices must be behind a reference: use `&[T]`");
            }
            TypeExpr::Array(e, n) => {
                let t = self.resolve_ty(e)?;
                let n = self.const_eval(n)?;
                if !(0..=0x100_0000).contains(&n) {
                    return err(Span::default(), "array length out of range");
                }
                Ty::Array(Box::new(t), n as u32)
            }
            TypeExpr::Name(n, span) => {
                if let Some(i) = IntTy::from_name(n) {
                    return Ok(Ty::Int(i));
                }
                match n.as_str() {
                    "bool" => Ty::Bool,
                    "char" => Ty::Char,
                    "Self" => match &self.self_ty {
                        Some(t) => t.clone(),
                        None => return err(*span, "`Self` is only available in impl blocks"),
                    },
                    "str" => return err(*span, "`str` must be used as `&str`"),
                    "i64" | "u64" | "i128" | "u128" | "f32" | "f64" => {
                        return err(*span, format!("`{n}` is not supported (R++ has 8, 16 and 32-bit integers)"));
                    }
                    "String" | "Vec" | "Box" | "Option" | "Result" => {
                        return err(*span, format!("`{n}` is not supported; use arrays, slices and &str"));
                    }
                    _ => {
                        if let Some(&id) = self.struct_ids.get(n) {
                            Ty::Struct(id)
                        } else if let Some(&id) = self.enum_ids.get(n) {
                            Ty::Enum(id)
                        } else {
                            return err(*span, format!("cannot find type `{n}`"));
                        }
                    }
                }
            }
        })
    }

    /// Evaluate an integer constant expression.
    fn const_eval(&self, e: &Expr) -> R<i64> {
        self.const_eval_d(e, 0)
    }

    fn const_eval_d(&self, e: &Expr, depth: u32) -> R<i64> {
        if depth > 64 {
            return err(e.span, "constant expression too deep (does a constant refer to itself?)");
        }
        let d = depth + 1;
        let bad = || err(e.span, "expected a constant expression");
        Ok(match &e.kind {
            ExprKind::Int(v, _) => *v as i64,
            ExprKind::Char(c) => *c as i64,
            ExprKind::Byte(c) => *c as i64,
            ExprKind::Bool(b) => *b as i64,
            ExprKind::Unary(UnOp::Neg, x) => -self.const_eval_d(x, d)?,
            ExprKind::Unary(UnOp::Not, x) => !self.const_eval_d(x, d)?,
            ExprKind::Cast(x, t) => {
                let v = self.const_eval_d(x, d)?;
                match self.resolve_ty(t)? {
                    Ty::Int(i) => {
                        let bits = i.size * 8;
                        let m = if bits == 32 { v & 0xFFFF_FFFF } else { v & ((1 << bits) - 1) };
                        if i.signed && m >> (bits - 1) & 1 == 1 {
                            m - (1 << bits)
                        } else {
                            m
                        }
                    }
                    _ => v,
                }
            }
            ExprKind::Block(b) if b.stmts.is_empty() && b.tail.is_some() => self.const_eval_d(b.tail.as_ref().unwrap(), d)?,
            ExprKind::Binary(op, a, b) => {
                let (x, y) = (self.const_eval_d(a, d)?, self.const_eval_d(b, d)?);
                match op {
                    BinOp::Add => x.wrapping_add(y),
                    BinOp::Sub => x.wrapping_sub(y),
                    BinOp::Mul => x.wrapping_mul(y),
                    BinOp::Div | BinOp::Rem if y == 0 => return err(e.span, "division by zero in a constant"),
                    BinOp::Div => x / y,
                    BinOp::Rem => x % y,
                    BinOp::And => x & y,
                    BinOp::Or => x | y,
                    BinOp::Xor => x ^ y,
                    BinOp::Shl => x << (y & 63),
                    BinOp::Shr => x >> (y & 63),
                    BinOp::Eq => (x == y) as i64,
                    BinOp::Ne => (x != y) as i64,
                    BinOp::Lt => (x < y) as i64,
                    BinOp::Le => (x <= y) as i64,
                    BinOp::Gt => (x > y) as i64,
                    BinOp::Ge => (x >= y) as i64,
                    BinOp::LAnd => (x != 0 && y != 0) as i64,
                    BinOp::LOr => (x != 0 || y != 0) as i64,
                }
            }
            ExprKind::Path(p) => {
                if let Some(v) = self.path_const(p) {
                    return Ok(v);
                }
                if p.len() == 1 {
                    if let Some(Global::Const(_, ce)) = self.globals.get(&p[0]) {
                        return self.const_eval_d(&ce.clone(), d);
                    }
                }
                return bad();
            }
            _ => return bad(),
        })
    }

    /// Enum variants and integer MIN/MAX.
    fn path_const(&self, p: &[String]) -> Option<i64> {
        if p.len() != 2 {
            return None;
        }
        if let Some(&id) = self.enum_ids.get(&p[0]) {
            return self.enums[id].variants.iter().find(|(n, _)| *n == p[1]).map(|(_, v)| *v);
        }
        let i = IntTy::from_name(&p[0])?;
        let bits = i.size * 8;
        Some(match (p[1].as_str(), i.signed) {
            ("MAX", true) => (1i64 << (bits - 1)) - 1,
            ("MIN", true) => -(1i64 << (bits - 1)),
            ("MAX", false) => (1i64 << bits) - 1,
            ("MIN", false) => 0,
            ("BITS", _) => bits as i64,
            _ => return None,
        })
    }

    fn static_init(&mut self, t: &Ty, e: &Expr, off: u32) -> R<()> {
        match (t, &e.kind) {
            (Ty::Str, ExprKind::Str(s)) => {
                let p = self.string(s);
                self.data_relocs.push((off, Abs::Data(p)));
                let len = s.len() as u32;
                self.data[off as usize + 4..off as usize + 8].copy_from_slice(&len.to_le_bytes());
            }
            (Ty::Array(et, n), ExprKind::ArrayLit(xs)) => {
                if xs.len() as u32 != *n {
                    return err(e.span, format!("expected an array with {n} elements, found {}", xs.len()));
                }
                let es = self.size(et);
                for (i, x) in xs.iter().enumerate() {
                    self.static_init(et, x, off + i as u32 * es)?;
                }
            }
            (Ty::Array(et, n), ExprKind::ArrayRepeat(x, _)) => {
                let es = self.size(et);
                for i in 0..*n {
                    self.static_init(et, x, off + i * es)?;
                }
            }
            (Ty::Struct(id), ExprKind::StructLit(_, fs)) => {
                let fields = self.structs[*id].fields.clone();
                for (n, ft, fo) in fields {
                    let x = fs.iter().find(|(f, _)| *f == n).map(|(_, x)| x);
                    match x {
                        Some(x) => self.static_init(&ft, x, off + fo)?,
                        None => return err(e.span, format!("missing field `{n}`")),
                    }
                }
            }
            (Ty::Int(_) | Ty::Bool | Ty::Char | Ty::Enum(_), _) => {
                let v = self.const_eval(e)? as u32;
                let n = self.size(t) as usize;
                self.data[off as usize..off as usize + n].copy_from_slice(&v.to_le_bytes()[..n]);
            }
            _ => return err(e.span, "this static initializer is not supported (use constants, arrays and struct literals)"),
        }
        Ok(())
    }

    /// Intern a string literal in the data section; returns its offset.
    fn string(&mut self, s: &[u8]) -> u32 {
        if let Some(&o) = self.strings.get(s) {
            return o;
        }
        let o = self.data.len() as u32;
        self.data.extend_from_slice(s);
        self.strings.insert(s.to_vec(), o);
        o
    }

    fn lookup_fn(&self, name: &str) -> Option<FnSig> {
        if self.in_prelude {
            self.pfns.get(name).cloned()
        } else {
            self.ufns.get(name).or_else(|| self.pfns.get(name)).cloned()
        }
    }

    fn prelude_fn(&self, name: &str) -> FnSig {
        self.pfns.get(name).cloned().unwrap_or_else(|| panic!("prelude function {name} missing"))
    }

    // ------------------------------------------------------------ program

    /// Emit the entry point and every function body.
    pub fn finish(&mut self) -> R<()> {
        self.systab = self.bss;
        self.bss += 4;
        let main = match self.ufns.get("main") {
            Some(m) => m.clone(),
            None => return err(Span { line: 1, col: 1 }, "`main` function not found"),
        };
        if !main.params.is_empty() || main.ret != Ty::Unit {
            return err(Span { line: 1, col: 1 }, "`main` must be `fn main()`");
        }
        // Entry: save the system table, initialise the runtime, run main.
        self.asm.load(4, false, EAX, ESP, 4);
        self.asm.mov_r_abs(ECX, Abs::Bss(self.systab));
        self.asm.store(4, EAX, ECX, 0);
        self.asm.cld();
        let init = self.prelude_fn("__init");
        self.asm.call(init.label);
        self.asm.call(main.label);
        self.asm.mov_ri(EAX, 0);
        self.asm.ret();
        let bodies = core::mem::take(&mut self.bodies);
        for b in &bodies {
            self.function(b)?;
        }
        Ok(())
    }

    fn function(&mut self, b: &FnBody) -> R<()> {
        self.in_prelude = b.prelude;
        self.self_ty = b.self_ty.clone();
        self.asm.bind(b.sig.label);
        self.asm.push(EBP);
        self.asm.mov_rr(EBP, ESP);
        let frame_patch = self.asm.sub_esp_patch();
        self.frame = 0;
        self.loops.clear();
        self.ret_ty = b.sig.ret.clone();
        self.ret_hidden = matches!(self.class(&b.sig.ret), Class::Agg { .. });
        self.ret_label = self.asm.label();
        let mut off = 8 + if self.ret_hidden { 4 } else { 0 };
        let mut scope = Vec::new();
        let mut names: Vec<(String, bool)> = Vec::new();
        if b.decl.self_kind.is_some() {
            names.push((String::from("self"), b.decl.self_mut));
        }
        for p in &b.decl.params {
            names.push((p.name.clone(), p.mutable));
        }
        for ((name, mutable), ty) in names.into_iter().zip(&b.sig.params) {
            scope.push(Local { name, off, ty: ty.clone(), mutable });
            off += match self.class(ty) {
                Class::Fat => 8,
                Class::Agg { size } => align_up(size, 4) as i32,
                _ => 4,
            };
        }
        self.scopes = vec![scope];
        let ret = b.sig.ret.clone();
        let t = self.block(&b.decl.body, Some(&ret))?;
        let span = b.decl.body.tail.as_ref().map(|e| e.span).unwrap_or(b.decl.span);
        if t != Ty::Never {
            if t == Ty::Unit && ret != Ty::Unit {
                return err(span, format!("mismatched types: expected `{}`, found `()` (missing return value)", self.tn(&ret)));
            }
            self.coerce(&t, &ret, span)?;
            self.ret_value();
        }
        self.asm.bind(self.ret_label);
        self.asm.mov_rr(ESP, EBP);
        self.asm.pop(EBP);
        self.asm.ret();
        let frame = align_up(self.frame, 4);
        self.asm.patch32(frame_patch, frame);
        Ok(())
    }

    /// The return value is in registers: copy aggregates to the caller's buffer.
    fn ret_value(&mut self) {
        if let Class::Agg { size } = self.class(&self.ret_ty.clone()) {
            self.asm.load(4, false, EDI, EBP, 8);
            self.asm.mov_rr(ESI, EAX);
            self.asm.mov_ri(ECX, size);
            self.asm.rep_movsb();
            self.asm.load(4, false, EAX, EBP, 8);
        }
    }

    fn alloc_slot(&mut self, t: &Ty) -> i32 {
        let size = self.size(t).max(1);
        let a = align_of(t, self).max(4);
        self.frame = align_up(self.frame + size, a);
        -(self.frame as i32)
    }

    fn lookup_local(&self, name: &str) -> Option<Local> {
        for s in self.scopes.iter().rev() {
            if let Some(l) = s.iter().rev().find(|l| l.name == name) {
                return Some(l.clone());
            }
        }
        None
    }

    fn declare_local(&mut self, name: &str, ty: Ty, mutable: bool) -> i32 {
        let off = self.alloc_slot(&ty);
        if name != "_" {
            self.scopes.last_mut().unwrap().push(Local { name: String::from(name), off, ty, mutable });
        }
        off
    }

    // ------------------------------------------------------------ value moves

    /// Store the value in registers to [base+disp].
    fn store_to(&mut self, t: &Ty, base: u8, disp: i32) {
        match self.class(t) {
            Class::Void => {}
            Class::Scalar { size, .. } => self.asm.store(size, EAX, base, disp),
            Class::Fat => {
                self.asm.store(4, EAX, base, disp);
                self.asm.store(4, EDX, base, disp + 4);
            }
            Class::Agg { size } => {
                self.asm.lea(EDI, base, disp);
                self.asm.mov_rr(ESI, EAX);
                self.asm.mov_ri(ECX, size);
                self.asm.rep_movsb();
            }
        }
    }

    /// Load a value of type `t` from the address in EAX.
    fn load_eax(&mut self, t: &Ty) {
        match self.class(t) {
            Class::Void | Class::Agg { .. } => {}
            Class::Scalar { size, signed } => self.asm.load(size, signed, EAX, EAX, 0),
            Class::Fat => {
                self.asm.load(4, false, EDX, EAX, 4);
                self.asm.load(4, false, EAX, EAX, 0);
            }
        }
    }

    /// Push the value in registers as a function argument; returns bytes pushed.
    fn push_value(&mut self, t: &Ty) -> u32 {
        match self.class(t) {
            Class::Void => 0,
            Class::Scalar { .. } => {
                self.asm.push(EAX);
                4
            }
            Class::Fat => {
                self.asm.push(EDX);
                self.asm.push(EAX);
                8
            }
            Class::Agg { size } => {
                let n = align_up(size, 4);
                self.asm.alu_ri(Alu::Sub, ESP, n);
                self.asm.mov_rr(EDI, ESP);
                self.asm.mov_rr(ESI, EAX);
                self.asm.mov_ri(ECX, size);
                self.asm.rep_movsb();
                n
            }
        }
    }

    /// Sign- or zero-extend EAX after arithmetic on a narrow type.
    fn normalize(&mut self, t: &Ty) {
        if let Class::Scalar { size, signed } = self.class(t) {
            if size < 4 {
                if signed {
                    self.asm.movsx_eax(size);
                } else {
                    self.asm.movzx_eax(size);
                }
            }
        }
    }

    fn coerce(&mut self, have: &Ty, want: &Ty, span: Span) -> R<()> {
        if have == want || *have == Ty::Never {
            return Ok(());
        }
        if let (Ty::Ptr(inner), Ty::Slice(e)) = (have, want) {
            if let Ty::Array(t, n) = &**inner {
                if t == e {
                    self.asm.mov_ri(EDX, *n);
                    return Ok(());
                }
            }
        }
        if let (Ty::Ptr(inner), Ty::Str) = (have, want) {
            if **inner == Ty::Str {
                self.load_eax(&Ty::Str);
                return Ok(());
            }
        }
        self.mismatch(span, want, have)
    }

    /// Compile `e` and convert it to `want`.
    fn expr_as(&mut self, e: &Expr, want: &Ty) -> R<()> {
        let t = self.expr(e, Some(want))?;
        self.coerce(&t, want, e.span)
    }

    /// Compile `e` only to learn its type; the code is discarded.
    fn probe(&mut self, e: &Expr) -> Option<Ty> {
        let snap = (self.asm.code.len(), self.asm.abs.len(), self.scopes.len(), self.loops.len(), self.frame);
        let rel = self.asm_rel_len();
        let r = self.expr(e, None);
        self.asm.code.truncate(snap.0);
        self.asm.abs.truncate(snap.1);
        self.asm_rel_truncate(rel);
        self.scopes.truncate(snap.2);
        self.loops.truncate(snap.3);
        self.frame = snap.4;
        r.ok()
    }

    fn asm_rel_len(&self) -> usize {
        self.asm.rel_len()
    }
    fn asm_rel_truncate(&mut self, n: usize) {
        self.asm.rel_truncate(n)
    }

    fn panic_here(&mut self, msg: &str, span: Span) {
        let p = self.string(msg.as_bytes());
        self.asm.push_i(span.col);
        self.asm.push_i(span.line);
        self.asm.push_i(msg.len() as u32);
        self.asm.mov_r_abs(EAX, Abs::Data(p));
        self.asm.push(EAX);
        let f = self.prelude_fn("__panic");
        self.asm.call(f.label);
    }

    // ------------------------------------------------------------ expressions

    fn addressable(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Path(p) if p.len() == 1 => {
                self.lookup_local(&p[0]).is_some() || matches!(self.globals.get(&p[0]), Some(Global::Static(..)))
            }
            ExprKind::Deref(_) | ExprKind::Field(..) => true,
            ExprKind::Index(_, i) => !matches!(i.kind, ExprKind::Range(..)),
            _ => false,
        }
    }

    /// Compile a place expression: its address in EAX, its type and whether
    /// it may be assigned.
    fn place(&mut self, e: &Expr) -> R<(Ty, bool)> {
        match &e.kind {
            ExprKind::Path(p) if p.len() == 1 => {
                if let Some(l) = self.lookup_local(&p[0]) {
                    self.asm.lea(EAX, EBP, l.off);
                    return Ok((l.ty, l.mutable));
                }
                if let Some(Global::Static(t, off)) = self.globals.get(&p[0]).cloned() {
                    self.asm.mov_r_abs(EAX, Abs::Data(off));
                    return Ok((t, true));
                }
                err(e.span, format!("cannot find value `{}` in this scope", p[0]))
            }
            ExprKind::Deref(inner) => {
                let t = self.expr(inner, None)?;
                match t {
                    Ty::Ptr(x) => Ok((*x, true)),
                    Ty::Str | Ty::Slice(_) => err(e.span, "cannot dereference a slice; index it instead"),
                    _ => err(e.span, format!("type `{}` cannot be dereferenced", self.tn(&t))),
                }
            }
            ExprKind::Field(base, f) => {
                let (bt, m) = self.deref_base(base)?;
                let id = match bt {
                    Ty::Struct(id) => id,
                    _ => return err(e.span, format!("no field `{f}` on type `{}`", self.tn(&bt))),
                };
                let field = self.structs[id].fields.iter().find(|(n, _, _)| n == f).cloned();
                match field {
                    Some((_, ft, off)) => {
                        if off != 0 {
                            self.asm.alu_ri(Alu::Add, EAX, off);
                        }
                        Ok((ft, m))
                    }
                    None => err(e.span, format!("no field `{f}` on type `{}`", self.structs[id].name)),
                }
            }
            ExprKind::Index(base, idx) => {
                if matches!(idx.kind, ExprKind::Range(..)) {
                    return err(e.span, "slicing needs a reference: write `&x[a..b]`");
                }
                let (et, len, m) = self.seq_base(base, e.span)?;
                self.asm.push(EAX);
                if len.is_none() {
                    self.asm.push(EDX);
                }
                let it = self.expr(idx, Some(&Ty::usize()))?;
                if !it.is_int() {
                    return err(idx.span, format!("the type `[{}]` cannot be indexed by `{}`", self.tn(&et), self.tn(&it)));
                }
                self.asm.mov_rr(ECX, EAX);
                match len {
                    Some(n) => self.asm.mov_ri(EDX, n),
                    None => self.asm.pop(EDX),
                }
                self.asm.pop(EAX);
                self.bounds_check(e.span);
                let es = self.size(&et);
                if es != 1 {
                    self.asm.imul_rri(ECX, ECX, es);
                }
                self.asm.alu_rr(Alu::Add, EAX, ECX);
                Ok((et, m))
            }
            _ => {
                let t = self.expr(e, None)?;
                match self.class(&t) {
                    Class::Agg { .. } => Ok((t, false)),
                    _ => err(e.span, "this expression is not a place (it cannot be assigned or borrowed here)"),
                }
            }
        }
    }

    /// ECX = index, EDX = length; panic if out of bounds.
    fn bounds_check(&mut self, span: Span) {
        let ok = self.asm.label();
        self.asm.alu_rr(Alu::Cmp, ECX, EDX);
        self.asm.jcc(Cc::B, ok);
        self.asm.push_i(span.col);
        self.asm.push_i(span.line);
        self.asm.push(EDX);
        self.asm.push(ECX);
        let f = self.prelude_fn("__panic_bounds");
        self.asm.call(f.label);
        self.asm.bind(ok);
    }

    /// A struct base for field access: follows references. EAX = struct address.
    fn deref_base(&mut self, base: &Expr) -> R<(Ty, bool)> {
        let (mut t, mut m) = if self.addressable(base) {
            self.place(base)?
        } else {
            let t = self.expr(base, None)?;
            match t {
                Ty::Ptr(inner) => (*inner, true),
                t => (t, false),
            }
        };
        while let Ty::Ptr(inner) = t {
            self.asm.load(4, false, EAX, EAX, 0);
            t = *inner;
            m = true;
        }
        Ok((t, m))
    }

    /// A sequence base for indexing: element type, constant length (arrays)
    /// or EDX = length (slices), EAX = pointer to the first element.
    fn seq_base(&mut self, base: &Expr, span: Span) -> R<(Ty, Option<u32>, bool)> {
        let (mut t, mut m, mut addr) = if self.addressable(base) {
            let (t, m) = self.place(base)?;
            (t, m, true)
        } else {
            let t = self.expr(base, None)?;
            (t, false, false)
        };
        loop {
            match t {
                // Arrays: EAX is the address (place or aggregate value).
                Ty::Array(e, n) => return Ok((*e, Some(n), m)),
                Ty::Slice(e) => {
                    if addr {
                        self.asm.load(4, false, EDX, EAX, 4);
                        self.asm.load(4, false, EAX, EAX, 0);
                    }
                    return Ok((*e, None, true));
                }
                Ty::Str => return err(span, "strings cannot be indexed by integers; use `s.as_bytes()[i]`"),
                Ty::Ptr(inner) => {
                    if addr {
                        self.asm.load(4, false, EAX, EAX, 0);
                    }
                    addr = true;
                    m = true;
                    t = *inner;
                }
                other => return err(span, format!("cannot index into a value of type `{}`", self.tn(&other))),
            }
        }
    }

    pub fn expr(&mut self, e: &Expr, want: Option<&Ty>) -> R<Ty> {
        let span = e.span;
        match &e.kind {
            ExprKind::Int(v, suffix) => self.int_lit(*v as i64, suffix.as_deref(), want, span),
            ExprKind::Bool(b) => {
                self.asm.mov_ri(EAX, *b as u32);
                Ok(Ty::Bool)
            }
            ExprKind::Char(c) => {
                self.asm.mov_ri(EAX, *c);
                Ok(Ty::Char)
            }
            ExprKind::Byte(c) => {
                self.asm.mov_ri(EAX, *c as u32);
                Ok(Ty::Int(IntTy::U8))
            }
            ExprKind::Str(s) => {
                let p = self.string(s);
                self.asm.mov_r_abs(EAX, Abs::Data(p));
                self.asm.mov_ri(EDX, s.len() as u32);
                Ok(Ty::Str)
            }
            ExprKind::ByteStr(s) => {
                let p = self.string(s);
                self.asm.mov_r_abs(EAX, Abs::Data(p));
                let arr = Ty::Array(Box::new(Ty::Int(IntTy::U8)), s.len() as u32);
                if let Some(Ty::Ptr(w)) = want {
                    if **w == arr {
                        return Ok(Ty::ptr(arr));
                    }
                }
                self.asm.mov_ri(EDX, s.len() as u32);
                Ok(Ty::Slice(Box::new(Ty::Int(IntTy::U8))))
            }
            ExprKind::Unit => Ok(Ty::Unit),
            ExprKind::Path(p) => self.path(p, want, span),
            ExprKind::Unary(op, x) => self.unary(*op, x, want, span),
            ExprKind::Binary(op, a, b) => self.binary(*op, a, b, want, span),
            ExprKind::Assign(l, r) => {
                if let ExprKind::Path(p) = &l.kind {
                    if p.len() == 1 && p[0] == "_" {
                        self.expr(r, None)?;
                        return Ok(Ty::Unit);
                    }
                }
                let (t, m) = self.place(l)?;
                if !m {
                    return err(l.span, self.immutable_msg(l));
                }
                self.asm.push(EAX);
                self.expr_as(r, &t)?;
                self.asm.pop(EDI);
                self.store_to(&t, EDI, 0);
                Ok(Ty::Unit)
            }
            ExprKind::OpAssign(op, l, r) => {
                let (t, m) = self.place(l)?;
                if !m {
                    return err(l.span, self.immutable_msg(l));
                }
                if !t.is_int() && !(t == Ty::Bool && matches!(op, BinOp::And | BinOp::Or | BinOp::Xor)) {
                    return err(span, format!("cannot use `{}=` on type `{}`", op_str(*op), self.tn(&t)));
                }
                self.asm.push(EAX);
                self.load_eax(&t);
                self.asm.push(EAX);
                let rt = if matches!(op, BinOp::Shl | BinOp::Shr) {
                    self.expr(r, None)?
                } else {
                    let rt = self.expr(r, Some(&t))?;
                    if rt != t {
                        return self.mismatch(r.span, &t, &rt);
                    }
                    rt
                };
                if !rt.is_int() && rt != Ty::Bool {
                    return err(r.span, format!("cannot use a `{}` here", self.tn(&rt)));
                }
                self.asm.mov_rr(ECX, EAX);
                self.asm.pop(EAX);
                self.arith(*op, &t, span);
                self.asm.pop(EDI);
                self.store_to(&t, EDI, 0);
                Ok(Ty::Unit)
            }
            ExprKind::Call(path, args) => self.call(path, args, want, span),
            ExprKind::MethodCall(recv, name, args) => self.method_call(recv, name, args, span),
            ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Deref(_) => {
                let (t, _) = self.place(e)?;
                self.load_eax(&t);
                Ok(t)
            }
            ExprKind::Range(..) => err(span, "ranges can only be used in `for` loops and slices"),
            ExprKind::Cast(x, te) => self.cast(x, te, span),
            ExprKind::Ref(inner) => self.reference(inner, want, span),
            ExprKind::Block(b) => self.block(b, want),
            ExprKind::If(c, then, els) => self.if_expr(c, then, els.as_deref(), want),
            ExprKind::While(c, body) => {
                let (brk, cont) = (self.asm.label(), self.asm.label());
                self.asm.bind(cont);
                self.expr_as(c, &Ty::Bool)?;
                self.asm.test_rr(EAX, EAX);
                self.asm.jcc(Cc::E, brk);
                self.loops.push(LoopCtx { brk, cont, broke: false });
                self.unit_block(body)?;
                self.loops.pop();
                self.asm.jmp(cont);
                self.asm.bind(brk);
                Ok(Ty::Unit)
            }
            ExprKind::Loop(body) => {
                let (brk, cont) = (self.asm.label(), self.asm.label());
                self.asm.bind(cont);
                self.loops.push(LoopCtx { brk, cont, broke: false });
                self.unit_block(body)?;
                let broke = self.loops.pop().unwrap().broke;
                self.asm.jmp(cont);
                self.asm.bind(brk);
                Ok(if broke { Ty::Unit } else { Ty::Never })
            }
            ExprKind::For(var, it, body) => self.for_loop(var, it, body, span),
            ExprKind::Break => match self.loops.last_mut() {
                Some(l) => {
                    l.broke = true;
                    let b = l.brk;
                    self.asm.jmp(b);
                    Ok(Ty::Never)
                }
                None => err(span, "`break` outside of a loop"),
            },
            ExprKind::Continue => match self.loops.last() {
                Some(l) => {
                    let c = l.cont;
                    self.asm.jmp(c);
                    Ok(Ty::Never)
                }
                None => err(span, "`continue` outside of a loop"),
            },
            ExprKind::Return(v) => {
                let rt = self.ret_ty.clone();
                match v {
                    Some(x) => self.expr_as(x, &rt)?,
                    None => {
                        if rt != Ty::Unit {
                            return err(span, format!("`return;` in a function returning `{}`", self.tn(&rt)));
                        }
                    }
                }
                self.ret_value();
                let l = self.ret_label;
                self.asm.jmp(l);
                Ok(Ty::Never)
            }
            ExprKind::Match(s, arms) => self.match_expr(s, arms, want, span),
            ExprKind::StructLit(name, fields) => self.struct_lit(name, fields, span),
            ExprKind::ArrayLit(xs) => self.array_lit(xs, want, span),
            ExprKind::ArrayRepeat(x, n) => self.array_repeat(x, n, want, span),
            ExprKind::Macro(name, args) => self.macro_call(name, args, span),
        }
    }

    fn immutable_msg(&self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Path(p) => format!("cannot assign twice to immutable variable `{}` (declare it with `let mut`)", p[0]),
            _ => String::from("cannot assign to this expression: it is not mutable"),
        }
    }

    fn int_lit(&mut self, v: i64, suffix: Option<&str>, want: Option<&Ty>, span: Span) -> R<Ty> {
        let it = match suffix {
            Some(s) => match IntTy::from_name(s) {
                Some(i) => i,
                None => return err(span, format!("invalid suffix `{s}` for a number literal")),
            },
            None => match want {
                Some(Ty::Int(i)) => *i,
                _ => IntTy::I32,
            },
        };
        if !it.fits(v) {
            return err(span, format!("literal out of range for `{}`", it.name()));
        }
        self.asm.mov_ri(EAX, v as u32);
        Ok(Ty::Int(it))
    }

    fn path(&mut self, p: &[String], want: Option<&Ty>, span: Span) -> R<Ty> {
        if p.len() == 1 {
            if let Some(l) = self.lookup_local(&p[0]) {
                match self.class(&l.ty) {
                    Class::Void => {}
                    Class::Scalar { size, signed } => self.asm.load(size, signed, EAX, EBP, l.off),
                    Class::Fat => {
                        self.asm.load(4, false, EAX, EBP, l.off);
                        self.asm.load(4, false, EDX, EBP, l.off + 4);
                    }
                    Class::Agg { .. } => self.asm.lea(EAX, EBP, l.off),
                }
                return Ok(l.ty);
            }
            match self.globals.get(&p[0]).cloned() {
                Some(Global::Const(t, e)) => {
                    self.const_depth += 1;
                    if self.const_depth > 32 {
                        return err(span, "constant refers to itself");
                    }
                    let r = self.expr_as(&e, &t);
                    self.const_depth -= 1;
                    r?;
                    return Ok(t);
                }
                Some(Global::Static(t, off)) => {
                    self.asm.mov_r_abs(EAX, Abs::Data(off));
                    self.load_eax(&t);
                    return Ok(t);
                }
                None => {}
            }
            if self.lookup_fn(&p[0]).is_some() {
                return err(span, "function pointers are not supported; call the function instead");
            }
            return err(span, format!("cannot find value `{}` in this scope", p[0]));
        }
        if p.len() == 2 {
            let owner = if p[0] == "Self" {
                match &self.self_ty {
                    Some(Ty::Enum(id)) => self.enums[*id].name.clone(),
                    Some(t) => self.tn(&t.clone()),
                    None => return err(span, "`Self` is only available in impl blocks"),
                }
            } else {
                p[0].clone()
            };
            if let Some(&id) = self.enum_ids.get(&owner) {
                match self.enums[id].variants.iter().find(|(n, _)| *n == p[1]) {
                    Some((_, v)) => {
                        self.asm.mov_ri(EAX, *v as u32);
                        return Ok(Ty::Enum(id));
                    }
                    None => return err(span, format!("no variant named `{}` in enum `{owner}`", p[1])),
                }
            }
            if let Some(i) = IntTy::from_name(&owner) {
                if let Some(v) = self.path_const(&[owner.clone(), p[1].clone()]) {
                    self.asm.mov_ri(EAX, v as u32);
                    return Ok(Ty::Int(if p[1] == "BITS" { IntTy::U32 } else { i }));
                }
            }
        }
        let _ = want;
        // std::f::NAME style: use the last segment.
        let last = &p[p.len() - 1];
        if self.globals.contains_key(last) {
            return self.path(&p[p.len() - 1..], want, span);
        }
        err(span, format!("cannot find value `{}`", p.join("::")))
    }

    fn unary(&mut self, op: UnOp, x: &Expr, want: Option<&Ty>, span: Span) -> R<Ty> {
        match op {
            UnOp::Neg => {
                if let ExprKind::Int(v, suffix) = &x.kind {
                    let t = self.int_lit(-(*v as i64), suffix.as_deref(), want, span)?;
                    if !t.signed() {
                        return err(span, format!("cannot apply unary operator `-` to type `{}`", self.tn(&t)));
                    }
                    return Ok(t);
                }
                let t = self.expr(x, want)?;
                if !t.signed() {
                    return err(span, format!("cannot apply unary operator `-` to type `{}`", self.tn(&t)));
                }
                self.asm.neg(EAX);
                self.normalize(&t);
                Ok(t)
            }
            UnOp::Not => {
                let t = self.expr(x, want)?;
                match t {
                    Ty::Bool => self.asm.alu_ri(Alu::Xor, EAX, 1),
                    Ty::Int(_) => {
                        self.asm.not(EAX);
                        self.normalize(&t);
                    }
                    _ => return err(span, format!("cannot apply unary operator `!` to type `{}`", self.tn(&t))),
                }
                Ok(t)
            }
        }
    }

    /// The type an operand pair should be compiled at.
    fn operand_ty(&mut self, a: &Expr, b: &Expr, want: Option<&Ty>) -> Option<Ty> {
        if !is_untyped_lit(a) {
            return None;
        }
        if let Some(w @ Ty::Int(_)) = want {
            return Some(w.clone());
        }
        match self.probe(b) {
            Some(t @ Ty::Int(_)) => Some(t),
            _ => None,
        }
    }

    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr, want: Option<&Ty>, span: Span) -> R<Ty> {
        if matches!(op, BinOp::LAnd | BinOp::LOr) {
            let done = self.asm.label();
            self.expr_as(a, &Ty::Bool)?;
            self.asm.test_rr(EAX, EAX);
            self.asm.jcc(if op == BinOp::LAnd { Cc::E } else { Cc::Ne }, done);
            self.expr_as(b, &Ty::Bool)?;
            self.asm.bind(done);
            return Ok(Ty::Bool);
        }
        let cmp = is_cmp(op);
        let lwant = if cmp { None } else { want.filter(|w| w.is_int()) };
        let lhint = self.operand_ty(a, b, lwant);
        let lt = self.expr(a, lhint.as_ref().or(lwant))?;
        // Strings compare by content.
        if matches!(lt, Ty::Str) && matches!(op, BinOp::Eq | BinOp::Ne) {
            self.asm.push(EDX);
            self.asm.push(EAX);
            self.expr_as(b, &Ty::Str)?;
            self.asm.push(EDX);
            self.asm.push(EAX);
            let f = self.prelude_fn("__str_eq");
            self.asm.call(f.label);
            self.asm.alu_ri(Alu::Add, ESP, 16);
            if op == BinOp::Ne {
                self.asm.alu_ri(Alu::Xor, EAX, 1);
            }
            return Ok(Ty::Bool);
        }
        let ok_operand = match &lt {
            Ty::Int(_) => true,
            Ty::Bool => matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) || cmp,
            Ty::Char | Ty::Ptr(_) => cmp,
            Ty::Enum(_) => matches!(op, BinOp::Eq | BinOp::Ne),
            _ => false,
        };
        if !ok_operand {
            return err(span, format!("binary operation `{}` cannot be applied to type `{}`", op_str(op), self.tn(&lt)));
        }
        self.asm.push(EAX);
        let rt = if matches!(op, BinOp::Shl | BinOp::Shr) {
            let rt = self.expr(b, None)?;
            if !rt.is_int() {
                return err(b.span, format!("cannot shift by a `{}`", self.tn(&rt)));
            }
            rt
        } else {
            let rt = self.expr(b, Some(&lt))?;
            if rt != lt && rt != Ty::Never {
                return self.mismatch(b.span, &lt, &rt);
            }
            rt
        };
        let _ = rt;
        self.asm.mov_rr(ECX, EAX);
        self.asm.pop(EAX);
        if cmp {
            let signed = lt.signed();
            let cc = match (op, signed) {
                (BinOp::Eq, _) => Cc::E,
                (BinOp::Ne, _) => Cc::Ne,
                (BinOp::Lt, true) => Cc::L,
                (BinOp::Le, true) => Cc::Le,
                (BinOp::Gt, true) => Cc::G,
                (BinOp::Ge, true) => Cc::Ge,
                (BinOp::Lt, false) => Cc::B,
                (BinOp::Le, false) => Cc::Be,
                (BinOp::Gt, false) => Cc::A,
                _ => Cc::Ae,
            };
            self.asm.alu_rr(Alu::Cmp, EAX, ECX);
            self.asm.setcc_eax(cc);
            return Ok(Ty::Bool);
        }
        self.arith(op, &lt, span);
        Ok(lt)
    }

    /// EAX = EAX op ECX at type `t`.
    fn arith(&mut self, op: BinOp, t: &Ty, span: Span) {
        let signed = t.signed();
        match op {
            BinOp::Add => self.asm.alu_rr(Alu::Add, EAX, ECX),
            BinOp::Sub => self.asm.alu_rr(Alu::Sub, EAX, ECX),
            BinOp::Mul => self.asm.imul_rr(EAX, ECX),
            BinOp::And => self.asm.alu_rr(Alu::And, EAX, ECX),
            BinOp::Or => self.asm.alu_rr(Alu::Or, EAX, ECX),
            BinOp::Xor => self.asm.alu_rr(Alu::Xor, EAX, ECX),
            BinOp::Shl => self.asm.shift_cl(4, EAX),
            BinOp::Shr => self.asm.shift_cl(if signed { 7 } else { 5 }, EAX),
            BinOp::Div | BinOp::Rem => {
                let ok = self.asm.label();
                self.asm.test_rr(ECX, ECX);
                self.asm.jcc(Cc::Ne, ok);
                let msg = if op == BinOp::Div {
                    "attempt to divide by zero"
                } else {
                    "attempt to calculate the remainder with a divisor of zero"
                };
                self.panic_here(msg, span);
                self.asm.bind(ok);
                if signed {
                    self.asm.cdq();
                    self.asm.idiv(ECX);
                } else {
                    self.asm.mov_ri(EDX, 0);
                    self.asm.div(ECX);
                }
                if op == BinOp::Rem {
                    self.asm.mov_rr(EAX, EDX);
                }
            }
            _ => unreachable!(),
        }
        self.normalize(t);
    }

    fn cast(&mut self, x: &Expr, te: &TypeExpr, span: Span) -> R<Ty> {
        let to = self.resolve_ty(te)?;
        let hint = if to.is_int() && is_untyped_lit(x) { Some(Ty::Int(IntTy::I32)) } else { None };
        let from = self.expr(x, hint.as_ref())?;
        let ok = match (&from, &to) {
            (a, b) if a == b => true,
            (Ty::Int(_) | Ty::Bool | Ty::Char | Ty::Enum(_), Ty::Int(_)) => true,
            (Ty::Int(IntTy { size: 1, signed: false, .. }), Ty::Char) => true,
            (Ty::Int(_), Ty::Char) => true,
            (Ty::Ptr(_), Ty::Ptr(_)) => true,
            (Ty::Ptr(_), Ty::Int(IntTy { size: 4, .. })) => true,
            (Ty::Int(IntTy { size: 4, .. }), Ty::Ptr(_)) => true,
            _ => false,
        };
        if !ok {
            return err(span, format!("non-primitive cast: `{}` as `{}`", self.tn(&from), self.tn(&to)));
        }
        self.normalize(&to);
        Ok(to)
    }

    fn reference(&mut self, inner: &Expr, want: Option<&Ty>, span: Span) -> R<Ty> {
        if let ExprKind::Index(base, idx) = &inner.kind {
            if let ExprKind::Range(lo, hi, incl) = &idx.kind {
                return self.slice_range(base, lo.as_deref(), hi.as_deref(), *incl, span);
            }
        }
        if self.addressable(inner) {
            let (t, _) = self.place(inner)?;
            return Ok(Ty::ptr(t));
        }
        let inner_want = match want {
            Some(Ty::Ptr(w)) => Some((**w).clone()),
            Some(Ty::Slice(e)) => Some(Ty::Slice(e.clone())),
            _ => None,
        };
        let t = self.expr(inner, inner_want.as_ref())?;
        match self.class(&t) {
            Class::Agg { .. } => Ok(Ty::ptr(t)),
            Class::Void => err(span, "cannot take a reference to `()`"),
            _ => {
                let slot = self.alloc_slot(&t);
                self.store_to(&t, EBP, slot);
                self.asm.lea(EAX, EBP, slot);
                Ok(Ty::ptr(t))
            }
        }
    }

    fn slice_range(&mut self, base: &Expr, lo: Option<&Expr>, hi: Option<&Expr>, incl: bool, span: Span) -> R<Ty> {
        // Base pointer and length.
        let is_str = matches!(self.probe(base), Some(Ty::Str));
        let et = if is_str {
            self.expr(base, None)?;
            Ty::Int(IntTy::U8)
        } else {
            let (et, len, _) = self.seq_base(base, span)?;
            if let Some(n) = len {
                self.asm.mov_ri(EDX, n);
            }
            et
        };
        self.asm.push(EAX);
        self.asm.push(EDX);
        match lo {
            Some(l) => {
                let t = self.expr(l, Some(&Ty::usize()))?;
                if !t.is_int() {
                    return err(l.span, "slice bounds must be integers");
                }
            }
            None => self.asm.mov_ri(EAX, 0),
        }
        self.asm.push(EAX);
        match hi {
            Some(h) => {
                let t = self.expr(h, Some(&Ty::usize()))?;
                if !t.is_int() {
                    return err(h.span, "slice bounds must be integers");
                }
                if incl {
                    self.asm.alu_ri(Alu::Add, EAX, 1);
                }
                self.asm.mov_rr(ECX, EAX);
            }
            None => self.asm.load(4, false, ECX, ESP, 4),
        }
        self.asm.pop(EAX); // lo
        self.asm.pop(EDX); // len
        // lo <= hi <= len
        let bad = self.asm.label();
        let ok = self.asm.label();
        self.asm.alu_rr(Alu::Cmp, ECX, EDX);
        self.asm.jcc(Cc::A, bad);
        self.asm.alu_rr(Alu::Cmp, EAX, ECX);
        self.asm.jcc(Cc::Be, ok);
        self.asm.bind(bad);
        self.panic_here("slice index out of range", span);
        self.asm.bind(ok);
        self.asm.alu_rr(Alu::Sub, ECX, EAX);
        let es = self.size(&et);
        if es != 1 {
            self.asm.imul_rri(EAX, EAX, es);
        }
        self.asm.pop(EDX);
        self.asm.alu_rr(Alu::Add, EAX, EDX);
        self.asm.mov_rr(EDX, ECX);
        Ok(if is_str { Ty::Str } else { Ty::Slice(Box::new(et)) })
    }

    fn block(&mut self, b: &Block, want: Option<&Ty>) -> R<Ty> {
        self.scopes.push(Vec::new());
        let mut diverges = false;
        for (si, s) in b.stmts.iter().enumerate() {
            match s {
                Stmt::Let { name, mutable, ty, init, span } => {
                    let declared = match ty {
                        Some(t) => Some(self.resolve_ty(t)?),
                        None => None,
                    };
                    let t = match (init, &declared) {
                        (Some(e), Some(d)) => {
                            self.expr_as(e, d)?;
                            d.clone()
                        }
                        (Some(e), None) => {
                            // `let x = 0;`: look ahead for how x is used.
                            let hint = if is_untyped_lit(e) {
                                self.infer_from_uses(name, &b.stmts[si + 1..], b.tail.as_deref(), want)
                            } else {
                                None
                            };
                            let t = self.expr(e, hint.as_ref())?;
                            if t == Ty::Never {
                                diverges = true;
                            }
                            t
                        }
                        (None, Some(d)) => {
                            // Uninitialised: zero it so behaviour is defined.
                            let t = d.clone();
                            let off = self.declare_local(name, t.clone(), *mutable);
                            let size = self.size(&t);
                            self.asm.lea(EDI, EBP, off);
                            self.asm.mov_ri(EAX, 0);
                            self.asm.mov_ri(ECX, size);
                            self.asm.rep_stosb();
                            continue;
                        }
                        (None, None) => return err(*span, "type annotations needed: `let x: T;`"),
                    };
                    let off = self.declare_local(name, t.clone(), *mutable);
                    self.store_to(&t, EBP, off);
                }
                Stmt::Expr(e) => {
                    let t = self.expr(e, None)?;
                    if t == Ty::Never {
                        diverges = true;
                    }
                }
            }
        }
        let t = match &b.tail {
            Some(e) => self.expr(e, want)?,
            None if diverges => Ty::Never,
            None => Ty::Unit,
        };
        self.scopes.pop();
        Ok(if diverges && b.tail.is_none() { Ty::Never } else { t })
    }

    /// The integer type the uses of local `x` imply, scanning statements in
    /// order (operands, assignments, arguments, indexes, fields, returns).
    fn infer_from_uses(&mut self, x: &str, stmts: &[Stmt], tail: Option<&Expr>, want: Option<&Ty>) -> Option<Ty> {
        for st in stmts {
            let found = match st {
                Stmt::Let { name, ty, init: Some(e), .. } => {
                    let d = ty.as_ref().and_then(|t| self.resolve_ty(t).ok());
                    let h = match d {
                        Some(d) if mentions(e, x) => Some(d),
                        _ => self.hint_in(e, x),
                    };
                    if h.is_none() && name == x {
                        return None; // shadowed
                    }
                    h
                }
                Stmt::Let { .. } => None,
                Stmt::Expr(e) => self.hint_in(e, x),
            };
            if let Some(t) = found {
                return Some(t);
            }
        }
        let t = tail?;
        if let (true, Some(w @ Ty::Int(_))) = (mentions(t, x), want) {
            return Some(w.clone());
        }
        self.hint_in(t, x)
    }

    fn int_probe(&mut self, e: &Expr) -> Option<Ty> {
        if is_untyped_lit(e) {
            return None;
        }
        match self.probe(e) {
            Some(t @ Ty::Int(_)) => Some(t),
            _ => None,
        }
    }

    fn hint_in(&mut self, e: &Expr, x: &str) -> Option<Ty> {
        let sub = |g: &mut Gen, es: &[&Expr]| es.iter().find_map(|c| g.hint_in(c, x));
        match &e.kind {
            ExprKind::Binary(op, a, b) if !matches!(op, BinOp::LAnd | BinOp::LOr) => {
                let shift = matches!(op, BinOp::Shl | BinOp::Shr);
                if mentions(a, x) && !mentions(b, x) && !shift {
                    if let Some(t) = self.int_probe(b) {
                        return Some(t);
                    }
                }
                if mentions(b, x) && !mentions(a, x) && !shift {
                    if let Some(t) = self.int_probe(a) {
                        return Some(t);
                    }
                }
                sub(self, &[a, b])
            }
            ExprKind::Assign(l, r) | ExprKind::OpAssign(_, l, r) => {
                let shift = matches!(e.kind, ExprKind::OpAssign(BinOp::Shl | BinOp::Shr, ..));
                if mentions(l, x) && !mentions(r, x) && !shift {
                    if let Some(t) = self.int_probe(r) {
                        return Some(t);
                    }
                }
                if mentions(r, x) && !mentions(l, x) && !shift {
                    if let Some(t) = self.int_probe(l) {
                        return Some(t);
                    }
                }
                sub(self, &[l, r])
            }
            ExprKind::Index(a, i) => {
                if mentions(i, x) && !matches!(i.kind, ExprKind::Range(..)) {
                    return Some(Ty::usize());
                }
                sub(self, &[a, i])
            }
            ExprKind::Call(path, args) => {
                let key = path.join("::");
                if let Some(sig) = self.lookup_fn(&key) {
                    let skip = sig.self_kind.is_some() as usize;
                    let _ = skip;
                    for (i, a) in args.iter().enumerate() {
                        if mentions(a, x) {
                            if let Some(t @ Ty::Int(_)) = sig.params.get(i) {
                                return Some(t.clone());
                            }
                        }
                    }
                }
                args.iter().find_map(|a| self.hint_in(a, x))
            }
            ExprKind::StructLit(name, fields) => {
                let id = *self.struct_ids.get(name)?;
                for (f, v) in fields {
                    if mentions(v, x) {
                        let ft = self.structs[id].fields.iter().find(|(n, _, _)| n == f).map(|(_, t, _)| t.clone());
                        if let Some(t @ Ty::Int(_)) = ft {
                            return Some(t);
                        }
                    }
                }
                fields.iter().find_map(|(_, v)| self.hint_in(v, x))
            }
            ExprKind::Return(Some(v)) => {
                if mentions(v, x) {
                    if let t @ Ty::Int(_) = self.ret_ty.clone() {
                        return Some(t);
                    }
                }
                self.hint_in(v, x)
            }
            ExprKind::Range(a, b, _) => {
                let (a, b) = (a.as_deref(), b.as_deref());
                if let (Some(a), Some(b)) = (a, b) {
                    if mentions(a, x) && !mentions(b, x) {
                        if let Some(t) = self.int_probe(b) {
                            return Some(t);
                        }
                    }
                    if mentions(b, x) && !mentions(a, x) {
                        if let Some(t) = self.int_probe(a) {
                            return Some(t);
                        }
                    }
                }
                a.into_iter().chain(b).find_map(|c| self.hint_in(c, x))
            }
            ExprKind::Unary(_, a) | ExprKind::Cast(a, _) | ExprKind::Ref(a) | ExprKind::Deref(a) | ExprKind::Field(a, _) => {
                self.hint_in(a, x)
            }
            ExprKind::MethodCall(r, _, args) => {
                if let Some(t) = self.hint_in(r, x) {
                    return Some(t);
                }
                args.iter().find_map(|a| self.hint_in(a, x))
            }
            ExprKind::Macro(_, args) | ExprKind::ArrayLit(args) => args.iter().find_map(|a| self.hint_in(a, x)),
            ExprKind::Block(b) | ExprKind::Loop(b) => self.hint_block(b, x),
            ExprKind::While(c, b) => self.hint_in(c, x).or_else(|| self.hint_block(b, x)),
            ExprKind::If(c, b, els) => self
                .hint_in(c, x)
                .or_else(|| self.hint_block(b, x))
                .or_else(|| els.as_ref().and_then(|e| self.hint_in(e, x))),
            ExprKind::For(v, it, b) => self.hint_in(it, x).or_else(|| if v == x { None } else { self.hint_block(b, x) }),
            ExprKind::Match(sc, arms) => self.hint_in(sc, x).or_else(|| {
                arms.iter().find_map(|a| a.guard.as_ref().and_then(|g| self.hint_in(g, x)).or_else(|| self.hint_in(&a.body, x)))
            }),
            _ => None,
        }
    }

    fn hint_block(&mut self, b: &Block, x: &str) -> Option<Ty> {
        // Nested blocks may use locals declared inside them, which probing
        // cannot see; those uses simply give no hint.
        self.infer_from_uses(x, &b.stmts, b.tail.as_deref(), None)
    }

    fn unit_block(&mut self, b: &Block) -> R<()> {
        let t = self.block(b, Some(&Ty::Unit))?;
        if t != Ty::Unit && t != Ty::Never {
            let span = b.tail.as_ref().map(|e| e.span).unwrap_or(b.span);
            return err(span, format!("mismatched types: expected `()`, found `{}` (add a `;`)", self.tn(&t)));
        }
        Ok(())
    }

    fn if_expr(&mut self, c: &Expr, then: &Block, els: Option<&Expr>, want: Option<&Ty>) -> R<Ty> {
        let else_l = self.asm.label();
        self.expr_as(c, &Ty::Bool)?;
        self.asm.test_rr(EAX, EAX);
        self.asm.jcc(Cc::E, else_l);
        let tt = self.block(then, want)?;
        let Some(els) = els else {
            if tt != Ty::Unit && tt != Ty::Never {
                let span = then.tail.as_ref().map(|e| e.span).unwrap_or(then.span);
                return err(span, format!("`if` without `else` must have type `()`, found `{}`", self.tn(&tt)));
            }
            self.asm.bind(else_l);
            return Ok(Ty::Unit);
        };
        let end = self.asm.label();
        let result = if tt == Ty::Never { None } else { Some(tt.clone()) };
        // Coerce the then-branch to the expected type where that differs.
        let target = match (want, &result) {
            (Some(w), Some(t)) if w != t => {
                self.coerce(t, w, then.span)?;
                Some(w.clone())
            }
            _ => result.clone(),
        };
        self.asm.jmp(end);
        self.asm.bind(else_l);
        let et = self.expr(els, target.as_ref().or(want))?;
        let out = match target {
            Some(t) => {
                if et != Ty::Never {
                    self.coerce(&et, &t, els.span)
                        .map_err(|_| Error::new(els.span, &format!("`if` and `else` have incompatible types: `{}` and `{}`", self.tn(&t), self.tn(&et))))?;
                }
                t
            }
            None => et,
        };
        self.asm.bind(end);
        Ok(out)
    }

    fn for_loop(&mut self, var: &str, it: &Expr, body: &Block, span: Span) -> R<Ty> {
        enum Src<'e> {
            Range(&'e Expr, &'e Expr, bool, bool, Option<&'e Expr>),
            Seq(&'e Expr, bool),
            Bytes(&'e Expr, bool),
        }
        let src = match &it.kind {
            ExprKind::Range(Some(lo), Some(hi), incl) => Src::Range(lo, hi, *incl, false, None),
            ExprKind::Range(..) => return err(it.span, "`for` needs a range with both ends, like `0..n`"),
            ExprKind::MethodCall(r, m, a) if matches!(r.kind, ExprKind::Range(Some(_), Some(_), _)) => {
                let ExprKind::Range(Some(lo), Some(hi), incl) = &r.kind else { unreachable!() };
                match (m.as_str(), a.len()) {
                    ("rev", 0) => Src::Range(lo, hi, *incl, true, None),
                    ("step_by", 1) => Src::Range(lo, hi, *incl, false, Some(&a[0])),
                    _ => return err(it.span, format!("unsupported range method `{m}` (use .rev() or .step_by(n))")),
                }
            }
            ExprKind::MethodCall(x, m, a) if a.is_empty() && (m == "iter" || m == "iter_mut") => Src::Seq(x, true),
            ExprKind::MethodCall(x, m, a) if a.is_empty() && (m == "bytes" || m == "chars") => Src::Bytes(x, m == "chars"),
            ExprKind::Ref(x) => Src::Seq(x, true),
            _ => match self.probe(it) {
                Some(Ty::Array(..)) => Src::Seq(it, false),
                Some(Ty::Slice(_)) | Some(Ty::Ptr(_)) => Src::Seq(it, true),
                Some(Ty::Str) => return err(it.span, "iterate a string with `.bytes()` or `.chars()`"),
                Some(t) => return err(it.span, format!("`{}` is not an iterator", self.tn(&t))),
                None => {
                    self.expr(it, None)?;
                    unreachable!()
                }
            },
        };
        let is_bytes = matches!(src, Src::Bytes(..));
        self.scopes.push(Vec::new());
        let (brk, cont, top) = (self.asm.label(), self.asm.label(), self.asm.label());
        match src {
            Src::Range(lo, hi, incl, rev, step) => {
                let body_hint = if is_untyped_lit(lo) && is_untyped_lit(hi) {
                    self.infer_from_uses(var, &body.stmts, body.tail.as_deref(), None)
                } else {
                    None
                };
                let t = match body_hint.or_else(|| self.operand_ty(lo, hi, None)) {
                    Some(t) => t,
                    None => match self.probe(lo) {
                        Some(t @ Ty::Int(_)) | Some(t @ Ty::Char) => t,
                        Some(t) => return err(lo.span, format!("cannot iterate over a range of `{}`", self.tn(&t))),
                        None => Ty::Int(IntTy::I32),
                    },
                };
                let signed = t.signed();
                self.expr_as(lo, &t)?;
                let lo_slot = self.alloc_slot(&Ty::Int(IntTy::U32));
                self.asm.store(4, EAX, EBP, lo_slot);
                self.expr_as(hi, &t)?;
                if incl {
                    self.asm.alu_ri(Alu::Add, EAX, 1);
                }
                let hi_slot = self.alloc_slot(&Ty::Int(IntTy::U32));
                self.asm.store(4, EAX, EBP, hi_slot);
                let step_slot = match step {
                    Some(s) => {
                        let st = self.expr(s, Some(&Ty::usize()))?;
                        if !st.is_int() {
                            return err(s.span, "step_by needs an integer");
                        }
                        let sl = self.alloc_slot(&Ty::usize());
                        self.asm.store(4, EAX, EBP, sl);
                        Some(sl)
                    }
                    None => None,
                };
                let cur = self.alloc_slot(&Ty::Int(IntTy::U32));
                let var_off = self.declare_local(var, t.clone(), false);
                if rev {
                    // cur = hi; while cur > lo { cur -= 1; var = cur; body }
                    self.asm.load(4, false, EAX, EBP, hi_slot);
                    self.asm.store(4, EAX, EBP, cur);
                    self.asm.bind(top);
                    self.asm.bind(cont);
                    self.asm.load(4, false, EAX, EBP, cur);
                    self.asm.load(4, false, ECX, EBP, lo_slot);
                    self.asm.alu_rr(Alu::Cmp, EAX, ECX);
                    self.asm.jcc(if signed { Cc::Le } else { Cc::Be }, brk);
                    self.asm.alu_ri(Alu::Sub, EAX, 1);
                    self.asm.store(4, EAX, EBP, cur);
                    self.store_to(&t, EBP, var_off);
                } else {
                    self.asm.load(4, false, EAX, EBP, lo_slot);
                    self.asm.store(4, EAX, EBP, cur);
                    self.asm.bind(top);
                    self.asm.load(4, false, EAX, EBP, cur);
                    self.asm.load(4, false, ECX, EBP, hi_slot);
                    self.asm.alu_rr(Alu::Cmp, EAX, ECX);
                    self.asm.jcc(if signed { Cc::Ge } else { Cc::Ae }, brk);
                    self.store_to(&t, EBP, var_off);
                }
                self.loops.push(LoopCtx { brk, cont, broke: false });
                self.unit_block(body)?;
                self.loops.pop();
                if !rev {
                    self.asm.bind(cont);
                    self.asm.load(4, false, EAX, EBP, cur);
                    match step_slot {
                        Some(sl) => {
                            self.asm.load(4, false, ECX, EBP, sl);
                            self.asm.alu_rr(Alu::Add, EAX, ECX);
                        }
                        None => self.asm.alu_ri(Alu::Add, EAX, 1),
                    }
                    self.asm.store(4, EAX, EBP, cur);
                }
                self.asm.jmp(top);
            }
            Src::Seq(x, by_ref) | Src::Bytes(x, by_ref) => {
                let chars = is_bytes && by_ref;
                let bytes = is_bytes;
                let et = if bytes {
                    let t = self.expr(x, None)?;
                    if t != Ty::Str {
                        return err(x.span, format!("`.bytes()`/`.chars()` need a `&str`, found `{}`", self.tn(&t)));
                    }
                    Ty::Int(IntTy::U8)
                } else {
                    let (et, len, _) = self.seq_base(x, span)?;
                    if let Some(n) = len {
                        self.asm.mov_ri(EDX, n);
                    }
                    et
                };
                let by_ref = by_ref && !bytes;
                let p_slot = self.alloc_slot(&Ty::usize());
                let n_slot = self.alloc_slot(&Ty::usize());
                let i_slot = self.alloc_slot(&Ty::usize());
                self.asm.store(4, EAX, EBP, p_slot);
                self.asm.store(4, EDX, EBP, n_slot);
                self.asm.mov_ri(EAX, 0);
                self.asm.store(4, EAX, EBP, i_slot);
                let vt = if by_ref {
                    Ty::ptr(et.clone())
                } else if chars {
                    Ty::Char
                } else {
                    et.clone()
                };
                let var_off = self.declare_local(var, vt.clone(), false);
                self.asm.bind(top);
                self.asm.load(4, false, EAX, EBP, i_slot);
                self.asm.load(4, false, ECX, EBP, n_slot);
                self.asm.alu_rr(Alu::Cmp, EAX, ECX);
                self.asm.jcc(Cc::Ae, brk);
                let es = self.size(&et);
                if es != 1 {
                    self.asm.imul_rri(EAX, EAX, es);
                }
                self.asm.load(4, false, ECX, EBP, p_slot);
                self.asm.alu_rr(Alu::Add, EAX, ECX);
                if !by_ref {
                    self.load_eax(&et);
                }
                self.store_to(&vt, EBP, var_off);
                self.loops.push(LoopCtx { brk, cont, broke: false });
                self.unit_block(body)?;
                self.loops.pop();
                self.asm.bind(cont);
                self.asm.load(4, false, EAX, EBP, i_slot);
                self.asm.alu_ri(Alu::Add, EAX, 1);
                self.asm.store(4, EAX, EBP, i_slot);
                self.asm.jmp(top);
            }
        }
        self.asm.bind(brk);
        self.scopes.pop();
        Ok(Ty::Unit)
    }

    fn match_expr(&mut self, s: &Expr, arms: &[Arm], want: Option<&Ty>, span: Span) -> R<Ty> {
        let st = self.expr(s, None)?;
        let is_str = st == Ty::Str;
        match self.class(&st) {
            Class::Scalar { .. } | Class::Fat if is_str || !matches!(st, Ty::Slice(_)) => {}
            _ => return err(s.span, format!("cannot match on a value of type `{}`", self.tn(&st))),
        }
        let slot = self.alloc_slot(&st);
        self.store_to(&st, EBP, slot);
        let end = self.asm.label();
        let mut result: Option<Ty> = want.cloned();
        let mut exhaustive = false;
        let signed = st.signed();
        for arm in arms {
            let next = self.asm.label();
            let body_l = self.asm.label();
            self.scopes.push(Vec::new());
            let mut binds = false;
            for p in &arm.pats {
                match p {
                    Pat::Wild => {
                        self.asm.jmp(body_l);
                        if arm.guard.is_none() {
                            exhaustive = true;
                        }
                    }
                    Pat::Bind(name) => {
                        if arm.pats.len() > 1 {
                            return err(arm.body.span, "bindings cannot be combined with `|`");
                        }
                        binds = true;
                        self.load_slot(&st, slot);
                        let off = self.declare_local(name, st.clone(), false);
                        self.store_to(&st, EBP, off);
                        self.asm.jmp(body_l);
                        if arm.guard.is_none() {
                            exhaustive = true;
                        }
                    }
                    Pat::Value(v) => {
                        if is_str {
                            self.load_slot(&st, slot);
                            self.asm.push(EDX);
                            self.asm.push(EAX);
                            self.expr_as(v, &Ty::Str)?;
                            self.asm.push(EDX);
                            self.asm.push(EAX);
                            let f = self.prelude_fn("__str_eq");
                            self.asm.call(f.label);
                            self.asm.alu_ri(Alu::Add, ESP, 16);
                            self.asm.test_rr(EAX, EAX);
                            self.asm.jcc(Cc::Ne, body_l);
                        } else {
                            let k = self.pat_const(v, &st)?;
                            self.load_slot(&st, slot);
                            self.asm.alu_ri(Alu::Cmp, EAX, k as u32);
                            self.asm.jcc(Cc::E, body_l);
                        }
                    }
                    Pat::Range(lo, hi) => {
                        if is_str {
                            return err(lo.span, "range patterns need integers or chars");
                        }
                        let (a, b) = (self.pat_const(lo, &st)?, self.pat_const(hi, &st)?);
                        let skip = self.asm.label();
                        self.load_slot(&st, slot);
                        self.asm.alu_ri(Alu::Cmp, EAX, a as u32);
                        self.asm.jcc(if signed { Cc::L } else { Cc::B }, skip);
                        self.asm.alu_ri(Alu::Cmp, EAX, b as u32);
                        self.asm.jcc(if signed { Cc::Le } else { Cc::Be }, body_l);
                        self.asm.bind(skip);
                    }
                }
            }
            let _ = binds;
            self.asm.jmp(next);
            self.asm.bind(body_l);
            if let Some(g) = &arm.guard {
                self.expr_as(g, &Ty::Bool)?;
                self.asm.test_rr(EAX, EAX);
                self.asm.jcc(Cc::E, next);
            }
            let t = self.expr(&arm.body, result.as_ref())?;
            match &result {
                Some(r) => {
                    let r = r.clone();
                    self.coerce(&t, &r, arm.body.span)?;
                }
                None => {
                    if t != Ty::Never {
                        result = Some(t);
                    }
                }
            }
            self.asm.jmp(end);
            self.scopes.pop();
            self.asm.bind(next);
        }
        if !exhaustive {
            self.panic_here("internal error: entered unreachable code: no match arm matched", span);
        }
        self.asm.bind(end);
        Ok(result.unwrap_or(Ty::Never))
    }

    fn load_slot(&mut self, t: &Ty, slot: i32) {
        match self.class(t) {
            Class::Fat => {
                self.asm.load(4, false, EAX, EBP, slot);
                self.asm.load(4, false, EDX, EBP, slot + 4);
            }
            Class::Scalar { size, signed } => self.asm.load(size, signed, EAX, EBP, slot),
            _ => self.asm.lea(EAX, EBP, slot),
        }
    }

    /// A constant pattern value, checked against the scrutinee type.
    fn pat_const(&mut self, v: &Expr, st: &Ty) -> R<i64> {
        if let ExprKind::Path(p) = &v.kind {
            if p.len() == 2 {
                if let Some(&id) = self.enum_ids.get(&p[0]) {
                    if *st != Ty::Enum(id) {
                        return self.mismatch(v.span, st, &Ty::Enum(id));
                    }
                }
            }
        }
        let k = self.const_eval(v)?;
        match st {
            Ty::Int(i) if !i.fits(k) => err(v.span, format!("pattern out of range for `{}`", i.name())),
            Ty::Char if matches!(v.kind, ExprKind::Int(..)) => err(v.span, "expected a char pattern like 'a'"),
            _ => Ok(k),
        }
    }

    fn struct_lit(&mut self, name: &str, fields: &[(String, Expr)], span: Span) -> R<Ty> {
        let t = self.resolve_ty(&TypeExpr::Name(String::from(name), span))?;
        let Ty::Struct(id) = t else {
            return err(span, format!("`{name}` is not a struct"));
        };
        for (f, e) in fields {
            if !self.structs[id].fields.iter().any(|(n, _, _)| n == f) {
                return err(e.span, format!("struct `{name}` has no field named `{f}`"));
            }
        }
        let slot = self.alloc_slot(&t);
        let decl_fields = self.structs[id].fields.clone();
        for (n, ft, off) in decl_fields {
            let e = match fields.iter().find(|(f, _)| *f == n) {
                Some((_, e)) => e,
                None => return err(span, format!("missing field `{n}` in initializer of `{name}`")),
            };
            self.expr_as(e, &ft)?;
            self.store_to(&ft, EBP, slot + off as i32);
        }
        self.asm.lea(EAX, EBP, slot);
        Ok(t)
    }

    fn elem_hint(want: Option<&Ty>) -> Option<Ty> {
        match want {
            Some(Ty::Array(e, _)) | Some(Ty::Slice(e)) => Some((**e).clone()),
            _ => None,
        }
    }

    fn array_lit(&mut self, xs: &[Expr], want: Option<&Ty>, span: Span) -> R<Ty> {
        let mut et = Self::elem_hint(want);
        if xs.is_empty() {
            return match et {
                Some(e) => {
                    let t = Ty::Array(Box::new(e), 0);
                    let slot = self.alloc_slot(&t);
                    self.asm.lea(EAX, EBP, slot);
                    Ok(t)
                }
                None => err(span, "type annotations needed for an empty array"),
            };
        }
        if et.is_none() {
            let hint = if is_untyped_lit(&xs[0]) { xs.iter().find_map(|x| if is_untyped_lit(x) { None } else { self.probe(x) }) } else { None };
            et = Some(match hint {
                Some(h) => h,
                None => match self.probe(&xs[0]) {
                    Some(t) => t,
                    None => {
                        self.expr(&xs[0], None)?;
                        unreachable!()
                    }
                },
            });
        }
        let et = et.unwrap();
        let t = Ty::Array(Box::new(et.clone()), xs.len() as u32);
        let es = self.size(&et) as i32;
        let slot = self.alloc_slot(&t);
        for (i, x) in xs.iter().enumerate() {
            self.expr_as(x, &et)?;
            self.store_to(&et, EBP, slot + i as i32 * es);
        }
        self.asm.lea(EAX, EBP, slot);
        Ok(t)
    }

    fn array_repeat(&mut self, x: &Expr, n: &Expr, want: Option<&Ty>, span: Span) -> R<Ty> {
        let n = self.const_eval(n)?;
        if !(0..=0x100_0000).contains(&n) {
            return err(span, "array length out of range");
        }
        let n = n as u32;
        let et = match Self::elem_hint(want) {
            Some(e) => {
                self.expr_as(x, &e)?;
                e
            }
            None => self.expr(x, None)?,
        };
        let t = Ty::Array(Box::new(et.clone()), n);
        let slot = self.alloc_slot(&t);
        let es = self.size(&et);
        if n > 0 {
            match self.class(&et) {
                Class::Scalar { size: 1, .. } => {
                    self.asm.lea(EDI, EBP, slot);
                    self.asm.mov_ri(ECX, n);
                    self.asm.rep_stosb();
                }
                cls => {
                    self.asm.mov_rr(EBX, EAX);
                    self.asm.lea(EDI, EBP, slot);
                    self.asm.mov_ri(ECX, n);
                    let top = self.asm.label();
                    self.asm.bind(top);
                    match cls {
                        Class::Scalar { size, .. } => self.asm.store(size, EBX, EDI, 0),
                        Class::Fat => {
                            self.asm.store(4, EBX, EDI, 0);
                            self.asm.store(4, EDX, EDI, 4);
                        }
                        Class::Agg { size } => {
                            self.asm.push(ECX);
                            self.asm.push(EDI);
                            self.asm.mov_rr(ESI, EBX);
                            self.asm.mov_ri(ECX, size);
                            self.asm.rep_movsb();
                            self.asm.pop(EDI);
                            self.asm.pop(ECX);
                        }
                        Class::Void => {}
                    }
                    self.asm.alu_ri(Alu::Add, EDI, es);
                    self.asm.alu_ri(Alu::Sub, ECX, 1);
                    self.asm.jcc(Cc::Ne, top);
                }
            }
        }
        self.asm.lea(EAX, EBP, slot);
        Ok(t)
    }

    // ------------------------------------------------------------ calls

    /// Push arguments (right to left) and call; returns the result type.
    fn emit_call(&mut self, sig: &FnSig, args: &[Expr], recv: Option<&Expr>, name: &str, span: Span) -> R<Ty> {
        let skip = recv.is_some() as usize;
        let n = sig.params.len() - skip.min(sig.params.len());
        if args.len() != n {
            return err(span, format!("`{name}` takes {n} argument(s) but {} were supplied", args.len()));
        }
        let mut bytes = 0;
        for (i, a) in args.iter().enumerate().rev() {
            let pt = sig.params[i + skip].clone();
            self.expr_as(a, &pt)?;
            bytes += self.push_value(&pt);
        }
        if let Some(r) = recv {
            let pt = sig.params[0].clone();
            self.receiver(r, &pt)?;
            bytes += self.push_value(&pt);
        }
        if let Class::Agg { .. } = self.class(&sig.ret) {
            let slot = self.alloc_slot(&sig.ret);
            self.asm.lea(EAX, EBP, slot);
            self.asm.push(EAX);
            bytes += 4;
        }
        self.asm.call(sig.label);
        if bytes > 0 {
            self.asm.alu_ri(Alu::Add, ESP, bytes);
        }
        Ok(sig.ret.clone())
    }

    /// Compile a method receiver to the parameter type: `&Self` or `Self`.
    fn receiver(&mut self, r: &Expr, pt: &Ty) -> R<()> {
        let want_ptr = matches!(pt, Ty::Ptr(_));
        let target = match pt {
            Ty::Ptr(t) => (**t).clone(),
            t => t.clone(),
        };
        let (mut t, addr) = if self.addressable(r) {
            let (t, _) = self.place(r)?;
            (t, true)
        } else {
            let t = self.expr(r, None)?;
            (t, false)
        };
        // Bring EAX to the address of a `target` value where possible.
        let mut is_addr = addr || matches!(self.class(&t), Class::Agg { .. });
        loop {
            if t == target {
                break;
            }
            match t {
                Ty::Ptr(inner) => {
                    if is_addr {
                        self.asm.load(4, false, EAX, EAX, 0);
                    }
                    is_addr = true;
                    t = *inner;
                }
                _ => return self.mismatch(r.span, &target, &t),
            }
        }
        if want_ptr {
            if !is_addr {
                let slot = self.alloc_slot(&t);
                self.store_to(&t, EBP, slot);
                self.asm.lea(EAX, EBP, slot);
            }
        } else if is_addr {
            self.load_eax(&t);
        }
        Ok(())
    }

    fn call(&mut self, path: &[String], args: &[Expr], want: Option<&Ty>, span: Span) -> R<Ty> {
        let _ = want;
        if path.len() == 1 {
            if let Some(t) = self.intrinsic(&path[0], args, span)? {
                return Ok(t);
            }
        }
        let mut key = path.join("::");
        if path.len() >= 2 && path[0] == "Self" {
            match &self.self_ty {
                Some(t) => {
                    let owner = types::method_owner(&t.clone(), self).unwrap_or_default();
                    key = format!("{owner}::{}", path[1..].join("::"));
                }
                None => return err(span, "`Self` is only available in impl blocks"),
            }
        }
        let sig = match self.lookup_fn(&key) {
            Some(s) => s,
            None => {
                // `std::process::exit` and friends: try the last segment, then Type::name.
                let last = &path[path.len() - 1];
                let alt = if path.len() > 2 { Some(format!("{}::{last}", path[path.len() - 2])) } else { None };
                match alt.and_then(|a| self.lookup_fn(&a)).or_else(|| self.lookup_fn(last)) {
                    Some(s) if path.len() > 1 && (path[0] == "std" || path[0] == "core") => s,
                    Some(s) if path.len() > 2 => s,
                    _ => {
                        if path.len() == 2 && self.struct_ids.contains_key(&path[0]) {
                            return err(span, format!("no function `{}` in `impl {}`", path[1], path[0]));
                        }
                        return err(span, format!("cannot find function `{key}` in this scope"));
                    }
                }
            }
        };
        if sig.self_kind.is_some() {
            // Type::method(receiver, args...)
            if args.is_empty() {
                return err(span, format!("`{key}` needs a receiver argument"));
            }
            return self.emit_call(&sig, &args[1..], Some(&args[0]), &key, span);
        }
        self.emit_call(&sig, args, None, &key, span)
    }

    fn method_call(&mut self, recv: &Expr, name: &str, args: &[Expr], span: Span) -> R<Ty> {
        let Some(mut rt) = self.probe(recv) else {
            self.expr(recv, None)?;
            unreachable!()
        };
        // Pointee types followed by auto-deref, loaded after compiling `recv`.
        let mut chain: Vec<Ty> = Vec::new();
        // Built-in methods on slices, strings, arrays, pointers and integers.
        loop {
            match (&rt, name, args.len()) {
                (Ty::Str, "len", 0) | (Ty::Slice(_), "len", 0) => {
                    self.recv_val(recv, &chain)?;
                    self.asm.mov_rr(EAX, EDX);
                    return Ok(Ty::usize());
                }
                (Ty::Str, "is_empty", 0) | (Ty::Slice(_), "is_empty", 0) => {
                    self.recv_val(recv, &chain)?;
                    self.asm.test_rr(EDX, EDX);
                    self.asm.setcc_eax(Cc::E);
                    return Ok(Ty::Bool);
                }
                (Ty::Str, "as_bytes", 0) => {
                    self.recv_val(recv, &chain)?;
                    return Ok(Ty::Slice(Box::new(Ty::Int(IntTy::U8))));
                }
                (Ty::Str, "as_ptr", 0) => {
                    self.recv_val(recv, &chain)?;
                    return Ok(Ty::ptr(Ty::Int(IntTy::U8)));
                }
                (Ty::Slice(e), "as_ptr" | "as_mut_ptr", 0) => {
                    let e = (**e).clone();
                    self.recv_val(recv, &chain)?;
                    return Ok(Ty::ptr(e));
                }
                (Ty::Array(_, n), "len", 0) => {
                    self.asm.mov_ri(EAX, *n);
                    return Ok(Ty::usize());
                }
                (Ty::Array(_, n), "is_empty", 0) => {
                    self.asm.mov_ri(EAX, (*n == 0) as u32);
                    return Ok(Ty::Bool);
                }
                (Ty::Array(e, _), "as_ptr" | "as_mut_ptr", 0) => {
                    let e = (**e).clone();
                    self.seq_base(recv, span)?;
                    return Ok(Ty::ptr(e));
                }
                (Ty::Ptr(inner), "add" | "offset" | "sub", 1) => {
                    let inner = (**inner).clone();
                    let pt = rt.clone();
                    self.recv_val(recv, &chain)?;
                    self.asm.push(EAX);
                    let it = self.expr(&args[0], Some(&Ty::usize()))?;
                    if !it.is_int() {
                        return err(args[0].span, "pointer offsets must be integers");
                    }
                    let es = self.size(&inner).max(1);
                    if es != 1 {
                        self.asm.imul_rri(EAX, EAX, es);
                    }
                    self.asm.mov_rr(ECX, EAX);
                    self.asm.pop(EAX);
                    self.asm.alu_rr(if name == "sub" { Alu::Sub } else { Alu::Add }, EAX, ECX);
                    return Ok(pt);
                }
                (Ty::Ptr(_), "is_null", 0) => {
                    self.recv_val(recv, &chain)?;
                    self.asm.test_rr(EAX, EAX);
                    self.asm.setcc_eax(Cc::E);
                    return Ok(Ty::Bool);
                }
                (Ty::Int(i), "abs", 0) if i.signed => {
                    let t = rt.clone();
                    self.recv_val(recv, &chain)?;
                    let done = self.asm.label();
                    self.asm.test_rr(EAX, EAX);
                    self.asm.jcc(Cc::Ge, done);
                    self.asm.neg(EAX);
                    self.normalize(&t);
                    self.asm.bind(done);
                    return Ok(t);
                }
                (Ty::Int(i), "min" | "max", 1) => {
                    let (t, signed) = (rt.clone(), i.signed);
                    self.recv_val(recv, &chain)?;
                    self.asm.push(EAX);
                    self.expr_as(&args[0], &t)?;
                    self.asm.mov_rr(ECX, EAX);
                    self.asm.pop(EAX);
                    self.asm.alu_rr(Alu::Cmp, EAX, ECX);
                    let keep = self.asm.label();
                    let cc = match (name, signed) {
                        ("min", true) => Cc::Le,
                        ("min", false) => Cc::Be,
                        (_, true) => Cc::Ge,
                        _ => Cc::Ae,
                    };
                    self.asm.jcc(cc, keep);
                    self.asm.mov_rr(EAX, ECX);
                    self.asm.bind(keep);
                    return Ok(t);
                }
                (Ty::Int(_), "wrapping_add" | "wrapping_sub" | "wrapping_mul", 1) => {
                    let t = rt.clone();
                    let op = match name {
                        "wrapping_add" => BinOp::Add,
                        "wrapping_sub" => BinOp::Sub,
                        _ => BinOp::Mul,
                    };
                    self.recv_val(recv, &chain)?;
                    self.asm.push(EAX);
                    self.expr_as(&args[0], &t)?;
                    self.asm.mov_rr(ECX, EAX);
                    self.asm.pop(EAX);
                    self.arith(op, &t, span);
                    return Ok(t);
                }
                (Ty::Ptr(inner), _, _) => {
                    // Auto-deref.
                    rt = (**inner).clone();
                    chain.push(rt.clone());
                    continue;
                }
                _ => break,
            }
        }
        let owner = match types::method_owner(&rt, self) {
            Some(o) => o,
            None => return err(span, format!("no method named `{name}` found for `{}`", self.tn(&rt))),
        };
        let key = format!("{owner}::{name}");
        let sig = match self.lookup_fn(&key) {
            Some(s) if s.self_kind.is_some() => s,
            Some(_) => return err(span, format!("`{key}` is an associated function, not a method; call it as `{key}(...)`")),
            None => return err(span, format!("no method named `{name}` found for `{owner}`")),
        };
        self.emit_call(&sig, args, Some(recv), &key, span)
    }

    /// Compile a method receiver and follow `chain` auto-derefs.
    fn recv_val(&mut self, recv: &Expr, chain: &[Ty]) -> R<()> {
        self.expr(recv, None)?;
        for t in chain {
            self.load_eax(t);
        }
        Ok(())
    }

    /// Built-in functions implemented in machine code. Returns None if
    /// `name` is not an intrinsic.
    fn intrinsic(&mut self, name: &str, args: &[Expr], span: Span) -> R<Option<Ty>> {
        let u32t = Ty::Int(IntTy::U32);
        let u8t = Ty::Int(IntTy::U8);
        let arity = |n: usize| -> R<()> {
            if args.len() != n {
                return err(span, format!("`{name}` takes {n} argument(s) but {} were supplied", args.len()));
            }
            Ok(())
        };
        let t = match name {
            "__systab" => {
                arity(0)?;
                self.asm.mov_r_abs(EAX, Abs::Bss(self.systab));
                self.asm.load(4, false, EAX, EAX, 0);
                u32t
            }
            "__int" => {
                arity(2)?;
                let regs = self.struct_ids.get("Regs").copied().map(Ty::Struct).unwrap();
                self.expr_as(&args[1], &Ty::ptr(regs))?;
                self.asm.push(EAX);
                self.expr_as(&args[0], &u32t)?;
                self.asm.push(EAX);
                self.asm.mov_r_abs(EAX, Abs::Bss(self.systab));
                self.asm.load(4, false, EAX, EAX, 0);
                self.asm.call_mem(EAX);
                self.asm.alu_ri(Alu::Add, ESP, 8);
                self.asm.cld();
                Ty::Unit
            }
            "__outb" | "outb" => {
                arity(2)?;
                self.expr_as(&args[1], &u8t)?;
                self.asm.push(EAX);
                self.expr_as(&args[0], &Ty::Int(IntTy { signed: false, size: 2, word: false }))?;
                self.asm.mov_rr(EDX, EAX);
                self.asm.pop(EAX);
                self.asm.out_dx_al();
                Ty::Unit
            }
            "__inb" | "inb" => {
                arity(1)?;
                self.expr_as(&args[0], &Ty::Int(IntTy { signed: false, size: 2, word: false }))?;
                self.asm.mov_rr(EDX, EAX);
                self.asm.mov_ri(EAX, 0);
                self.asm.in_al_dx();
                u8t
            }
            "__memcpy" => {
                arity(3)?;
                let p = Ty::ptr(u8t.clone());
                self.expr_as(&args[2], &Ty::usize())?;
                self.asm.push(EAX);
                self.expr_as(&args[1], &p)?;
                self.asm.push(EAX);
                self.expr_as(&args[0], &p)?;
                self.asm.mov_rr(EDI, EAX);
                self.asm.pop(ESI);
                self.asm.pop(ECX);
                self.asm.rep_movsb();
                Ty::Unit
            }
            "__memset" => {
                arity(3)?;
                let p = Ty::ptr(u8t.clone());
                self.expr_as(&args[2], &Ty::usize())?;
                self.asm.push(EAX);
                self.expr_as(&args[1], &u8t)?;
                self.asm.push(EAX);
                self.expr_as(&args[0], &p)?;
                self.asm.mov_rr(EDI, EAX);
                self.asm.pop(EAX);
                self.asm.pop(ECX);
                self.asm.rep_stosb();
                Ty::Unit
            }
            "__str" | "str_from_bytes" => {
                arity(1)?;
                self.expr_as(&args[0], &Ty::Slice(Box::new(u8t)))?;
                Ty::Str
            }
            "__slice" | "slice_from_raw" => {
                arity(2)?;
                self.expr_as(&args[1], &Ty::usize())?;
                self.asm.push(EAX);
                self.expr_as(&args[0], &Ty::ptr(u8t.clone()))?;
                self.asm.pop(EDX);
                Ty::Slice(Box::new(u8t))
            }
            "__size_of_regs" => {
                arity(0)?;
                let regs = self.struct_ids.get("Regs").copied().map(Ty::Struct).unwrap();
                let n = self.size(&regs);
                self.asm.mov_ri(EAX, n);
                Ty::usize()
            }
            _ => return Ok(None),
        };
        Ok(Some(t))
    }

    // ------------------------------------------------------------ macros

    fn macro_call(&mut self, name: &str, args: &[Expr], span: Span) -> R<Ty> {
        match name {
            "print" | "println" | "eprint" | "eprintln" => {
                if args.is_empty() {
                    if name.ends_with("ln") {
                        self.print_lit(b"\n");
                        return Ok(Ty::Unit);
                    }
                    return err(span, "requires at least a format string argument");
                }
                self.format(args, span)?;
                if name.ends_with("ln") {
                    self.print_lit(b"\n");
                }
                Ok(Ty::Unit)
            }
            "panic" | "unreachable" | "todo" | "unimplemented" => {
                self.panic_header(span);
                if args.is_empty() {
                    let m: &[u8] = match name {
                        "panic" => b"explicit panic",
                        "unreachable" => b"internal error: entered unreachable code",
                        "todo" => b"not yet implemented",
                        _ => b"not implemented",
                    };
                    self.print_lit(m);
                } else {
                    if name == "unreachable" {
                        self.print_lit(b"internal error: entered unreachable code: ");
                    }
                    self.format(args, span)?;
                }
                self.print_lit(b"\n");
                self.exit_101();
                Ok(Ty::Never)
            }
            "assert" => {
                if args.is_empty() {
                    return err(span, "`assert!` needs a condition");
                }
                self.expr_as(&args[0], &Ty::Bool)?;
                let ok = self.asm.label();
                self.asm.test_rr(EAX, EAX);
                self.asm.jcc(Cc::Ne, ok);
                self.panic_header(span);
                if args.len() > 1 {
                    self.format(&args[1..], span)?;
                } else {
                    self.print_lit(b"assertion failed");
                }
                self.print_lit(b"\n");
                self.exit_101();
                self.asm.bind(ok);
                Ok(Ty::Unit)
            }
            "assert_eq" | "assert_ne" => {
                if args.len() < 2 {
                    return err(span, format!("`{name}!` needs two values"));
                }
                let hint = self.operand_ty(&args[0], &args[1], None);
                let t = self.expr(&args[0], hint.as_ref())?;
                let a = self.alloc_slot(&t);
                self.store_to(&t, EBP, a);
                self.expr_as(&args[1], &t)?;
                let b = self.alloc_slot(&t);
                self.store_to(&t, EBP, b);
                // Compare.
                match self.class(&t) {
                    Class::Scalar { .. } => {
                        self.load_slot(&t, b);
                        self.asm.mov_rr(ECX, EAX);
                        self.load_slot(&t, a);
                        self.asm.alu_rr(Alu::Cmp, EAX, ECX);
                        self.asm.setcc_eax(Cc::E);
                    }
                    Class::Fat if t == Ty::Str => {
                        self.load_slot(&t, a);
                        self.asm.push(EDX);
                        self.asm.push(EAX);
                        self.load_slot(&t, b);
                        self.asm.push(EDX);
                        self.asm.push(EAX);
                        let f = self.prelude_fn("__str_eq");
                        self.asm.call(f.label);
                        self.asm.alu_ri(Alu::Add, ESP, 16);
                    }
                    _ => return err(span, format!("`{name}!` is not supported for `{}`", self.tn(&t))),
                }
                let ok = self.asm.label();
                self.asm.test_rr(EAX, EAX);
                self.asm.jcc(if name == "assert_eq" { Cc::Ne } else { Cc::E }, ok);
                self.panic_header(span);
                let op: &[u8] = if name == "assert_eq" { b"==" } else { b"!=" };
                let mut m = b"assertion `left ".to_vec();
                m.extend_from_slice(op);
                m.extend_from_slice(b" right` failed");
                self.print_lit(&m);
                if args.len() > 2 {
                    self.print_lit(b": ");
                    self.format(&args[2..], span)?;
                }
                self.print_lit(b"\n  left: ");
                self.load_slot(&t, a);
                self.print_value(&t, true, 10, false, 0, 0, span)?;
                self.print_lit(b"\n right: ");
                self.load_slot(&t, b);
                self.print_value(&t, true, 10, false, 0, 0, span)?;
                self.print_lit(b"\n");
                self.exit_101();
                self.asm.bind(ok);
                Ok(Ty::Unit)
            }
            "format" | "vec" | "write" | "writeln" => err(span, format!("`{name}!` is not supported in R++")),
            _ => err(span, format!("cannot find macro `{name}`")),
        }
    }

    fn panic_header(&mut self, span: Span) {
        self.print_lit(b"thread 'main' panicked at ");
        self.asm.mov_ri(EAX, span.line);
        self.print_value(&Ty::Int(IntTy::U32), false, 10, false, 0, 0, span).ok();
        self.print_lit(b":");
        self.asm.mov_ri(EAX, span.col);
        self.print_value(&Ty::Int(IntTy::U32), false, 10, false, 0, 0, span).ok();
        self.print_lit(b":\n");
    }

    fn exit_101(&mut self) {
        let f = self.prelude_fn("exit");
        self.asm.push_i(101);
        self.asm.call(f.label);
    }

    fn print_lit(&mut self, s: &[u8]) {
        if s.is_empty() {
            return;
        }
        let p = self.string(s);
        self.asm.push_i(s.len() as u32);
        self.asm.mov_r_abs(EAX, Abs::Data(p));
        self.asm.push(EAX);
        let f = self.prelude_fn("__print_str");
        self.asm.call(f.label);
        self.asm.alu_ri(Alu::Add, ESP, 8);
    }

    fn parse_format(&self, args: &[Expr], span: Span) -> R<Vec<Piece>> {
        let fmt = match &args[0].kind {
            ExprKind::Str(s) => s.clone(),
            _ => return err(args[0].span, "format argument must be a string literal"),
        };
        let mut pieces = Vec::new();
        let mut lit = Vec::new();
        let mut next_arg = 1;
        let mut used = vec![false; args.len()];
        let mut i = 0;
        while i < fmt.len() {
            let c = fmt[i];
            if c == b'{' && fmt.get(i + 1) == Some(&b'{') {
                lit.push(b'{');
                i += 2;
                continue;
            }
            if c == b'}' && fmt.get(i + 1) == Some(&b'}') {
                lit.push(b'}');
                i += 2;
                continue;
            }
            if c == b'}' {
                return err(args[0].span, "invalid format string: unmatched `}` found (use `}}`)");
            }
            if c != b'{' {
                lit.push(c);
                i += 1;
                continue;
            }
            let close = match fmt[i..].iter().position(|&x| x == b'}') {
                Some(p) => i + p,
                None => return err(args[0].span, "invalid format string: expected `}`"),
            };
            let inner = core::str::from_utf8(&fmt[i + 1..close]).unwrap_or("");
            i = close + 1;
            if !lit.is_empty() {
                pieces.push(Piece::Lit(core::mem::take(&mut lit)));
            }
            let (name, spec) = match inner.find(':') {
                Some(p) => (&inner[..p], &inner[p + 1..]),
                None => (inner, ""),
            };
            let expr = if name.is_empty() {
                if next_arg >= args.len() {
                    return err(span, format!("{} positional arguments in format string, but there is {} argument", next_arg, args.len() - 1));
                }
                used[next_arg] = true;
                next_arg += 1;
                args[next_arg - 1].clone()
            } else if let Ok(n) = name.parse::<usize>() {
                if n + 1 >= args.len() {
                    return err(args[0].span, format!("invalid reference to positional argument {n}"));
                }
                used[n + 1] = true;
                args[n + 1].clone()
            } else {
                Expr { kind: ExprKind::Path(vec![String::from(name)]), span: args[0].span }
            };
            // Spec: [<>^][0][width][?|x|X|b|o|p]
            let sb = spec.as_bytes();
            let mut k = 0;
            let mut align = 0;
            if let Some(&a) = sb.first() {
                if matches!(a, b'<' | b'>' | b'^') {
                    align = match a {
                        b'<' => 1,
                        b'>' => 2,
                        _ => 3,
                    };
                    k = 1;
                }
            }
            let mut zero = 0;
            if sb.get(k) == Some(&b'0') {
                zero = 1;
                k += 1;
            }
            let mut width = 0u32;
            while k < sb.len() && sb[k].is_ascii_digit() {
                width = width * 10 + (sb[k] - b'0') as u32;
                k += 1;
            }
            let (mut debug, mut radix, mut upper, mut ptr) = (false, 10, false, false);
            match &spec[k..] {
                "" => {}
                "?" => debug = true,
                "x" => radix = 16,
                "X" => {
                    radix = 16;
                    upper = true;
                }
                "b" => radix = 2,
                "o" => radix = 8,
                "p" => ptr = true,
                "#x" | "#X" | "#?" => {
                    return err(args[0].span, "the `#` format flag is not supported");
                }
                other => return err(args[0].span, format!("unknown format spec `{other}`")),
            }
            pieces.push(Piece::Arg { expr, debug, radix, upper, width, flags: zero | align << 1, ptr });
        }
        if !lit.is_empty() {
            pieces.push(Piece::Lit(lit));
        }
        if let Some(p) = (1..args.len()).find(|&p| !used[p]) {
            return err(args[p].span, "argument never used in the format string");
        }
        Ok(pieces)
    }

    fn format(&mut self, args: &[Expr], span: Span) -> R<()> {
        for p in self.parse_format(args, span)? {
            match p {
                Piece::Lit(s) => self.print_lit(&s),
                Piece::Arg { expr, debug, radix, upper, width, flags, ptr } => {
                    let t = self.expr(&expr, None)?;
                    if ptr {
                        if !matches!(t, Ty::Ptr(_) | Ty::Str | Ty::Slice(_)) {
                            return err(expr.span, "`{:p}` needs a reference or pointer");
                        }
                        self.print_lit(b"0x");
                        self.print_value(&Ty::Int(IntTy::U32), false, 16, false, 8, 1, expr.span)?;
                        continue;
                    }
                    self.print_value(&t, debug, radix, upper, width, flags, expr.span)?;
                }
            }
        }
        Ok(())
    }

    /// Print the value in registers.
    #[allow(clippy::too_many_arguments)]
    fn print_value(&mut self, t: &Ty, debug: bool, radix: u32, upper: bool, width: u32, flags: u32, span: Span) -> R<()> {
        match t {
            Ty::Int(i) => {
                self.asm.push_i(flags);
                self.asm.push_i(width);
                self.asm.push_i(upper as u32);
                self.asm.push_i(radix);
                self.asm.push_i(i.signed as u32);
                self.asm.push(EAX);
                let f = self.prelude_fn("__print_int");
                self.asm.call(f.label);
                self.asm.alu_ri(Alu::Add, ESP, 24);
            }
            Ty::Char => {
                if debug {
                    self.asm.push(EAX);
                    self.print_lit(b"'");
                    self.asm.pop(EAX);
                }
                self.asm.push(EAX);
                let f = self.prelude_fn("__print_char");
                self.asm.call(f.label);
                self.asm.alu_ri(Alu::Add, ESP, 4);
                if debug {
                    self.print_lit(b"'");
                }
            }
            Ty::Bool => {
                self.asm.push_i(flags);
                self.asm.push_i(width);
                self.asm.push(EAX);
                let f = self.prelude_fn("__print_bool");
                self.asm.call(f.label);
                self.asm.alu_ri(Alu::Add, ESP, 12);
            }
            Ty::Str => {
                if debug {
                    self.asm.push(EDX);
                    self.asm.push(EAX);
                    self.print_lit(b"\"");
                    self.asm.pop(EAX);
                    self.asm.pop(EDX);
                }
                self.asm.push_i(flags);
                self.asm.push_i(width);
                self.asm.push(EDX);
                self.asm.push(EAX);
                let f = self.prelude_fn("__print_str_w");
                self.asm.call(f.label);
                self.asm.alu_ri(Alu::Add, ESP, 16);
                if debug {
                    self.print_lit(b"\"");
                }
            }
            Ty::Unit => self.print_lit(b"()"),
            Ty::Ptr(inner) if !matches!(**inner, Ty::Unit) => {
                let inner = (**inner).clone();
                self.load_eax(&inner);
                self.print_value(&inner, debug, radix, upper, width, flags, span)?;
            }
            Ty::Enum(id) if debug => {
                let variants = self.enums[*id].variants.clone();
                let end = self.asm.label();
                for (n, v) in variants {
                    let next = self.asm.label();
                    self.asm.alu_ri(Alu::Cmp, EAX, v as u32);
                    self.asm.jcc(Cc::Ne, next);
                    self.print_lit(n.as_bytes());
                    self.asm.jmp(end);
                    self.asm.bind(next);
                }
                self.asm.bind(end);
            }
            Ty::Struct(id) if debug => {
                let (name, fields) = (self.structs[*id].name.clone(), self.structs[*id].fields.clone());
                let slot = self.alloc_slot(&Ty::usize());
                self.asm.store(4, EAX, EBP, slot);
                self.print_lit(format!("{name} {{ ").as_bytes());
                for (i, (n, ft, off)) in fields.iter().enumerate() {
                    let sep = if i > 0 { ", " } else { "" };
                    self.print_lit(format!("{sep}{n}: ").as_bytes());
                    self.asm.load(4, false, EAX, EBP, slot);
                    if *off != 0 {
                        self.asm.alu_ri(Alu::Add, EAX, *off);
                    }
                    self.load_eax(ft);
                    self.print_value(ft, true, 10, false, 0, 0, span)?;
                }
                self.print_lit(b" }");
            }
            Ty::Array(e, n) if debug => {
                let e = (**e).clone();
                self.asm.mov_ri(EDX, *n);
                self.print_seq(&e, span)?;
            }
            Ty::Slice(e) if debug => {
                let e = (**e).clone();
                self.print_seq(&e, span)?;
            }
            Ty::Enum(_) | Ty::Struct(_) | Ty::Array(..) | Ty::Slice(_) => {
                return err(span, format!("`{}` doesn't implement `Display`; use `{{:?}}`", self.tn(t)));
            }
            _ => return err(span, format!("cannot format a value of type `{}`", self.tn(t))),
        }
        Ok(())
    }

    /// Print `[a, b, c]` for EAX = pointer, EDX = length.
    fn print_seq(&mut self, et: &Ty, span: Span) -> R<()> {
        let p = self.alloc_slot(&Ty::usize());
        let n = self.alloc_slot(&Ty::usize());
        let i = self.alloc_slot(&Ty::usize());
        self.asm.store(4, EAX, EBP, p);
        self.asm.store(4, EDX, EBP, n);
        self.asm.mov_ri(EAX, 0);
        self.asm.store(4, EAX, EBP, i);
        self.print_lit(b"[");
        let (top, done, first) = (self.asm.label(), self.asm.label(), self.asm.label());
        self.asm.bind(top);
        self.asm.load(4, false, EAX, EBP, i);
        self.asm.load(4, false, ECX, EBP, n);
        self.asm.alu_rr(Alu::Cmp, EAX, ECX);
        self.asm.jcc(Cc::Ae, done);
        self.asm.test_rr(EAX, EAX);
        self.asm.jcc(Cc::E, first);
        self.print_lit(b", ");
        self.asm.bind(first);
        self.asm.load(4, false, EAX, EBP, i);
        let es = self.size(et);
        if es != 1 {
            self.asm.imul_rri(EAX, EAX, es);
        }
        self.asm.load(4, false, ECX, EBP, p);
        self.asm.alu_rr(Alu::Add, EAX, ECX);
        self.load_eax(et);
        self.print_value(et, true, 10, false, 0, 0, span)?;
        self.asm.load(4, false, EAX, EBP, i);
        self.asm.alu_ri(Alu::Add, EAX, 1);
        self.asm.store(4, EAX, EBP, i);
        self.asm.jmp(top);
        self.asm.bind(done);
        self.print_lit(b"]");
        Ok(())
    }
}
