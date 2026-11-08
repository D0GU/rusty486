//! Syntax tree for R++.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

pub use crate::lexer::Span;

#[derive(Clone, Debug)]
pub enum TypeExpr {
    /// A named type (`i32`, `Point`, `Self`).
    Name(String, Span),
    /// `&T`, `&mut T`, `*const T`, `*mut T`.
    Ptr(Box<TypeExpr>),
    Array(Box<TypeExpr>, Box<Expr>),
    /// `&[T]`.
    Slice(Box<TypeExpr>),
    Unit,
    Never,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub mutable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelfKind {
    Value,
    Ref,
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub name: String,
    pub self_kind: Option<SelfKind>,
    pub self_mut: bool,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<(String, TypeExpr)>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<(String, Option<Expr>)>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Item {
    Fn(FnDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Const(String, TypeExpr, Expr, Span),
    Static(String, TypeExpr, Expr, Span),
    Impl(String, Vec<FnDecl>, Span),
}

#[derive(Clone, Debug, Default)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub tail: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let { name: String, mutable: bool, ty: Option<TypeExpr>, init: Option<Expr>, span: Span },
    Expr(Expr),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    LAnd,
    LOr,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug)]
pub enum Pat {
    Wild,
    Bind(String),
    /// A literal, constant or enum variant (an expression evaluated at compile time).
    Value(Expr),
    Range(Expr, Expr),
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub pats: Vec<Pat>,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(u64, Option<String>),
    Bool(bool),
    Char(u32),
    Byte(u8),
    Str(Vec<u8>),
    ByteStr(Vec<u8>),
    Unit,
    Path(Vec<String>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    OpAssign(BinOp, Box<Expr>, Box<Expr>),
    Call(Vec<String>, Vec<Expr>),
    MethodCall(Box<Expr>, String, Vec<Expr>),
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Range(Option<Box<Expr>>, Option<Box<Expr>>, bool),
    Cast(Box<Expr>, TypeExpr),
    Ref(Box<Expr>),
    Deref(Box<Expr>),
    Block(Block),
    If(Box<Expr>, Block, Option<Box<Expr>>),
    While(Box<Expr>, Block),
    Loop(Block),
    For(String, Box<Expr>, Block),
    Break,
    Continue,
    Return(Option<Box<Expr>>),
    Match(Box<Expr>, Vec<Arm>),
    StructLit(String, Vec<(String, Expr)>),
    ArrayLit(Vec<Expr>),
    ArrayRepeat(Box<Expr>, Box<Expr>),
    Macro(String, Vec<Expr>),
}
