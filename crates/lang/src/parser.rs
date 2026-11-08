//! Recursive-descent parser for R++.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::ast::*;
use crate::lexer::{Tok, Token};
use crate::Error;

pub struct Parser<'t> {
    toks: &'t [Token],
    pos: usize,
}

type R<T> = Result<T, Error>;

fn bin_prec(p: &str) -> Option<(u8, BinOp)> {
    Some(match p {
        "||" => (1, BinOp::LOr),
        "&&" => (2, BinOp::LAnd),
        "==" => (3, BinOp::Eq),
        "!=" => (3, BinOp::Ne),
        "<" => (3, BinOp::Lt),
        "<=" => (3, BinOp::Le),
        ">" => (3, BinOp::Gt),
        ">=" => (3, BinOp::Ge),
        "|" => (4, BinOp::Or),
        "^" => (5, BinOp::Xor),
        "&" => (6, BinOp::And),
        "<<" => (7, BinOp::Shl),
        ">>" => (7, BinOp::Shr),
        "+" => (8, BinOp::Add),
        "-" => (8, BinOp::Sub),
        "*" => (9, BinOp::Mul),
        "/" => (9, BinOp::Div),
        "%" => (9, BinOp::Rem),
        _ => return None,
    })
}

fn op_assign(p: &str) -> Option<BinOp> {
    Some(match p {
        "+=" => BinOp::Add,
        "-=" => BinOp::Sub,
        "*=" => BinOp::Mul,
        "/=" => BinOp::Div,
        "%=" => BinOp::Rem,
        "&=" => BinOp::And,
        "|=" => BinOp::Or,
        "^=" => BinOp::Xor,
        "<<=" => BinOp::Shl,
        ">>=" => BinOp::Shr,
        _ => return None,
    })
}

fn block_like(e: &ExprKind) -> bool {
    matches!(
        e,
        ExprKind::Block(_) | ExprKind::If(..) | ExprKind::While(..) | ExprKind::Loop(_) | ExprKind::For(..) | ExprKind::Match(..)
    )
}

impl<'t> Parser<'t> {
    pub fn new(toks: &'t [Token]) -> Self {
        Parser { toks, pos: 0 }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }
    fn peek_at(&self, k: usize) -> &Tok {
        &self.toks[(self.pos + k).min(self.toks.len() - 1)].tok
    }
    fn span(&self) -> Span {
        self.toks[self.pos].span
    }
    fn bump(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn is(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(q) if *q == p)
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Ident(s) if s == k)
    }
    fn eat(&mut self, p: &str) -> bool {
        if self.is(p) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn eat_kw(&mut self, k: &str) -> bool {
        if self.is_kw(k) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn err<T>(&self, msg: &str) -> R<T> {
        Err(Error::new(self.span(), msg))
    }
    fn found(&self) -> String {
        match self.peek() {
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Int(v, _) => format!("`{v}`"),
            Tok::Str(_) | Tok::ByteStr(_) => String::from("a string"),
            Tok::Char(_) | Tok::Byte(_) => String::from("a character"),
            Tok::Punct(p) => format!("`{p}`"),
            Tok::Eof => String::from("end of file"),
            Tok::Lifetime => String::from("a lifetime"),
        }
    }
    fn expect(&mut self, p: &str) -> R<()> {
        if self.eat(p) {
            Ok(())
        } else {
            self.err(&format!("expected `{p}`, found {}", self.found()))
        }
    }
    fn ident(&mut self) -> R<String> {
        match self.peek().clone() {
            Tok::Ident(s) if !is_keyword(&s) => {
                self.bump();
                Ok(s)
            }
            _ => self.err(&format!("expected a name, found {}", self.found())),
        }
    }

    fn skip_lifetime(&mut self) {
        if matches!(self.peek(), Tok::Lifetime) {
            self.bump();
        }
    }

    /// `<'a, 'b>` after a function or struct name.
    fn skip_lifetime_params(&mut self) -> R<()> {
        if self.is("<") && matches!(self.peek_at(1), Tok::Lifetime) {
            self.bump();
            while !self.eat(">") {
                match self.bump() {
                    Tok::Lifetime | Tok::Punct(",") => {}
                    _ => return self.err("generic parameters are not supported (only lifetimes)"),
                }
            }
        } else if self.is("<") {
            return self.err("generics are not supported in R++");
        }
        Ok(())
    }

    fn skip_attributes(&mut self) -> R<()> {
        while self.is("#") {
            self.bump();
            self.eat("!");
            self.expect("[")?;
            let mut depth = 1;
            while depth > 0 {
                match self.bump() {
                    Tok::Punct("[") => depth += 1,
                    Tok::Punct("]") => depth -= 1,
                    Tok::Eof => return self.err("unterminated attribute"),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn program(&mut self) -> R<Vec<Item>> {
        let mut items = Vec::new();
        loop {
            self.skip_attributes()?;
            if matches!(self.peek(), Tok::Eof) {
                return Ok(items);
            }
            self.item(&mut items)?;
        }
    }

    fn item(&mut self, items: &mut Vec<Item>) -> R<()> {
        let span = self.span();
        self.eat_kw("pub");
        if self.eat_kw("use") {
            while !self.is(";") {
                if matches!(self.peek(), Tok::Eof) {
                    return self.err("expected `;`");
                }
                self.bump();
            }
            self.bump();
            return Ok(());
        }
        self.eat_kw("unsafe");
        if self.is_kw("fn") {
            items.push(Item::Fn(self.fn_decl(false)?));
        } else if self.eat_kw("struct") {
            let name = self.ident()?;
            self.skip_lifetime_params()?;
            let mut fields = Vec::new();
            if !self.eat(";") {
                self.expect("{")?;
                while !self.eat("}") {
                    self.skip_attributes()?;
                    self.eat_kw("pub");
                    let f = self.ident()?;
                    self.expect(":")?;
                    fields.push((f, self.ty()?));
                    if !self.eat(",") {
                        self.expect("}")?;
                        break;
                    }
                }
            }
            items.push(Item::Struct(StructDecl { name, fields, span }));
        } else if self.eat_kw("enum") {
            let name = self.ident()?;
            self.expect("{")?;
            let mut variants = Vec::new();
            while !self.eat("}") {
                self.skip_attributes()?;
                let v = self.ident()?;
                let val = if self.eat("=") { Some(self.expr()?) } else { None };
                variants.push((v, val));
                if !self.eat(",") {
                    self.expect("}")?;
                    break;
                }
            }
            items.push(Item::Enum(EnumDecl { name, variants, span }));
        } else if self.eat_kw("const") {
            let name = self.ident()?;
            self.expect(":")?;
            let ty = self.ty()?;
            self.expect("=")?;
            let e = self.expr()?;
            self.expect(";")?;
            items.push(Item::Const(name, ty, e, span));
        } else if self.eat_kw("static") {
            self.eat_kw("mut");
            let name = self.ident()?;
            self.expect(":")?;
            let ty = self.ty()?;
            self.expect("=")?;
            let e = self.expr()?;
            self.expect(";")?;
            items.push(Item::Static(name, ty, e, span));
        } else if self.eat_kw("impl") {
            self.skip_lifetime_params()?;
            let name = self.ident()?;
            self.skip_lifetime_params()?;
            self.expect("{")?;
            let mut fns = Vec::new();
            while !self.eat("}") {
                self.skip_attributes()?;
                self.eat_kw("pub");
                self.eat_kw("unsafe");
                if !self.is_kw("fn") {
                    return self.err(&format!("expected `fn` in impl block, found {}", self.found()));
                }
                fns.push(self.fn_decl(true)?);
            }
            items.push(Item::Impl(name, fns, span));
        } else {
            return self.err(&format!("expected an item (fn, struct, enum, const, static, impl), found {}", self.found()));
        }
        Ok(())
    }

    fn fn_decl(&mut self, in_impl: bool) -> R<FnDecl> {
        let span = self.span();
        self.bump(); // fn
        let name = self.ident()?;
        self.skip_lifetime_params()?;
        self.expect("(")?;
        let mut params = Vec::new();
        let mut self_kind = None;
        let mut self_mut = false;
        if in_impl {
            // self, mut self, &self, &mut self
            let save = self.pos;
            let is_ref = self.eat("&");
            let m = self.eat_kw("mut");
            if self.eat_kw("self") {
                self_kind = Some(if is_ref { SelfKind::Ref } else { SelfKind::Value });
                self_mut = m;
                if !self.eat(",") && !self.is(")") {
                    return self.err("expected `,` or `)`");
                }
            } else {
                self.pos = save;
            }
        }
        while !self.eat(")") {
            let mutable = self.eat_kw("mut");
            let pname = if self.eat_kw("_") { String::from("_") } else { self.ident()? };
            self.expect(":")?;
            let ty = self.ty()?;
            params.push(Param { name: pname, ty, mutable });
            if !self.eat(",") {
                self.expect(")")?;
                break;
            }
        }
        let ret = if self.eat("->") { Some(self.ty()?) } else { None };
        let body = self.block()?;
        Ok(FnDecl { name, self_kind, self_mut, params, ret, body, span })
    }

    pub fn ty(&mut self) -> R<TypeExpr> {
        let span = self.span();
        if self.eat("&") {
            self.skip_lifetime();
            self.eat_kw("mut");
            return Ok(TypeExpr::Ptr(Box::new(self.ty()?)));
        }
        if self.eat("&&") {
            self.skip_lifetime();
            self.eat_kw("mut");
            return Ok(TypeExpr::Ptr(Box::new(TypeExpr::Ptr(Box::new(self.ty()?)))));
        }
        if self.eat("*") {
            if !self.eat_kw("const") && !self.eat_kw("mut") {
                return self.err("expected `const` or `mut` after `*`");
            }
            return Ok(TypeExpr::Ptr(Box::new(self.ty()?)));
        }
        if self.eat("[") {
            let elem = self.ty()?;
            if self.eat(";") {
                let n = self.expr()?;
                self.expect("]")?;
                return Ok(TypeExpr::Array(Box::new(elem), Box::new(n)));
            }
            self.expect("]")?;
            return Ok(TypeExpr::Slice(Box::new(elem)));
        }
        if self.eat("(") {
            self.expect(")")?;
            return Ok(TypeExpr::Unit);
        }
        if self.eat("!") {
            return Ok(TypeExpr::Never);
        }
        let mut name = match self.peek().clone() {
            Tok::Ident(s) => {
                self.bump();
                s
            }
            _ => return self.err(&format!("expected a type, found {}", self.found())),
        };
        // Paths like std::primitive::u8: keep the last segment.
        while self.eat("::") {
            name = self.ident()?;
        }
        Ok(TypeExpr::Name(name, span))
    }

    pub fn block(&mut self) -> R<Block> {
        let span = self.span();
        self.expect("{")?;
        let mut b = Block { stmts: Vec::new(), tail: None, span };
        loop {
            self.skip_attributes()?;
            if self.eat("}") {
                return Ok(b);
            }
            if self.eat(";") {
                continue;
            }
            if self.is_kw("let") {
                let span = self.span();
                self.bump();
                let mutable = self.eat_kw("mut");
                let name = if self.eat_kw("_") { String::from("_") } else { self.ident()? };
                let ty = if self.eat(":") { Some(self.ty()?) } else { None };
                let init = if self.eat("=") { Some(self.expr()?) } else { None };
                self.expect(";")?;
                b.stmts.push(Stmt::Let { name, mutable, ty, init, span });
                continue;
            }
            if self.is_kw("fn") || self.is_kw("struct") || self.is_kw("const") || self.is_kw("static") {
                return self.err("items inside functions are not supported; move it to the top level");
            }
            let e = self.expr_stmt()?;
            if self.eat(";") {
                b.stmts.push(Stmt::Expr(e));
            } else if self.is("}") {
                b.tail = Some(Box::new(e));
            } else if block_like(&e.kind) {
                b.stmts.push(Stmt::Expr(e));
            } else {
                return self.err(&format!("expected `;`, found {}", self.found()));
            }
        }
    }

    /// An expression in statement position: block-like expressions end the
    /// statement without a semicolon.
    fn expr_stmt(&mut self) -> R<Expr> {
        let starts_block = matches!(self.peek(), Tok::Punct("{"))
            || ["if", "while", "loop", "for", "match", "unsafe"].iter().any(|k| self.is_kw(k));
        if starts_block {
            let e = self.primary(false)?;
            if block_like(&e.kind) {
                if self.is(".") || self.is("?") {
                    return self.postfix_from(e, false).and_then(|e| self.binary_rest(e, 0, false));
                }
                return Ok(e);
            }
            return self.binary_rest(e, 0, false);
        }
        self.expr()
    }

    pub fn expr(&mut self) -> R<Expr> {
        self.expr_ns(false)
    }

    fn expr_ns(&mut self, ns: bool) -> R<Expr> {
        self.assign(ns)
    }

    fn assign(&mut self, ns: bool) -> R<Expr> {
        let span = self.span();
        let lhs = self.range(ns)?;
        if self.eat("=") {
            let rhs = self.assign(ns)?;
            return Ok(Expr { kind: ExprKind::Assign(Box::new(lhs), Box::new(rhs)), span });
        }
        if let Tok::Punct(p) = self.peek() {
            if let Some(op) = op_assign(p) {
                self.bump();
                let rhs = self.assign(ns)?;
                return Ok(Expr { kind: ExprKind::OpAssign(op, Box::new(lhs), Box::new(rhs)), span });
            }
        }
        Ok(lhs)
    }

    fn range_end(&self) -> bool {
        matches!(self.peek(), Tok::Punct(")" | "]" | "}" | "," | ";" | "{") | Tok::Eof)
    }

    fn range(&mut self, ns: bool) -> R<Expr> {
        let span = self.span();
        if self.is("..") || self.is("..=") {
            let incl = self.bump() == Tok::Punct("..=");
            let hi = if self.range_end() { None } else { Some(Box::new(self.binary(0, ns)?)) };
            return Ok(Expr { kind: ExprKind::Range(None, hi, incl), span });
        }
        let lo = self.binary(0, ns)?;
        if self.is("..") || self.is("..=") {
            let incl = self.bump() == Tok::Punct("..=");
            let hi = if self.range_end() || (ns && self.is("{")) { None } else { Some(Box::new(self.binary(0, ns)?)) };
            return Ok(Expr { kind: ExprKind::Range(Some(Box::new(lo)), hi, incl), span });
        }
        Ok(lo)
    }

    fn binary(&mut self, min: u8, ns: bool) -> R<Expr> {
        let lhs = self.unary(ns)?;
        self.binary_rest(lhs, min, ns)
    }

    fn binary_rest(&mut self, mut lhs: Expr, min: u8, ns: bool) -> R<Expr> {
        loop {
            let (prec, op) = match self.peek() {
                Tok::Punct(p) => match bin_prec(p) {
                    Some(x) => x,
                    None => return Ok(lhs),
                },
                _ => return Ok(lhs),
            };
            if prec <= min {
                return Ok(lhs);
            }
            let span = lhs.span;
            self.bump();
            let rhs = self.binary(prec, ns)?;
            if prec == 3 {
                if let ExprKind::Binary(o, ..) = &lhs.kind {
                    if bin_prec_of(*o) == 3 {
                        return Err(Error::new(span, "comparison operators cannot be chained; use parentheses"));
                    }
                }
            }
            lhs = Expr { kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span };
        }
    }

    /// A unary expression followed by any `as` casts (unary binds tighter).
    fn unary(&mut self, ns: bool) -> R<Expr> {
        let e = self.unary_inner(ns)?;
        self.casts(e)
    }

    fn unary_inner(&mut self, ns: bool) -> R<Expr> {
        let span = self.span();
        if self.eat("-") {
            let e = self.unary_inner(ns)?;
            return Ok(Expr { kind: ExprKind::Unary(UnOp::Neg, Box::new(e)), span });
        }
        if self.eat("!") {
            let e = self.unary_inner(ns)?;
            return Ok(Expr { kind: ExprKind::Unary(UnOp::Not, Box::new(e)), span });
        }
        if self.eat("*") {
            let e = self.unary_inner(ns)?;
            return Ok(Expr { kind: ExprKind::Deref(Box::new(e)), span });
        }
        if self.eat("&") {
            self.eat_kw("mut");
            let e = self.unary_inner(ns)?;
            return Ok(Expr { kind: ExprKind::Ref(Box::new(e)), span });
        }
        if self.eat("&&") {
            self.eat_kw("mut");
            let e = self.unary_inner(ns)?;
            let inner = Expr { kind: ExprKind::Ref(Box::new(e)), span };
            return Ok(Expr { kind: ExprKind::Ref(Box::new(inner)), span });
        }
        self.postfix(ns)
    }

    fn casts(&mut self, mut e: Expr) -> R<Expr> {
        while self.eat_kw("as") {
            let span = e.span;
            let t = self.ty()?;
            e = Expr { kind: ExprKind::Cast(Box::new(e), t), span };
        }
        Ok(e)
    }

    fn postfix(&mut self, ns: bool) -> R<Expr> {
        let e = self.primary(ns)?;
        self.postfix_from(e, ns)
    }

    fn postfix_from(&mut self, mut e: Expr, _ns: bool) -> R<Expr> {
        loop {
            let span = self.span();
            if self.eat(".") {
                match self.peek().clone() {
                    Tok::Ident(name) => {
                        self.bump();
                        if self.eat("(") {
                            let args = self.args(")")?;
                            e = Expr { kind: ExprKind::MethodCall(Box::new(e), name, args), span };
                        } else {
                            e = Expr { kind: ExprKind::Field(Box::new(e), name), span };
                        }
                    }
                    _ => return self.err("expected a field or method name after `.`"),
                }
            } else if self.eat("[") {
                let idx = self.expr()?;
                self.expect("]")?;
                e = Expr { kind: ExprKind::Index(Box::new(e), Box::new(idx)), span };
            } else if self.is("?") {
                return self.err("the `?` operator is not supported");
            } else {
                return Ok(e);
            }
        }
    }

    fn args(&mut self, close: &str) -> R<Vec<Expr>> {
        let mut v = Vec::new();
        while !self.eat(close) {
            v.push(self.expr()?);
            if !self.eat(",") {
                self.expect(close)?;
                break;
            }
        }
        Ok(v)
    }

    fn primary(&mut self, ns: bool) -> R<Expr> {
        let span = self.span();
        let mk = |kind| Ok(Expr { kind, span });
        match self.peek().clone() {
            Tok::Int(v, s) => {
                self.bump();
                mk(ExprKind::Int(v, s))
            }
            Tok::Str(s) => {
                self.bump();
                mk(ExprKind::Str(s))
            }
            Tok::ByteStr(s) => {
                self.bump();
                mk(ExprKind::ByteStr(s))
            }
            Tok::Char(c) => {
                self.bump();
                mk(ExprKind::Char(c))
            }
            Tok::Byte(c) => {
                self.bump();
                mk(ExprKind::Byte(c))
            }
            Tok::Punct("(") => {
                self.bump();
                if self.eat(")") {
                    return mk(ExprKind::Unit);
                }
                let e = self.expr()?;
                if self.is(",") {
                    return self.err("tuples are not supported");
                }
                self.expect(")")?;
                Ok(e)
            }
            Tok::Punct("[") => {
                self.bump();
                if self.eat("]") {
                    return mk(ExprKind::ArrayLit(Vec::new()));
                }
                let first = self.expr()?;
                if self.eat(";") {
                    let n = self.expr()?;
                    self.expect("]")?;
                    return mk(ExprKind::ArrayRepeat(Box::new(first), Box::new(n)));
                }
                let mut v = vec![first];
                if self.eat(",") {
                    v.extend(self.args("]")?);
                } else {
                    self.expect("]")?;
                }
                mk(ExprKind::ArrayLit(v))
            }
            Tok::Punct("{") => mk(ExprKind::Block(self.block()?)),
            Tok::Punct("|") | Tok::Punct("||") => self.err("closures are not supported"),
            Tok::Ident(k) => match k.as_str() {
                "true" | "false" => {
                    self.bump();
                    mk(ExprKind::Bool(k == "true"))
                }
                "unsafe" => {
                    self.bump();
                    mk(ExprKind::Block(self.block()?))
                }
                "if" => self.if_expr(),
                "while" => {
                    self.bump();
                    if self.is_kw("let") {
                        return self.err("`while let` is not supported");
                    }
                    let c = self.expr_ns(true)?;
                    let b = self.block()?;
                    mk(ExprKind::While(Box::new(c), b))
                }
                "loop" => {
                    self.bump();
                    mk(ExprKind::Loop(self.block()?))
                }
                "for" => {
                    self.bump();
                    let v = if self.eat_kw("_") { String::from("_") } else { self.ident()? };
                    if !self.eat_kw("in") {
                        return self.err("expected `in`");
                    }
                    let it = self.expr_ns(true)?;
                    let b = self.block()?;
                    mk(ExprKind::For(v, Box::new(it), b))
                }
                "match" => self.match_expr(),
                "break" => {
                    self.bump();
                    if !self.range_end() {
                        return self.err("`break` with a value is not supported");
                    }
                    mk(ExprKind::Break)
                }
                "continue" => {
                    self.bump();
                    mk(ExprKind::Continue)
                }
                "return" => {
                    self.bump();
                    let v = if self.range_end() { None } else { Some(Box::new(self.expr()?)) };
                    mk(ExprKind::Return(v))
                }
                _ if is_keyword(&k) && k != "self" && k != "Self" => self.err(&format!("unexpected keyword `{k}`")),
                _ => self.path_expr(ns),
            },
            _ => self.err(&format!("expected an expression, found {}", self.found())),
        }
    }

    fn path_expr(&mut self, ns: bool) -> R<Expr> {
        let span = self.span();
        let mut path = Vec::new();
        match self.bump() {
            Tok::Ident(s) => path.push(s),
            _ => unreachable!(),
        }
        while self.is("::") {
            self.bump();
            path.push(self.ident_or_self()?);
        }
        if path.len() == 1 && self.is("!") && matches!(self.peek_at(1), Tok::Punct("(" | "[")) {
            self.bump();
            let close = if self.eat("(") { ")" } else {
                self.bump();
                "]"
            };
            let args = self.args(close)?;
            return Ok(Expr { kind: ExprKind::Macro(path.pop().unwrap(), args), span });
        }
        if self.eat("(") {
            let args = self.args(")")?;
            return Ok(Expr { kind: ExprKind::Call(path, args), span });
        }
        if !ns && self.is("{") && path.len() == 1 && self.looks_like_struct_lit() {
            self.bump();
            let mut fields = Vec::new();
            while !self.eat("}") {
                let fspan = self.span();
                let f = self.ident()?;
                let e = if self.eat(":") {
                    self.expr()?
                } else {
                    Expr { kind: ExprKind::Path(vec![f.clone()]), span: fspan }
                };
                fields.push((f, e));
                if !self.eat(",") {
                    self.expect("}")?;
                    break;
                }
            }
            return Ok(Expr { kind: ExprKind::StructLit(path.pop().unwrap(), fields), span });
        }
        Ok(Expr { kind: ExprKind::Path(path), span })
    }

    fn ident_or_self(&mut self) -> R<String> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.bump();
                Ok(s)
            }
            _ => self.err("expected a name after `::`"),
        }
    }

    fn looks_like_struct_lit(&self) -> bool {
        match (self.peek_at(1), self.peek_at(2)) {
            (Tok::Punct("}"), _) => true,
            (Tok::Ident(_), Tok::Punct(":" | "," | "}")) => true,
            _ => false,
        }
    }

    fn if_expr(&mut self) -> R<Expr> {
        let span = self.span();
        self.bump(); // if
        if self.is_kw("let") {
            return self.err("`if let` is not supported; use `match`");
        }
        let c = self.expr_ns(true)?;
        let then = self.block()?;
        let els = if self.eat_kw("else") {
            if self.is_kw("if") {
                Some(Box::new(self.if_expr()?))
            } else {
                let bspan = self.span();
                let b = self.block()?;
                Some(Box::new(Expr { kind: ExprKind::Block(b), span: bspan }))
            }
        } else {
            None
        };
        Ok(Expr { kind: ExprKind::If(Box::new(c), then, els), span })
    }

    fn match_expr(&mut self) -> R<Expr> {
        let span = self.span();
        self.bump(); // match
        let scrut = self.expr_ns(true)?;
        self.expect("{")?;
        let mut arms = Vec::new();
        while !self.eat("}") {
            let mut pats = vec![self.pattern()?];
            while self.eat("|") {
                pats.push(self.pattern()?);
            }
            let guard = if self.eat_kw("if") { Some(self.expr()?) } else { None };
            self.expect("=>")?;
            let body = self.expr_stmt()?;
            let blocky = block_like(&body.kind);
            arms.push(Arm { pats, guard, body });
            if !self.eat(",") && !blocky && !self.is("}") {
                return self.err(&format!("expected `,` after match arm, found {}", self.found()));
            }
        }
        Ok(Expr { kind: ExprKind::Match(Box::new(scrut), arms), span })
    }

    fn pattern(&mut self) -> R<Pat> {
        if self.eat_kw("_") {
            return Ok(Pat::Wild);
        }
        let span = self.span();
        // A plain lowercase name binds; paths and literals compare.
        if let Tok::Ident(s) = self.peek().clone() {
            if !matches!(self.peek_at(1), Tok::Punct("::" | "(" | "{" | "..=" | "..")) && !is_keyword(&s) {
                let first = s.chars().next().unwrap_or('a');
                if first.is_ascii_lowercase() || first == '_' {
                    self.bump();
                    return Ok(Pat::Bind(s));
                }
            }
        }
        let lo = self.pat_value(span)?;
        if self.is("..=") || self.is("...") {
            self.bump();
            let hi = self.pat_value(self.span())?;
            return Ok(Pat::Range(lo, hi));
        }
        if self.is("..") {
            return self.err("exclusive range patterns are not supported; use `..=`");
        }
        Ok(Pat::Value(lo))
    }

    fn pat_value(&mut self, span: Span) -> R<Expr> {
        if self.eat("-") {
            let e = self.primary(true)?;
            return Ok(Expr { kind: ExprKind::Unary(UnOp::Neg, Box::new(e)), span });
        }
        match self.peek() {
            Tok::Int(..) | Tok::Char(_) | Tok::Byte(_) | Tok::Str(_) | Tok::Ident(_) => self.primary(true),
            _ => self.err(&format!("expected a pattern, found {}", self.found())),
        }
    }
}

fn bin_prec_of(op: BinOp) -> u8 {
    match op {
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 3,
        _ => 0,
    }
}

pub fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break" | "const" | "continue" | "else" | "enum" | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop"
            | "match" | "mod" | "mut" | "pub" | "return" | "static" | "struct" | "true" | "unsafe" | "use" | "while" | "self"
            | "Self" | "_"
    )
}
