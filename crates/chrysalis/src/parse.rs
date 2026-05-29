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
    Def,
    With,
    Process,
    Step,
    Composite,
    Reaction,
    Unit,
    Context,
    Using,
    Import,
    Where,
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
    Contract,
    Fulfills,

    // operators / punctuation
    Eq,   // =
    EqEq, // ==
    Ne,   // !=
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    PlusPlus,   // ++
    AmpAmp,     // &&
    BarBar,     // ||
    Arrow,      // ->
    FatArrow,   // =>
    Tilde,      // ~
    Bar,        // |
    At,         // @ (composite bridge: `port :: Type @ internal.path`)
    Percent,    // % (self / here — the enclosing composite's own place)
    Bang,       // !
    Question,   // ?
    Caret,      // ^ (unit/dimension power)
    BiArrow,    // <-> (bidirectional context rule)
    ColonColon, // :: (typed site / as-pattern)
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
        "def" => Tok::Def,
        "with" => Tok::With,
        "process" => Tok::Process,
        "step" => Tok::Step,
        "composite" => Tok::Composite,
        "reaction" => Tok::Reaction,
        "unit" => Tok::Unit,
        "context" => Tok::Context,
        "using" => Tok::Using,
        "import" => Tok::Import,
        "contract" => Tok::Contract,
        "fulfills" => Tok::Fulfills,
        "where" => Tok::Where,
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
                    return Err(ParseError {
                        message: "unterminated string".into(),
                        line,
                    });
                }
                let s: String = chars[start..i].iter().collect();
                i += 1; // closing quote
                push(Tok::Str(s), line, &mut out);
            }
            // multi- and single-char operators
            _ => {
                let three: String = chars[i..(i + 3).min(n)].iter().collect();
                let two: String = chars[i..(i + 2).min(n)].iter().collect();
                let (tok, len) = if three == "<->" {
                    (Tok::BiArrow, 3)
                } else {
                    match two.as_str() {
                        "==" => (Tok::EqEq, 2),
                        "!=" => (Tok::Ne, 2),
                        "<=" => (Tok::Le, 2),
                        ">=" => (Tok::Ge, 2),
                        "++" => (Tok::PlusPlus, 2),
                        "&&" => (Tok::AmpAmp, 2),
                        "||" => (Tok::BarBar, 2),
                        "->" => (Tok::Arrow, 2),
                        "=>" => (Tok::FatArrow, 2),
                        "::" => (Tok::ColonColon, 2),
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
                                '%' => Tok::Percent,
                                '!' => Tok::Bang,
                                '?' => Tok::Question,
                                '^' => Tok::Caret,
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
                                    });
                                }
                            };
                            (t, 1)
                        }
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
        assert!(
            t.contains(&Tok::For)
                && t.contains(&Tok::In)
                && t.contains(&Tok::Not)
                && t.contains(&Tok::EqEq)
        );
    }

    #[test]
    fn lexes_bigraph_punctuation() {
        let t = toks("~{a} ->{b} | x ++ y => @ %");
        assert!(
            t.contains(&Tok::Tilde)
                && t.contains(&Tok::Arrow)
                && t.contains(&Tok::Bar)
                && t.contains(&Tok::PlusPlus)
                && t.contains(&Tok::FatArrow)
                && t.contains(&Tok::At)
                && t.contains(&Tok::Percent)
        );
    }
}

// ─────────────────────────────────────────────────────────────────────
// Parser
// ─────────────────────────────────────────────────────────────────────

use indexmap::IndexMap;

use crate::ast::{
    BinOp, ContractDef, ContractRef, Def, Expr, MethodDef, Param, PlacePath, Program, SchemaExpr,
    StringLit, StringSeg, TypeDef, UnaryOp,
};

struct Parser {
    toks: Vec<Spanned>,
    pos: usize,
    /// Type aliases (`Mass = Quantity[…]`) collected as they're declared, so a
    /// later schema reference (`mass: Mass`) inlines to the aliased schema —
    /// e.g. so the units lowering sees the `Quantity` (not an opaque `Custom`).
    aliases: std::collections::HashMap<String, SchemaExpr>,
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
        ParseError {
            message: message.to_string(),
            line: self.line(),
        }
    }
    /// A TYPE-ascription separator: `::` (canonical) or legacy `:` (accepted
    /// during the migration to `::`=type / `:`=value). Errors if neither.
    fn expect_type_sep(&mut self) -> Result<(), ParseError> {
        if self.accept(&Tok::ColonColon) {
            Ok(())
        } else {
            self.expect(&Tok::Colon)
        }
    }
    /// Optional type-ascription separator (`::`, or legacy `:`).
    fn accept_type_sep(&mut self) -> bool {
        self.accept(&Tok::ColonColon) || self.accept(&Tok::Colon)
    }
    /// A port's contract separator: `fulfills` (canonical) or legacy `::`.
    fn accept_contract_sep(&mut self) -> bool {
        self.accept(&Tok::Fulfills) || self.accept(&Tok::ColonColon)
    }
    fn ident(&mut self) -> Result<String, ParseError> {
        match self.bump() {
            Tok::Ident(s) => Ok(s),
            other => Err(self.err(&format!("expected identifier, found {other:?}"))),
        }
    }

    /// A field/key name that may collide with a keyword — e.g. a `rest<>`
    /// protocol field named `process`. Accepts a plain identifier OR any keyword
    /// token, mapping the keyword back to its source text.
    fn field_name(&mut self) -> Result<String, ParseError> {
        let kw = match self.bump() {
            Tok::Ident(s) => return Ok(s),
            Tok::Type => "type",
            Tok::Def => "def",
            Tok::With => "with",
            Tok::Process => "process",
            Tok::Step => "step",
            Tok::Composite => "composite",
            Tok::Reaction => "reaction",
            Tok::Unit => "unit",
            Tok::Context => "context",
            Tok::Using => "using",
            Tok::Import => "import",
            Tok::Contract => "contract",
            Tok::Fulfills => "fulfills",
            Tok::Where => "where",
            Tok::Replace => "replace",
            other => return Err(self.err(&format!("expected field name, found {other:?}"))),
        };
        Ok(kw.to_string())
    }
}

/// Parse a full `.ys` program (string form; `import` directives are left as
/// `Def::Import` — use [`parse_file`] to resolve them).
pub fn parse_program(src: &str) -> Result<Program, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser {
        toks,
        pos: 0,
        aliases: Default::default(),
    };
    let mut program = Program::new();
    while !p.check(&Tok::Eof) {
        // A capitalized `Name = <schema>` is a TYPE ALIAS (e.g.
        // `Mass = Quantity[unit: pg, extensive]`): recorded + inlined at use
        // sites, not emitted as a Def. (Lowercase `name = expr` is a value
        // binding; `type Name = …` is a first-class type — both via parse_def.)
        let is_alias = matches!(p.peek(), Tok::Ident(n) if n.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
            && *p.peek2() == Tok::Eq;
        if is_alias {
            let name = p.ident()?;
            p.expect(&Tok::Eq)?;
            let schema = p.parse_schema()?;
            // Record for same-file inlining (so the UNIT lowering sees the
            // `Quantity` directly), AND emit a real `Def::Type` so the alias is a
            // first-class type: carried across `.ys` imports and registered, so a
            // CROSS-FILE reference (`cell.ys`'s `mass :: Mass` for `Mass` from
            // `grow.ys`) resolves to its representation via the one lowering /
            // the type registry — instead of vanishing (it was neither inlined
            // across files nor carried as a def).
            p.aliases.insert(name.clone(), schema.clone());
            program.push(Def::Type(TypeDef {
                name,
                params: Vec::new(),
                representation: schema,
                methods: Vec::new(),
            }));
            continue;
        }
        program.push(p.parse_def()?);
    }
    Ok(program)
}

/// Attach `contract` to every output port that doesn't already declare one
/// (`fulfills C` = `:: C` on each output).
fn apply_fulfills(interface: &mut Interface, contract: &ContractRef) {
    for (_, decl) in interface.outputs.iter_mut() {
        if decl.contract.is_none() {
            decl.contract = Some(contract.clone());
        }
    }
}

/// Parse a standalone schema expression — the surface form used on the RHS of a
/// `type Name = …` declaration. Used to give a native imported type (declared via
/// a `ModuleRegistry`) its representation from a source string.
pub fn parse_schema_expr(src: &str) -> Result<SchemaExpr, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser {
        toks,
        pos: 0,
        aliases: Default::default(),
    };
    let schema = p.parse_schema()?;
    if !p.check(&Tok::Eof) {
        return Err(p.err("unexpected trailing tokens after schema expression"));
    }
    Ok(schema)
}

/// Parse a `.ys` file, **resolving `import Name from "rel"`** by loading the
/// referenced files (relative to each file's directory) and merging their
/// definitions. Imported `main` bindings and already-defined names are skipped;
/// import cycles are guarded. After resolution the `Program` has no
/// `Def::Import`. This is the file-driven form of the program-merge "import"
/// mechanism.
pub fn parse_file(path: impl AsRef<std::path::Path>) -> Result<Program, ParseError> {
    let mut visited = std::collections::HashSet::new();
    load_file(path.as_ref(), &mut visited)
}

fn load_file(
    path: &std::path::Path,
    visited: &mut std::collections::HashSet<std::path::PathBuf>,
) -> Result<Program, ParseError> {
    visited.insert(path.to_path_buf());
    let src = std::fs::read_to_string(path).map_err(|e| ParseError {
        message: format!("cannot read {}: {e}", path.display()),
        line: 0,
    })?;
    let raw = parse_program(&src)?;
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut program = Program::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for def in raw.defs {
        if let Def::Import { path: rel, .. } = &def {
            let imported_path = dir.join(rel);
            if visited.contains(&imported_path) {
                continue; // already loaded / cycle
            }
            let imported = load_file(&imported_path, visited)?;
            for d in imported.defs {
                let name = crate::ast::def_name(&d).to_string();
                if name == "main" || !seen.insert(name) {
                    continue; // skip the imported `main` and duplicate names
                }
                program.push(d);
            }
        } else {
            seen.insert(crate::ast::def_name(&def).to_string());
            program.push(def);
        }
    }
    Ok(program)
}

impl Parser {
    fn parse_def(&mut self) -> Result<Def, ParseError> {
        match self.peek() {
            Tok::Type => self.parse_type_def(),
            Tok::Def => self.parse_def_binding(),
            Tok::Process => self.parse_process_def(false),
            Tok::Step => self.parse_process_def(true),
            Tok::Composite => self.parse_composite_def(),
            Tok::Reaction => self.parse_reaction_def(),
            Tok::Unit => self.parse_unit_def(),
            Tok::Context => self.parse_context_def(),
            Tok::Contract => self.parse_contract_def(),
            Tok::Import => {
                self.bump();
                let name = self.ident()?;
                let from = self.ident()?; // `from` is a contextual keyword
                if from != "from" {
                    return Err(self.err(&format!("expected `from` in import, found `{from}`")));
                }
                let path = match self.bump() {
                    Tok::Str(s) => s,
                    other => {
                        return Err(self.err(&format!("expected a quoted path, found {other:?}")));
                    }
                };
                Ok(Def::Import { name, path })
            }
            // `from <module> import <name>, …` — native host imports (the
            // `extern` replacement). `from` is a contextual keyword, so guard on
            // it not being a `from = …` binding.
            Tok::Ident(s) if s == "from" && *self.peek2() != Tok::Eq => self.parse_use_def(),
            // `protocol Name = stream<Cell, path: '…'>` — a protocol-bound control.
            // `protocol` is CONTEXTUAL (a `{protocol: …}` field key stays an ident);
            // it's a def only when followed by the alias name (an ident).
            Tok::Ident(s) if s == "protocol" && matches!(self.peek2(), Tok::Ident(_)) => {
                self.parse_protocol_def()
            }
            // `name = expr` binding (e.g. `growth = 0.02`), OR a trailing bare
            // expression — the file's root VALUE, which becomes the implicit
            // `main` (so `Environment[…]` on the last line needs no `main =`).
            // A bare trailing expression is the implicit `main`. Named values
            // require `def` — so `name = …` / `name :: T = …` can't silently
            // shadow a control or type. Give a pointed error for the old form.
            _ => {
                if matches!(self.peek(), Tok::Ident(_))
                    && matches!(self.peek2(), Tok::Eq | Tok::ColonColon)
                {
                    let name = self.ident()?;
                    return Err(self.err(&format!(
                        "bare binding `{name}`: named values require `def` \u{2014} write `def {name} = …`"
                    )));
                }
                Ok(Def::Binding {
                    name: "main".into(),
                    schema: None,
                    value: self.parse_expr()?,
                })
            }
        }
    }

    // ── value definer ───────────────────────────────────────────────
    // `def name [:: Type] = expr` — a named value (the definer for bindings, so
    // values read like every other top-level form). Functions (`def name(args)
    // = body`) are a planned extension; today the value form covers shared data
    // like `def network :: CRN = {…}`.
    fn parse_def_binding(&mut self) -> Result<Def, ParseError> {
        self.expect(&Tok::Def)?;
        let name = self.ident()?;
        // Function form: `def name(params) [:: Ret] = body`. Params may be
        // untyped (`x`) or typed (`x: float`).
        if self.check(&Tok::LParen) {
            self.expect(&Tok::LParen)?;
            let mut params = Vec::new();
            while !self.check(&Tok::RParen) {
                let pname = self.ident()?;
                let schema = if self.accept_type_sep() {
                    self.parse_schema()?
                } else {
                    SchemaExpr::Any
                };
                params.push(Param {
                    name: pname,
                    schema,
                    default: None,
                });
                if !self.accept(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::RParen)?;
            if self.accept(&Tok::ColonColon) {
                let _ret = self.parse_schema()?; // return type: reserved, unused
            }
            self.expect(&Tok::Eq)?;
            let body = self.parse_expr()?;
            return Ok(Def::Function(crate::ast::FunctionDef {
                name,
                params,
                body,
            }));
        }
        // Value form: `def name [:: Type] = expr`.
        let schema = if self.accept(&Tok::ColonColon) {
            Some(self.parse_schema()?)
        } else {
            None
        };
        self.expect(&Tok::Eq)?;
        let value = self.parse_expr()?;
        Ok(Def::Binding {
            name,
            schema,
            value,
        })
    }

    // ── native host import ──────────────────────────────────────────
    // `from <module> import <name> (, <name>)*` — pull native processes /
    // functions into scope (replaces `extern`). `from`/the module/the names are
    // identifiers; `import` is the keyword token.
    /// A module path: `ident (("-" | ".") ident)*` — package names may contain `-`
    /// (`spatio-flux`) and submodules are dotted (`spatio-flux.composites`); the
    /// lexer splits these, so reassemble into one string.
    fn parse_module_path(&mut self) -> Result<String, ParseError> {
        let mut module = self.ident()?;
        loop {
            if self.accept(&Tok::Minus) {
                module.push('-');
                module.push_str(&self.ident()?);
            } else if self.accept(&Tok::Dot) {
                module.push('.');
                module.push_str(&self.ident()?);
            } else {
                break;
            }
        }
        Ok(module)
    }

    fn parse_use_def(&mut self) -> Result<Def, ParseError> {
        let kw = self.ident()?; // contextual `from`
        debug_assert_eq!(kw, "from");
        let module = self.parse_module_path()?;
        match self.bump() {
            Tok::Import => {}
            other => {
                return Err(self.err(&format!(
                    "expected `import` after `from {module}`, found {other:?}"
                )));
            }
        }
        let mut names = vec![self.ident()?];
        while self.accept(&Tok::Comma) {
            names.push(self.ident()?);
        }
        Ok(Def::Use { module, names })
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
        Ok(Def::Type(TypeDef {
            name,
            params: vec![],
            representation,
            methods,
        }))
    }

    // ── protocol-bound control ──────────────────────────────────────
    // `protocol Name = stream<Cell, path: '…'>` — bind a composite/process to a
    // transport + its typed address fields. The wrapped control is the (single)
    // positional arg; the named args are the protocol's address fields. Field
    // values are parsed at postfix level (not full comparison) so the closing `>`
    // is unambiguous (`<>` is the free bracket — see docs/protocols-as-types.md).
    fn parse_protocol_def(&mut self) -> Result<Def, ParseError> {
        self.bump(); // the contextual `protocol` ident
        let name = self.ident()?;
        self.expect(&Tok::Eq)?;
        let protocol = self.ident()?; // the transport / address-type tag
        self.expect(&Tok::Lt)?;
        let wrapped = self.ident()?; // the composite/process being addressed
        let mut fields = Vec::new();
        while self.accept(&Tok::Comma) {
            let field = self.field_name()?; // may be a keyword, e.g. `process:`
            self.expect(&Tok::Colon)?;
            let value = self.parse_postfix()?; // literal / var — stops before `>`
            fields.push((field, value));
        }
        self.expect(&Tok::Gt)?;
        Ok(Def::Protocol(crate::ast::ProtocolDef {
            name,
            protocol,
            wrapped,
            fields,
        }))
    }

    // ── contract declaration ────────────────────────────────────────
    // `contract Name (axis: value, …)` — a named process contract.
    fn parse_contract_def(&mut self) -> Result<Def, ParseError> {
        self.expect(&Tok::Contract)?;
        let name = self.ident()?;
        self.expect(&Tok::LParen)?;
        let mut axes: IndexMap<String, String> = IndexMap::new();
        while !self.check(&Tok::RParen) {
            let axis = self.ident()?;
            self.expect(&Tok::Colon)?;
            let value = self.ident()?;
            axes.insert(axis, value);
            if !self.accept(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RParen)?;
        Ok(Def::Contract(ContractDef { name, axes }))
    }

    // `Name` or `Name[axis: value, …]` — a contract reference (used by `::` on
    // a port and by `fulfills`); the optional `[…]` pins refine axes.
    fn parse_contract_ref(&mut self) -> Result<ContractRef, ParseError> {
        let name = self.ident()?;
        let mut cref = ContractRef::new(name);
        if self.accept(&Tok::LBrack) {
            while !self.check(&Tok::RBrack) {
                let axis = self.ident()?;
                self.expect(&Tok::Colon)?;
                let value = self.ident()?;
                cref = cref.pin(axis, value);
                if !self.accept(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::RBrack)?;
        }
        Ok(cref)
    }

    /// Parse an optional `fulfills C[…]` clause, returning the contract (applied
    /// to outputs once the interface is known — `fulfills` may sit before or
    /// after the `~{}->{}` interface).
    fn parse_optional_fulfills(&mut self) -> Result<Option<ContractRef>, ParseError> {
        if self.accept(&Tok::Fulfills) {
            Ok(Some(self.parse_contract_ref()?))
        } else {
            Ok(None)
        }
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
            self.expect_type_sep()?;
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
                    // Primitives accept either case (`bool` or `Bool`).
                    "any" | "Any" => Ok(SchemaExpr::Any),
                    "bool" | "Bool" => Ok(SchemaExpr::Bool),
                    "int" | "Int" => Ok(SchemaExpr::Int),
                    "float" | "Float" => Ok(SchemaExpr::Float),
                    "string" | "String" => Ok(SchemaExpr::String),
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
                    "overwrite" => {
                        self.expect(&Tok::LBrack)?;
                        let inner = self.parse_schema()?;
                        self.expect(&Tok::RBrack)?;
                        Ok(SchemaExpr::overwrite_of(inner))
                    }
                    // `array[[d1, d2, …], element]` — a fixed-shape numeric
                    // array (the additive field type; element may be dimensioned).
                    "array" => {
                        self.expect(&Tok::LBrack)?;
                        self.expect(&Tok::LBrack)?;
                        let mut shape = Vec::new();
                        while !self.check(&Tok::RBrack) {
                            match self.bump() {
                                Tok::Int(n) if n >= 0 => shape.push(n as usize),
                                other => {
                                    return Err(self.err(&format!(
                                        "array shape expects non-negative ints, found {other:?}"
                                    )));
                                }
                            }
                            if !self.accept(&Tok::Comma) {
                                break;
                            }
                        }
                        self.expect(&Tok::RBrack)?; // close shape
                        self.expect(&Tok::Comma)?;
                        let element = self.parse_schema()?;
                        self.expect(&Tok::RBrack)?; // close array
                        Ok(SchemaExpr::array(shape, element))
                    }
                    // `Quantity[unit: <unit-expr>, extensive?, affine?]` — a
                    // dimensioned scalar (the unit lives in the schema, erased
                    // after the dimensional check).
                    "Quantity" => {
                        self.expect(&Tok::LBrack)?;
                        // `unit:` label — `unit` is a keyword token here.
                        self.expect(&Tok::Unit)?;
                        self.expect(&Tok::Colon)?;
                        let unit = self.parse_unit_expr()?;
                        let (mut extensive, mut affine) = (false, false);
                        while self.accept(&Tok::Comma) {
                            match self.ident()?.as_str() {
                                "extensive" => extensive = true,
                                "affine" => affine = true,
                                other => {
                                    return Err(
                                        self.err(&format!("unknown Quantity flag `{other}`"))
                                    );
                                }
                            }
                        }
                        self.expect(&Tok::RBrack)?;
                        Ok(SchemaExpr::quantity(unit, extensive, affine))
                    }
                    // A type alias inlines to its schema (so units lowering
                    // sees the `Quantity`, not an opaque `Custom`).
                    _ if self.aliases.contains_key(&name) => Ok(self.aliases[&name].clone()),
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
        // Only `not` is boolean negation; `!` is the free-link literal (Unbound).
        if self.check(&Tok::Not) {
            self.bump();
            let operand = self.parse_not()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            });
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
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_unary()?;
            lhs = binop(op, lhs, rhs);
        }
        Ok(lhs)
    }

    // Unary minus (`-flux`), binding tighter than `*`/`/`. (Binary `a - b` is
    // handled in `parse_add`; a leading `-` here is negation.)
    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.check(&Tok::Minus) {
            self.bump();
            return Ok(Expr::neg(self.parse_unary()?));
        }
        self.parse_postfix()
    }

    // `primary ( "." (IDENT | "*") ( "(" args ")" )? )*` — field access (incl.
    // `*` wildcard path segments, e.g. `agents.*.mass`) + method calls.
    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut base = self.parse_primary()?;
        loop {
            if self.accept(&Tok::Dot) {
                // `*` is a wildcard path segment (a fan-out wire); else an identifier.
                let name = if self.accept(&Tok::Star) {
                    "*".to_string()
                } else {
                    self.ident()?
                };
                if self.check(&Tok::LParen) {
                    base = Expr::Method {
                        receiver: Box::new(base),
                        method: name,
                        args: self.parse_call_args()?,
                    };
                } else {
                    // Field access. A `Var`/`Path` base extends a *place path*
                    // (the wiring form, `var.seg.seg`); any other base is a
                    // *value* field access (`all[].config.bridge`) — so every
                    // value composes under `.field`, like `.method()` already
                    // does.
                    base = match base {
                        Expr::Var(v) => Expr::Path(PlacePath::local(v).dot(name)),
                        Expr::Path(p) => Expr::Path(p.dot(name)),
                        other => Expr::Field {
                            base: Box::new(other),
                            name,
                        },
                    };
                }
            } else if self.check(&Tok::LParen) {
                // bare call: `f(args)` — call a function value.
                base = Expr::Call {
                    func: Box::new(base),
                    args: self.parse_call_args()?,
                };
            } else {
                break;
            }
        }
        Ok(base)
    }

    /// Parse `( expr, … )` argument list (shared by method calls and `f(args)`).
    fn parse_call_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        self.expect(&Tok::LParen)?;
        let mut args = Vec::new();
        while !self.check(&Tok::RParen) {
            args.push(self.parse_expr()?);
            if !self.accept(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RParen)?;
        Ok(args)
    }

    /// Parse a (possibly interpolated) string: `'plain'` → one `Lit`;
    /// `'{expr}_suffix'` → `Expr`/`Lit` segments, each `{…}` parsed as a
    /// sub-expression (brace depth tracked for nested `{}`).
    fn parse_string_lit(&self, raw: &str) -> Result<StringLit, ParseError> {
        let chars: Vec<char> = raw.chars().collect();
        let mut segments = Vec::new();
        let mut lit = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '{' {
                if !lit.is_empty() {
                    segments.push(StringSeg::Lit(std::mem::take(&mut lit)));
                }
                let start = i + 1;
                let mut depth = 1;
                i += 1;
                while i < chars.len() && depth > 0 {
                    match chars[i] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                if depth != 0 {
                    return Err(self.err("unterminated `{` in string interpolation"));
                }
                let inner: String = chars[start..i].iter().collect();
                i += 1; // skip closing `}`
                let mut sub = Parser {
                    toks: lex(&inner)?,
                    pos: 0,
                    aliases: Default::default(),
                };
                segments.push(StringSeg::Expr(sub.parse_expr()?));
            } else {
                lit.push(chars[i]);
                i += 1;
            }
        }
        if !lit.is_empty() || segments.is_empty() {
            segments.push(StringSeg::Lit(lit));
        }
        Ok(StringLit::template(segments))
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
                Ok(Expr::Str(self.parse_string_lit(&s)?))
            }
            // `%` — the enclosing composite's own location (a place-graph
            // self-reference; used in output targets like `->{env: %}` and
            // self field access like `%.volume`). (`@` is now the bridge op.)
            Tok::Percent => {
                self.bump();
                Ok(Expr::Path(PlacePath::here()))
            }
            // `^` — the PARENT place (one level up the place graph). In a unit
            // EXPRESSION `^` is the power operator (parsed separately); here in
            // place/value position it is the parent reference, used in wire targets
            // like `~{glucose: ^.glucose}` — a cell reading its enclosing env's
            // pool (two levels up: cell → cells-map → env). Lowers to `[".."]`.
            Tok::Caret => {
                self.bump();
                Ok(Expr::Path(PlacePath::parent()))
            }
            // `replace <id> with <value>` — a structural rewrite directive.
            Tok::Replace => {
                self.bump();
                let id = self.parse_expr()?;
                self.expect(&Tok::With)?;
                let with = self.parse_expr()?;
                Ok(Expr::ReplaceWith {
                    id: Box::new(id),
                    with: Box::new(with),
                })
            }
            // `?name` (site) / `?name :: Sort` (typed site) / `?name.field…`
            // (a path rooted at the `?`-local). The name keeps its `?`.
            Tok::Question => {
                self.bump();
                let name = format!("?{}", self.ident()?);
                if self.accept(&Tok::ColonColon) {
                    Ok(Expr::site_typed(name, self.parse_postfix()?))
                } else if self.check(&Tok::Dot) {
                    // `?f.blueprint` — postfix extends this into a place path.
                    Ok(Expr::Path(PlacePath::local(name)))
                } else {
                    Ok(Expr::site(name))
                }
            }
            // `!` — the free-link (Unbound) literal.
            Tok::Bang => {
                self.bump();
                Ok(Expr::Unbound)
            }
            // `~name` — a link variable (in a port-target position).
            Tok::Tilde => {
                self.bump();
                Ok(Expr::LinkVar(self.ident()?))
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
                // A term call (instantiate a control): an identifier followed by
                // `[args]` / `~{}` / `->{}`, OR a CAPITALIZED identifier (the
                // convention: capitalized = controls, lowercase = vars/definers).
                let is_control = name.chars().next().is_some_and(|c| c.is_ascii_uppercase());
                if self.check(&Tok::LBrack)
                    || self.check(&Tok::Tilde)
                    || self.check(&Tok::Arrow)
                    || is_control
                {
                    self.parse_term_call(name)
                } else {
                    Ok(Expr::var(name))
                }
            }
            // `( … )` — grouping, OR a parenthesized parallel `(a | b)` /
            // entries `(k: v | …)` (a reaction redex/reactum, a term body).
            // The body grammar handles all three.
            Tok::LParen => self.parse_body(),
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

    // `{ key: expr, … }` — Record (all bare-ident keys) or Map (any string
    // key, which may be interpolated, e.g. `'{id}_0'`). Both evaluate to a
    // `Value::Map`; the Map form exists so keys can be computed.
    fn parse_braces(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Tok::LBrace)?;
        enum K {
            Id(String),
            S(String),
        }
        let mut entries: Vec<(K, Expr)> = Vec::new();
        let mut any_str = false;
        let mut first_entry = true;
        while !self.check(&Tok::RBrace) {
            let key = match self.bump() {
                Tok::Ident(k) => K::Id(k),
                Tok::Str(s) => {
                    any_str = true;
                    K::S(s)
                }
                other => return Err(self.err(&format!("expected a field key, found {other:?}"))),
            };
            self.expect(&Tok::Colon)?;
            let value = self.parse_expr()?;
            // `{ k: body for v in src (if p) }` — a MAP comprehension. Only valid
            // as the sole entry; the key may be computed (a string literal,
            // possibly interpolated) or a bare ident (e.g. `_divide`).
            if first_entry && self.accept(&Tok::For) {
                let key_expr = match key {
                    K::Id(s) => Expr::Str(StringLit::plain(s)),
                    K::S(s) => Expr::Str(self.parse_string_lit(&s)?),
                };
                let comp = self.parse_comprehension_tail(Some(key_expr), value)?;
                self.expect(&Tok::RBrace)?;
                return Ok(comp);
            }
            first_entry = false;
            entries.push((key, value));
            if !self.accept(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBrace)?;
        if any_str {
            // Map: keys are StringLits (string keys interpolated).
            let mut map = Vec::new();
            for (k, v) in entries {
                let key = match k {
                    K::Id(s) => StringLit::plain(s),
                    K::S(s) => self.parse_string_lit(&s)?,
                };
                map.push((key, v));
            }
            Ok(Expr::Map(map))
        } else {
            // Record: bare-ident keys.
            let rec: IndexMap<String, Expr> = entries
                .into_iter()
                .map(|(k, v)| match k {
                    K::Id(s) => (s, v),
                    K::S(_) => unreachable!(),
                })
                .collect();
            Ok(Expr::Record(rec))
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
            let comp = self.parse_comprehension_tail(None, first)?;
            self.expect(&Tok::RBrack)?;
            return Ok(comp);
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

    /// Parse the tail of a comprehension after `for` has been consumed:
    /// `v "in" src ("if" cond)?` or `kv, v "in" src ("if" cond)?`. `key` is
    /// `Some` for a map comprehension (`{ k: body for … }`), `None` for a list
    /// (`[ body for … ]`); `body` is the already-parsed body/value expression.
    fn parse_comprehension_tail(
        &mut self,
        key: Option<Expr>,
        body: Expr,
    ) -> Result<Expr, ParseError> {
        let first = self.ident()?;
        let (key_var, var) = if self.accept(&Tok::Comma) {
            (Some(first), self.ident()?) // `for kv, v in …`
        } else {
            (None, first) // `for v in …`
        };
        self.expect(&Tok::In)?;
        let source = self.parse_expr()?;
        let filter = if self.accept(&Tok::If) {
            Some(Box::new(self.parse_expr()?))
        } else {
            None
        };
        Ok(Expr::Comprehension {
            key_var,
            var,
            source: Box::new(source),
            filter,
            body: Box::new(body),
            key: key.map(Box::new),
        })
    }
}

fn binop(op: BinOp, lhs: Expr, rhs: Expr) -> Expr {
    Expr::BinOp {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

/// A bare subprocess term in a COMPOSITE body (`Diffusion ~{…}`, no slot key)
/// is auto-keyed by its control name (`diffusion: Diffusion ~{…}`) so it's
/// registered + runs — otherwise an unkeyed term parses but is silently never
/// instantiated. Applied only to composite bodies (NOT reaction redex/reactum
/// or term bodies, where bare terms are anonymous products/patterns).
fn auto_key_subprocesses(body: Expr) -> Expr {
    let key_term = |control: &str, used: &mut std::collections::HashSet<String>| -> String {
        let base = lower_first(control);
        let mut key = base.clone();
        let mut n = 2;
        while used.contains(&key) {
            key = format!("{base}_{n}");
            n += 1;
        }
        used.insert(key.clone());
        key
    };
    match body {
        Expr::Parallel(items) => {
            let mut used: std::collections::HashSet<String> = items
                .iter()
                .filter_map(|e| match e {
                    Expr::KeyedEntry { key, .. } => key.as_plain(),
                    _ => None,
                })
                .collect();
            let keyed = items
                .into_iter()
                .map(|e| match &e {
                    Expr::Term { control, .. } => {
                        let key = key_term(control, &mut used);
                        Expr::entry(key, e)
                    }
                    _ => e,
                })
                .collect();
            Expr::parallel(keyed)
        }
        // A lone bare subprocess body.
        Expr::Term { ref control, .. } => {
            let mut used = std::collections::HashSet::new();
            let key = key_term(control, &mut used);
            Expr::parallel(vec![Expr::entry(key, body)])
        }
        other => other,
    }
}

/// Lowercase the first character (`Synthesize` → `synthesize`, `RunBrs` → `runBrs`).
fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_ascii_lowercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

// ─────────────────────────────────────────────────────────────────────
// M2: process / step / composite / extern / reaction declarations
//     + the bigraph surface (`~{}->{}` interfaces, `|` bodies, term calls)
// ─────────────────────────────────────────────────────────────────────

use crate::ast::{Interface, Name, PortDecl, ProcessDef, ReactionDef, StepDef};

impl Parser {
    fn peek2(&self) -> &Tok {
        self.toks
            .get(self.pos + 1)
            .map(|s| &s.tok)
            .unwrap_or(&Tok::Eof)
    }

    /// `[ param ("," param)* ]` (config params). Empty if no `[`.
    fn parse_bracket_params(&mut self) -> Result<Vec<Param>, ParseError> {
        if !self.accept(&Tok::LBrack) {
            return Ok(vec![]);
        }
        let mut params = Vec::new();
        while !self.check(&Tok::RBrack) {
            let name = self.ident()?;
            self.expect_type_sep()?;
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
        self.expect(&Tok::RBrack)?;
        Ok(params)
    }

    /// `( "~" "{" port* "}" )? ( "->" "{" port* "}" )?` — a DECLARED interface,
    /// where each port is `name (":" schema)? ("=" default)?`.
    fn parse_interface(&mut self) -> Result<Interface, ParseError> {
        let mut iface = Interface::new();
        if self.accept(&Tok::Tilde) {
            for (name, decl) in self.parse_iface_ports()? {
                iface = iface.with_input(name, decl);
            }
        }
        if self.accept(&Tok::Arrow) {
            for (name, decl) in self.parse_iface_ports()? {
                iface = iface.with_output(name, decl);
            }
        }
        Ok(iface)
    }

    fn parse_iface_ports(&mut self) -> Result<Vec<(String, PortDecl)>, ParseError> {
        self.expect(&Tok::LBrace)?;
        let mut ports = Vec::new();
        while !self.check(&Tok::RBrace) {
            let name = self.ident()?;
            // `name` alone (no schema) → an untyped port (Any).
            let schema = if self.accept_type_sep() {
                self.parse_schema()?
            } else {
                SchemaExpr::Any
            };
            // Optional `@ internal.path` — the composite bridge target: the
            // inner state path this port maps to (absent ⇒ name-inferred).
            let bridge = if self.accept(&Tok::At) {
                Some(self.parse_dotted_path()?)
            } else {
                None
            };
            // Optional `fulfills Contract` (legacy `:: Contract`) — the
            // process-contract this port carries (output) or demands (input).
            // See docs/process-contracts.md.
            let contract = if self.accept_contract_sep() {
                Some(self.parse_contract_ref()?)
            } else {
                None
            };
            let mut decl = if self.accept(&Tok::Eq) {
                PortDecl::with_default(schema, self.parse_expr()?)
            } else {
                PortDecl::required(schema)
            };
            if let Some(c) = contract {
                decl = decl.with_contract(c);
            }
            if let Some(b) = bridge {
                decl = decl.with_bridge(b);
            }
            ports.push((name, decl));
            if !self.accept(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBrace)?;
        Ok(ports)
    }

    /// `ident ("." ident)*` — a dotted internal-state path (e.g. the bridge
    /// target `fields.values`), returned as its segments.
    fn parse_dotted_path(&mut self) -> Result<Vec<Name>, ParseError> {
        let mut segs = vec![self.ident()?];
        while self.accept(&Tok::Dot) {
            segs.push(self.ident()?);
        }
        Ok(segs)
    }

    /// A term call: `Control ( "[" args "]" )? ( "~{" inputs "}" )? ( "->{" outputs "}" )?`.
    /// Call-site ports wire a port name to a target EXPR (`mass: mass`); a bare
    /// `name` wires to `var(name)`.
    fn parse_term_call(&mut self, control: String) -> Result<Expr, ParseError> {
        let mut tb = Expr::term(control);
        if self.accept(&Tok::LBrack) {
            while !self.check(&Tok::RBrack) {
                if matches!(self.peek(), Tok::Ident(_)) && *self.peek2() == Tok::Colon {
                    let name = self.ident()?;
                    self.expect(&Tok::Colon)?;
                    tb = tb.arg_named(name, self.parse_expr()?);
                } else {
                    tb = tb.arg(self.parse_expr()?);
                }
                if !self.accept(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::RBrack)?;
        }
        if self.accept(&Tok::Tilde) {
            for (port, target) in self.parse_callsite_ports()? {
                tb = tb.input(port, target);
            }
        }
        if self.accept(&Tok::Arrow) {
            for (port, target) in self.parse_callsite_ports()? {
                tb = tb.output(port, target);
            }
        }
        let mut term = tb.build();
        // Optional term body: the `K[args](body)` kernel form — e.g.
        // `Compartment (enzyme: MEK | …)`.
        if self.check(&Tok::LParen) {
            let body = self.parse_body()?;
            if let Expr::Term { body: slot, .. } = &mut term {
                *slot = Some(Box::new(body));
            }
        }
        Ok(term)
    }

    fn parse_callsite_ports(&mut self) -> Result<Vec<(String, Expr)>, ParseError> {
        self.expect(&Tok::LBrace)?;
        let mut ports = Vec::new();
        while !self.check(&Tok::RBrace) {
            let name = self.ident()?;
            let target = if self.accept(&Tok::Colon) {
                self.parse_expr()?
            } else {
                Expr::var(name.clone()) // `~{mass}` shorthand → wired to `mass`
            };
            ports.push((name, target));
            if !self.accept(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RBrace)?;
        Ok(ports)
    }

    fn parse_process_def(&mut self, is_step: bool) -> Result<Def, ParseError> {
        self.bump(); // `process` | `step`
        let name = self.ident()?;
        let params = self.parse_bracket_params()?;
        // `fulfills C[…]` may appear before or after the `~{}->{}` interface.
        let fulfills_before = self.parse_optional_fulfills()?;
        let mut interface = self.parse_interface()?;
        let fulfills_after = self.parse_optional_fulfills()?;
        if let Some(c) = fulfills_before.or(fulfills_after) {
            apply_fulfills(&mut interface, &c);
        }
        let body = self.parse_body()?;
        Ok(if is_step {
            Def::Step(StepDef {
                name,
                params,
                interface,
                body,
            })
        } else {
            Def::Process(ProcessDef {
                name,
                params,
                interface,
                body,
            })
        })
    }

    fn parse_composite_def(&mut self) -> Result<Def, ParseError> {
        self.expect(&Tok::Composite)?;
        let name = self.ident()?;
        let params = self.parse_bracket_params()?;
        // `using Name(args)` clauses scope conversion contexts over the body.
        let mut using = Vec::new();
        while self.accept(&Tok::Using) {
            let ctx = self.ident()?;
            self.expect(&Tok::LParen)?;
            let args = self.parse_using_args()?;
            self.expect(&Tok::RParen)?;
            using.push(crate::ast::ContextUse { name: ctx, args });
        }
        // `fulfills C[…]` may sit before or after the interface (as for
        // processes) — a composite can be a contracted fulfiller too, e.g. a
        // rest-addressed remote engine queried via `fulfillers`.
        let fulfills_before = self.parse_optional_fulfills()?;
        let mut interface = self.parse_interface()?;
        let fulfills_after = self.parse_optional_fulfills()?;
        if let Some(c) = fulfills_before.or(fulfills_after) {
            apply_fulfills(&mut interface, &c);
        }
        // Auto-key any bare subprocess terms in the body (so they're registered).
        let body = auto_key_subprocesses(self.parse_body()?);
        Ok(Def::Composite(crate::ast::CompositeDef {
            name,
            params,
            interface,
            using,
            body,
        }))
    }

    fn parse_reaction_def(&mut self) -> Result<Def, ParseError> {
        self.expect(&Tok::Reaction)?;
        let name = self.ident()?;
        let params = self.parse_bracket_params()?;
        self.expect(&Tok::LParen)?;
        let redex = self.parse_expr()?;
        // optional `where <guard>` between redex and `=>`
        let guard = if self.accept(&Tok::Where) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.expect(&Tok::FatArrow)?;
        let reactum = self.parse_expr()?;
        self.expect(&Tok::RParen)?;
        Ok(Def::Reaction(ReactionDef {
            name,
            params,
            redex,
            reactum,
            guard,
            rate: None,
        }))
    }

    /// A body `( item ("|" item)* )`. An item is a keyed entry (`name: expr`),
    /// a binding (`name = expr`), or a bare value expr. All entries → a
    /// `Parallel` (a composite body); bindings + a final value → a `Block` (a
    /// process body); a lone value → that value.
    fn parse_body(&mut self) -> Result<Expr, ParseError> {
        self.expect(&Tok::LParen)?;
        enum Item {
            Entry(String, Expr),
            Bind(String, Expr),
            Value(Expr),
        }
        let mut items = Vec::new();
        if !self.check(&Tok::RParen) {
            loop {
                let item = if matches!(self.peek(), Tok::Ident(_)) && *self.peek2() == Tok::Colon {
                    let name = self.ident()?;
                    self.expect(&Tok::Colon)?;
                    Item::Entry(name, self.parse_expr()?)
                } else if matches!(self.peek(), Tok::Ident(_)) && *self.peek2() == Tok::Eq {
                    let name = self.ident()?;
                    self.expect(&Tok::Eq)?;
                    Item::Bind(name, self.parse_expr()?)
                } else {
                    Item::Value(self.parse_expr()?)
                };
                items.push(item);
                if !self.accept(&Tok::Bar) {
                    break;
                }
            }
        }
        self.expect(&Tok::RParen)?;

        let has_bind = items.iter().any(|i| matches!(i, Item::Bind(..)));
        let has_entry = items.iter().any(|i| matches!(i, Item::Entry(..)));
        if has_bind {
            // Block: bindings then a final value.
            let mut bindings = Vec::new();
            let mut value = Expr::Unit;
            for item in items {
                match item {
                    Item::Bind(n, e) => bindings.push((n, e)),
                    Item::Value(e) | Item::Entry(_, e) => value = e,
                }
            }
            Ok(Expr::Block(crate::ast::Block::from_parts(bindings, value)))
        } else if has_entry {
            // Parallel of entries (+ any bare values kept as-is).
            let elems = items
                .into_iter()
                .map(|i| match i {
                    Item::Entry(n, e) => Expr::entry(n, e),
                    Item::Value(e) => e,
                    Item::Bind(_, e) => e,
                })
                .collect();
            Ok(Expr::parallel(elems))
        } else if items.len() == 1 {
            Ok(match items.pop().unwrap() {
                Item::Value(e) | Item::Entry(_, e) | Item::Bind(_, e) => e,
            })
        } else {
            Ok(Expr::parallel(
                items
                    .into_iter()
                    .map(|i| match i {
                        Item::Value(e) | Item::Entry(_, e) | Item::Bind(_, e) => e,
                    })
                    .collect(),
            ))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// M3: units — `unit name : [dim] = def`, unit expressions, dimensions
//     (Quantity schemas are parsed in `parse_schema`). `context`/`using`
//     are deferred.
// ─────────────────────────────────────────────────────────────────────

use crate::ast::{Dimension, Ratio, UnitDef, UnitExpr};

impl Parser {
    /// `unit name : [dim] = <definition>`. The definition may lead with a
    /// magnitude (`1e-12 kg`) or be a plain unit expr (`um^3`, `mol / 6.02e23`,
    /// `1/s`).
    fn parse_unit_def(&mut self) -> Result<Def, ParseError> {
        self.expect(&Tok::Unit)?;
        let name = self.ident()?;
        self.expect(&Tok::Colon)?;
        let dimension = self.parse_dimension()?;
        self.expect(&Tok::Eq)?;
        let definition = if matches!(self.peek(), Tok::Int(_) | Tok::Float(_))
            && matches!(self.peek2(), Tok::Ident(_) | Tok::LParen)
        {
            // leading magnitude × unit (`1e-12 kg`)
            let mag = match self.bump() {
                Tok::Int(n) => n as f64,
                Tok::Float(f) => f,
                _ => unreachable!(),
            };
            UnitExpr::scalar(mag).mul(self.parse_unit_expr()?)
        } else {
            self.parse_unit_expr()?
        };
        Ok(Def::Unit(UnitDef {
            name,
            dimension,
            definition,
            affine_offset: None,
        }))
    }

    /// A dimension expression: `[base]` factors combined with `*` / `/`, each
    /// optionally `^ int` — `[mass]`, `[length]^3`, `[substance]/[length]^3`.
    /// Accumulated as (base, power) pairs (`/` negates the power) then built via
    /// `Dimension::of`.
    fn parse_dimension(&mut self) -> Result<Dimension, ParseError> {
        let mut powers: Vec<(String, i32)> = Vec::new();
        let (base, p) = self.parse_dim_term()?;
        powers.push((base, p));
        loop {
            if self.accept(&Tok::Star) {
                let (b, p) = self.parse_dim_term()?;
                powers.push((b, p));
            } else if self.accept(&Tok::Slash) {
                let (b, p) = self.parse_dim_term()?;
                powers.push((b, -p));
            } else {
                break;
            }
        }
        let refs: Vec<(&str, i32)> = powers.iter().map(|(b, p)| (b.as_str(), *p)).collect();
        Ok(Dimension::of(&refs))
    }

    /// One `[ base ] ( "^" int )?` dimension factor → (base, power).
    fn parse_dim_term(&mut self) -> Result<(String, i32), ParseError> {
        self.expect(&Tok::LBrack)?;
        let base = self.ident()?;
        self.expect(&Tok::RBrack)?;
        let power: i32 = if self.accept(&Tok::Caret) {
            match self.bump() {
                Tok::Int(n) => n as i32,
                other => {
                    return Err(
                        self.err(&format!("dimension power expects an int, found {other:?}"))
                    );
                }
            }
        } else {
            1
        };
        Ok((base, power))
    }

    /// Unit expression: `*` / `/` (left-assoc) over factors; a factor is a
    /// named unit, a scalar, or `( … )`, optionally `^ int`.
    fn parse_unit_expr(&mut self) -> Result<UnitExpr, ParseError> {
        let mut lhs = self.parse_unit_factor()?;
        loop {
            if self.accept(&Tok::Star) {
                lhs = lhs.mul(self.parse_unit_factor()?);
            } else if self.accept(&Tok::Slash) {
                lhs = lhs.div(self.parse_unit_factor()?);
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_unit_factor(&mut self) -> Result<UnitExpr, ParseError> {
        let base = match self.bump() {
            Tok::Ident(n) => UnitExpr::named(n),
            Tok::Int(n) => UnitExpr::scalar(n as f64),
            Tok::Float(f) => UnitExpr::scalar(f),
            Tok::LParen => {
                let e = self.parse_unit_expr()?;
                self.expect(&Tok::RParen)?;
                e
            }
            other => return Err(self.err(&format!("expected a unit, found {other:?}"))),
        };
        if self.accept(&Tok::Caret) {
            let exp = match self.bump() {
                Tok::Int(n) => n as i32,
                other => {
                    return Err(self.err(&format!("unit power expects an int, found {other:?}")));
                }
            };
            Ok(base.pow(Ratio::new(exp, 1)))
        } else {
            Ok(base)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// M3: contexts — `context Name(params) ( <dim> <-> <dim> : transform | … )`
//     and `using Name(args)` clauses on composites.
// ─────────────────────────────────────────────────────────────────────

use crate::ast::{ContextDef, ContextRule, TermArg};

impl Parser {
    /// `context Name ( params ) ( rule ("|" rule)* )` where a rule is
    /// `<dim> ("<->" | "->") <dim> : <transform>` (the transform may reference
    /// `value` + the context's params).
    fn parse_context_def(&mut self) -> Result<Def, ParseError> {
        self.expect(&Tok::Context)?;
        let name = self.ident()?;
        self.expect(&Tok::LParen)?;
        let params = self.parse_paren_params()?;
        self.expect(&Tok::RParen)?;
        self.expect(&Tok::LParen)?;
        let mut rules = Vec::new();
        while !self.check(&Tok::RParen) {
            let from = self.parse_dimension()?;
            let bidirectional = if self.accept(&Tok::BiArrow) {
                true
            } else if self.accept(&Tok::Arrow) {
                false
            } else {
                return Err(self.err("context rule expects `<->` or `->`"));
            };
            let to = self.parse_dimension()?;
            self.expect(&Tok::Colon)?;
            let transform = self.parse_expr()?;
            rules.push(ContextRule {
                from,
                to,
                bidirectional,
                transform,
            });
            if !self.accept(&Tok::Bar) {
                break;
            }
        }
        self.expect(&Tok::RParen)?;
        Ok(Def::Context(ContextDef {
            name,
            params,
            rules,
        }))
    }

    /// `param ("," param)*` inside parens — `IDENT ":" schema ("=" default)?`.
    fn parse_paren_params(&mut self) -> Result<Vec<Param>, ParseError> {
        let mut params = Vec::new();
        while !self.check(&Tok::RParen) {
            let name = self.ident()?;
            self.expect_type_sep()?;
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

    /// `( name: expr ("," name: expr)* )` — `using` args (named or positional).
    fn parse_using_args(&mut self) -> Result<Vec<TermArg>, ParseError> {
        let mut args = Vec::new();
        while !self.check(&Tok::RParen) {
            if matches!(self.peek(), Tok::Ident(_)) && *self.peek2() == Tok::Colon {
                let name = self.ident()?;
                self.expect(&Tok::Colon)?;
                args.push(TermArg::named(name, self.parse_expr()?));
            } else {
                args.push(TermArg::Positional(self.parse_expr()?));
            }
            if !self.accept(&Tok::Comma) {
                break;
            }
        }
        Ok(args)
    }
}
