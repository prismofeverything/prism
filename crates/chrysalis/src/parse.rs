//! Lexer + recursive-descent parser for the chrysalis surface syntax (`.ys`).
//!
//! Structure (the confirmed design): ONE shared expression/body grammar, with
//! thin declaration wrappers (`type`/`process`/`step`/`composite`/`reaction`/
//! `extern`/`unit`/`context`/`import`/`name = expr`) that attach invocation +
//! binding to a shared body. Produces the existing [`crate::ast`] (`Program` /
//! `Def` / `Expr` / `SchemaExpr`) — the parser is a thin front-end; everything
//! downstream (eval, compile) is unchanged.
//!
//! Built incrementally; this file currently covers the lexer + the expression /
//! schema / `type` / binding grammar (the graph-type target). Process / step /
//! composite / reaction / import wrappers (the `~{}->{}` / `|` / term grammar)
//! land next.

// ─────────────────────────────────────────────────────────────────────
// Tokens
// ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),

    // keywords
    Type,
    With,
    Process,
    Step,
    Composite,
    Reaction,
    Extern,
    Unit,
    Context,
    Import,
    If,
    Then,
    Else,
    Let,
    In,
    For,
    Not,
    And,
    Or,
    True,
    False,
    Replace,

    // operators / punctuation
    Eq,     // =
    EqEq,   // ==
    Ne,     // !=
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    PlusPlus, // ++
    AmpAmp,   // &&
    BarBar,   // ||
    Arrow,    // ->
    FatArrow, // =>
    Tilde,    // ~
    Bar,      // |
    At,       // @
    Bang,     // !
    Question, // ?
    Dot,
    Comma,
    Colon,
    LParen,
    RParen,
    LBrack,
    RBrack,
    LBrace,
    RBrace,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Spanned {
    pub tok: Tok,
    pub line: usize,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("parse error (line {line}): {message}")]
pub struct ParseError {
    pub message: String,
    pub line: usize,
}

fn keyword(word: &str) -> Option<Tok> {
    Some(match word {
        "type" => Tok::Type,
        "with" => Tok::With,
        "process" => Tok::Process,
        "step" => Tok::Step,
        "composite" => Tok::Composite,
        "reaction" => Tok::Reaction,
        "extern" => Tok::Extern,
        "unit" => Tok::Unit,
        "context" => Tok::Context,
        "import" => Tok::Import,
        // NOTE: `from` is a CONTEXTUAL keyword (only meaningful after `import`),
        // not reserved — it's a common field name (a graph edge's `from`). The
        // import parser matches the bare identifier `from` instead.
        "if" => Tok::If,
        "then" => Tok::Then,
        "else" => Tok::Else,
        "let" => Tok::Let,
        "in" => Tok::In,
        "for" => Tok::For,
        "not" => Tok::Not,
        "and" => Tok::And,
        "or" => Tok::Or,
        "true" => Tok::True,
        "false" => Tok::False,
        "replace" => Tok::Replace,
        _ => return None,
    })
}

/// Tokenize `.ys` source. Whitespace and `#`-to-end-of-line comments are
/// skipped; newlines are insignificant (the grammar uses explicit delimiters).
pub fn lex(src: &str) -> Result<Vec<Spanned>, ParseError> {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut i = 0;
    let mut line = 1usize;
    let mut out: Vec<Spanned> = Vec::new();
    let push = |tok: Tok, line: usize, out: &mut Vec<Spanned>| out.push(Spanned { tok, line });

    while i < n {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' => i += 1,
            '\n' => {
                line += 1;
                i += 1;
            }
            '#' => {
                while i < n && chars[i] != '\n' {
                    i += 1;
                }
            }
            // identifiers / keywords
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < n && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                push(keyword(&word).unwrap_or(Tok::Ident(word)), line, &mut out);
            }
            // numbers (int / float, with optional scientific exponent)
            c if c.is_ascii_digit() => {
                let start = i;
                let mut is_float = false;
                while i < n && chars[i].is_ascii_digit() {
                    i += 1;
                }
                // fractional part — only if a digit follows the `.`
                if i + 1 < n && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
                    is_float = true;
                    i += 1;
                    while i < n && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                // exponent
                if i < n && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    if j < n && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    if j < n && chars[j].is_ascii_digit() {
                        is_float = true;
                        i = j;
                        while i < n && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                let text: String = chars[start..i].iter().collect();
                if is_float {
                    let f = text.parse::<f64>().map_err(|_| ParseError {
                        message: format!("bad float `{text}`"),
                        line,
                    })?;
                    push(Tok::Float(f), line, &mut out);
                } else {
                    let v = text.parse::<i64>().map_err(|_| ParseError {
                        message: format!("bad int `{text}`"),
                        line,
                    })?;
                    push(Tok::Int(v), line, &mut out);
                }
            }
            // strings (single or double quoted; raw content for now)
            '\'' | '"' => {
                let quote = c;
                i += 1;
                let start = i;
                while i < n && chars[i] != quote {
                    if chars[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
                if i >= n {
                    return Err(ParseError { message: "unterminated string".into(), line });
                }
                let s: String = chars[start..i].iter().collect();
                i += 1; // closing quote
                push(Tok::Str(s), line, &mut out);
            }
            // multi- and single-char operators
            _ => {
                let two: String = chars[i..(i + 2).min(n)].iter().collect();
                let (tok, len) = match two.as_str() {
                    "==" => (Tok::EqEq, 2),
                    "!=" => (Tok::Ne, 2),
                    "<=" => (Tok::Le, 2),
                    ">=" => (Tok::Ge, 2),
                    "++" => (Tok::PlusPlus, 2),
                    "&&" => (Tok::AmpAmp, 2),
                    "||" => (Tok::BarBar, 2),
                    "->" => (Tok::Arrow, 2),
                    "=>" => (Tok::FatArrow, 2),
                    _ => {
                        let t = match c {
                            '=' => Tok::Eq,
                            '<' => Tok::Lt,
                            '>' => Tok::Gt,
                            '+' => Tok::Plus,
                            '-' => Tok::Minus,
                            '*' => Tok::Star,
                            '/' => Tok::Slash,
                            '~' => Tok::Tilde,
                            '|' => Tok::Bar,
                            '@' => Tok::At,
                            '!' => Tok::Bang,
                            '?' => Tok::Question,
                            '.' => Tok::Dot,
                            ',' => Tok::Comma,
                            ':' => Tok::Colon,
                            '(' => Tok::LParen,
                            ')' => Tok::RParen,
                            '[' => Tok::LBrack,
                            ']' => Tok::RBrack,
                            '{' => Tok::LBrace,
                            '}' => Tok::RBrace,
                            other => {
                                return Err(ParseError {
                                    message: format!("unexpected character `{other}`"),
                                    line,
                                })
                            }
                        };
                        (t, 1)
                    }
                };
                i += len;
                push(tok, line, &mut out);
            }
        }
    }
    push(Tok::Eof, line, &mut out);
    Ok(out)
}

#[cfg(test)]
mod lex_tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        lex(src).unwrap().into_iter().map(|s| s.tok).collect()
    }

    #[test]
    fn lexes_keywords_idents_ops() {
        let t = toks("type Graph = { nodes: list[string] } with");
        assert_eq!(
            t,
            vec![
                Tok::Type,
                Tok::Ident("Graph".into()),
                Tok::Eq,
                Tok::LBrace,
                Tok::Ident("nodes".into()),
                Tok::Colon,
                Tok::Ident("list".into()),
                Tok::LBrack,
                Tok::Ident("string".into()),
                Tok::RBrack,
                Tok::RBrace,
                Tok::With,
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_numbers_and_comments() {
        // `#` comment skipped; int, float, scientific.
        let t = toks("# a comment\n1 0.02 1e-12 x.field");
        assert_eq!(
            t,
            vec![
                Tok::Int(1),
                Tok::Float(0.02),
                Tok::Float(1e-12),
                Tok::Ident("x".into()),
                Tok::Dot,
                Tok::Ident("field".into()),
                Tok::Eof,
            ]
        );
    }

    #[test]
    fn lexes_comprehension_and_membership_ops() {
        let t = toks("[e.to for e in xs if not (e == y)]");
        assert!(t.contains(&Tok::For) && t.contains(&Tok::In) && t.contains(&Tok::Not) && t.contains(&Tok::EqEq));
    }

    #[test]
    fn lexes_bigraph_punctuation() {
        let t = toks("~{a} ->{b} | x ++ y => @");
        assert!(t.contains(&Tok::Tilde) && t.contains(&Tok::Arrow) && t.contains(&Tok::Bar)
            && t.contains(&Tok::PlusPlus) && t.contains(&Tok::FatArrow) && t.contains(&Tok::At));
    }
}

// ─────────────────────────────────────────────────────────────────────
// Parser
// ─────────────────────────────────────────────────────────────────────

use indexmap::IndexMap;

use crate::ast::{
    BinOp, Def, Expr, MethodDef, Param, PlacePath, Program, SchemaExpr, StringLit, TypeDef, UnaryOp,
};

struct Parser {
    toks: Vec<Spanned>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }
    fn line(&self) -> usize {
        self.toks[self.pos].line
    }
    fn bump(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }
    fn check(&self, t: &Tok) -> bool {
        self.peek() == t
    }
    fn accept(&mut self, t: &Tok) -> bool {
        if self.check(t) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, t: &Tok) -> Result<(), ParseError> {
        if self.check(t) {
            self.bump();
            Ok(())
        } else {
            Err(self.err(&format!("expected {t:?}, found {:?}", self.peek())))
        }
    }
    fn err(&self, message: &str) -> ParseError {
        ParseError { message: message.to_string(), line: self.line() }
    }
    fn ident(&mut self) -> Result<String, ParseError> {
        match self.bump() {
            Tok::Ident(s) => Ok(s),
            other => Err(self.err(&format!("expected identifier, found {other:?}"))),
        }
    }
}

/// Parse a full `.ys` program.
pub fn parse_program(src: &str) -> Result<Program, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0 };
    let mut program = Program::new();
    while !p.check(&Tok::Eof) {
        program.push(p.parse_def()?);
    }
    Ok(program)
}

impl Parser {
    fn parse_def(&mut self) -> Result<Def, ParseError> {
        match self.peek() {
            Tok::Type => self.parse_type_def(),
            // `name = expr` binding (e.g. `main = …`).
            Tok::Ident(_) => {
                let name = self.ident()?;
                self.expect(&Tok::Eq)?;
                let value = self.parse_expr()?;
                Ok(Def::Binding { name, value })
            }
            other => Err(self.err(&format!("expected a declaration, found {other:?}"))),
        }
    }

    // ── type declaration ────────────────────────────────────────────
    // `type Name = <schema> ( with { method* } )?`
    fn parse_type_def(&mut self) -> Result<Def, ParseError> {
        self.expect(&Tok::Type)?;
        let name = self.ident()?;
        self.expect(&Tok::Eq)?;
        let representation = self.parse_schema()?;
        let mut methods = Vec::new();
        if self.accept(&Tok::With) {
            self.expect(&Tok::LBrace)?;
            while !self.check(&Tok::RBrace) {
                methods.push(self.parse_method()?);
            }
            self.expect(&Tok::RBrace)?;
        }
        Ok(Def::Type(TypeDef { name, params: vec![], representation, methods }))
    }

    // `name(params) = expr`
    fn parse_method(&mut self) -> Result<MethodDef, ParseError> {
        let name = self.ident()?;
        self.expect(&Tok::LParen)?;
        let params = self.parse_params()?;
        self.expect(&Tok::RParen)?;
        self.expect(&Tok::Eq)?;
        let body = self.parse_expr()?;
        Ok(MethodDef { name, params, body })
    }

    // `param ("," param)*` ; `param := IDENT ":" schema ("=" expr)?`
    fn parse_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        if self.check(&Tok::RParen) {
            return Ok(params);
        }
        loop {
            let name = self.ident()?;
            self.expect(&Tok::Colon)?;
            let schema = self.parse_schema()?;
            let param = if self.accept(&Tok::Eq) {
                Param::with_default(name, schema, self.parse_expr()?)
            } else {
                Param::required(name, schema)
            };
            params.push(param);
            if !self.accept(&Tok::Comma) {
                break;
            }
        }
        Ok(params)
    }

    // ── schema expressions ──────────────────────────────────────────
    fn parse_schema(&mut self) -> Result<SchemaExpr, ParseError> {
        match self.peek().clone() {
            // `{ field: schema, … }` — a record
            Tok::LBrace => {
                self.bump();
                let mut fields: IndexMap<String, SchemaExpr> = IndexMap::new();
                while !self.check(&Tok::RBrace) {
                    let field = self.ident()?;
                    self.expect(&Tok::Colon)?;
                    fields.insert(field, self.parse_schema()?);
                    if !self.accept(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(&Tok::RBrace)?;
                Ok(SchemaExpr::Record(fields))
            }
            Tok::Ident(name) => {
                self.bump();
                match name.as_str() {
                    "any" => Ok(SchemaExpr::Any),
                    "bool" => Ok(SchemaExpr::Bool),
                    "int" => Ok(SchemaExpr::Int),
                    "float" => Ok(SchemaExpr::Float),
                    "string" => Ok(SchemaExpr::String),
                    "list" => {
                        self.expect(&Tok::LBrack)?;
                        let inner = self.parse_schema()?;
                        self.expect(&Tok::RBrack)?;
                        Ok(SchemaExpr::list_of(inner))
                    }
                    "map" => {
                        self.expect(&Tok::LBrack)?;
                        let inner = self.parse_schema()?;
                        self.expect(&Tok::RBrack)?;
                        Ok(SchemaExpr::map_of(inner))
                    }
                    // Otherwise a (possibly parameterized) custom type.
                    _ => {
                        let mut params = Vec::new();
                        if self.accept(&Tok::LBrack) {
                            while !self.check(&Tok::RBrack) {
                                params.push(self.parse_schema()?);
                                if !self.accept(&Tok::Comma) {
                                    break;
                                }
                            }
                            self.expect(&Tok::RBrack)?;
                        }
                        Ok(SchemaExpr::Custom { name, params })
                    }
                }
            }
            other => Err(self.err(&format!("expected a schema, found {other:?}"))),
        }
    }

    // ── expressions (one shared grammar) ────────────────────────────
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_and()?;
        while self.check(&Tok::Or) || self.check(&Tok::BarBar) {
            self.bump();
            let rhs = self.parse_and()?;
            lhs = binop(BinOp::Or, lhs, rhs);
        }
        Ok(lhs)
    }
    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_not()?;
        while self.check(&Tok::And) || self.check(&Tok::AmpAmp) {
            self.bump();
            let rhs = self.parse_not()?;
            lhs = binop(BinOp::And, lhs, rhs);
        }
        Ok(lhs)
    }
    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        if self.check(&Tok::Not) || self.check(&Tok::Bang) {
            self.bump();
            let operand = self.parse_not()?;
            return Ok(Expr::UnaryOp { op: UnaryOp::Not, operand: Box::new(operand) });
        }
        self.parse_cmp()
    }
    fn parse_cmp(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_concat()?;
        let op = match self.peek() {
            Tok::EqEq => BinOp::Eq,
            Tok::Ne => BinOp::Ne,
            Tok::Lt => BinOp::Lt,
            Tok::Le => BinOp::Le,
            Tok::Gt => BinOp::Gt,
            Tok::Ge => BinOp::Ge,
            Tok::In => BinOp::In,
            _ => return Ok(lhs),
        };
        self.bump();
        let rhs = self.parse_concat()?;
        Ok(binop(op, lhs, rhs))
    }
    fn parse_concat(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_add()?;
        while self.check(&Tok::PlusPlus) {
            self.bump();
            let rhs = self.parse_add()?;
            lhs = binop(BinOp::Concat, lhs, rhs);
        }
        Ok(lhs)
    }
    fn parse_add(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_mul()?;
            lhs = binop(op, lhs, rhs);
        }
        Ok(lhs)
    }
    fn parse_mul(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_postfix()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_postfix()?;
            lhs = binop(op, lhs, rhs);
        }
        Ok(lhs)
    }

    // `primary ( "." IDENT ( "(" args ")" )? )*` — field access + method calls
    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut base = self.parse_primary()?;
        while self.accept(&Tok::Dot) {
            let name = self.ident()?;
            if self.accept(&Tok::LParen) {
                let mut args = Vec::new();
                while !self.check(&Tok::RParen) {
                    args.push(self.parse_expr()?);
                    if !self.accept(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(&Tok::RParen)?;
                base = Expr::Method { receiver: Box::new(base), method: name, args };
            } else {
                // field access — extend a place path
                base = match base {
                    Expr::Var(v) => Expr::Path(PlacePath::local(v).dot(name)),
                    Expr::Path(p) => Expr::Path(p.dot(name)),
                    other => {
                        return Err(self.err(&format!(
                            "field access `.{name}` on a non-path expression {other:?}"
                        )))
                    }
                };
            }
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                Ok(Expr::int(n))
            }
            Tok::Float(f) => {
                self.bump();
                Ok(Expr::float(f))
            }
            Tok::Str(s) => {
                self.bump();
                Ok(Expr::Str(StringLit::plain(s)))
            }
            Tok::True => {
                self.bump();
                Ok(Expr::Bool(true))
            }
            Tok::False => {
                self.bump();
                Ok(Expr::Bool(false))
            }
            Tok::Ident(name) => {
                self.bump();
                Ok(Expr::var(name))
            }
            Tok::LParen => {
                self.bump();
                let e = self.parse_expr()?;
                self.expect(&Tok::RParen)?;
                Ok(e)
            }
            Tok::If => {
                self.bump();
                let cond = self.parse_expr()?;
                self.expect(&Tok::Then)?;
                let then_ = self.parse_expr()?;
                self.expect(&Tok::Else)?;
                let else_ = self.parse_expr()?;
                Ok(Expr::If {
                    cond: Box::new(cond),
                    then_: Box::new(then_),
                    else_: Some(Box::new(else_)),
                })
            }
            Tok::LBrace => self.parse_braces(),
            Tok::LBrack => self.parse_brackets(),
            other => Err(self.err(&format!("unexpected token in expression: {other:?}"))),
        }
    }

    // `{ key: expr, … }` — Record (ident keys) or Map (string keys).
    fn parse_braces(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Tok::LBrace)?;
        let mut record: IndexMap<String, Expr> = IndexMap::new();
        let mut map_entries: Vec<(StringLit, Expr)> = Vec::new();
        let mut is_map = false;
        while !self.check(&Tok::RBrace) {
            let key = match self.bump() {
                Tok::Ident(k) => k,
                Tok::Str(s) => {
                    is_map = true;
                    s
                }
                other => return Err(self.err(&format!("expected a field key, found {other:?}"))),
            };
            self.expect(&Tok::Colon)?;
            let value = self.parse_expr()?;
            if is_map {
                map_entries.push((StringLit::plain(key), value));
            } else {
                record.insert(key, value);
            }
            if !self.accept(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBrace)?;
        if is_map {
            // a string-keyed brace: fold any ident-keyed entries collected
            // before the first string key in too (kept simple: maps use string
            // keys throughout).
            Ok(Expr::Map(map_entries))
        } else {
            Ok(Expr::Record(record))
        }
    }

    // `[ ]` | `[ expr "for" v "in" src ("if" cond)? ]` | `[ expr, … ]`
    fn parse_brackets(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Tok::LBrack)?;
        if self.accept(&Tok::RBrack) {
            return Ok(Expr::List(vec![]));
        }
        let first = self.parse_expr()?;
        if self.accept(&Tok::For) {
            let var = self.ident()?;
            self.expect(&Tok::In)?;
            let source = self.parse_expr()?;
            let filter = if self.accept(&Tok::If) {
                Some(Box::new(self.parse_expr()?))
            } else {
                None
            };
            self.expect(&Tok::RBrack)?;
            return Ok(Expr::Comprehension {
                var,
                source: Box::new(source),
                filter,
                body: Box::new(first),
            });
        }
        let mut items = vec![first];
        while self.accept(&Tok::Comma) {
            if self.check(&Tok::RBrack) {
                break;
            }
            items.push(self.parse_expr()?);
        }
        self.expect(&Tok::RBrack)?;
        Ok(Expr::List(items))
    }
}

fn binop(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }
}
