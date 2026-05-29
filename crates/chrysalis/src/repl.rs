//! `chrysalis repl` — an interactive, homoiconic prompt.
//!
//! Read a line of `.ys`, evaluate it, print the result; `def`s (and `type` /
//! `process` / … definers) accumulate in the session. Because everything is a
//! value, the REPL is where homoiconicity pays off: build a value (record, list,
//! pattern, …) at the prompt, see its structure, bind it, reuse it.
//!
//! Commands: `:help`, `:env`, `:type <expr>`, `:reset`, `:quit`.

use std::path::PathBuf;
use std::sync::Arc;

use indexmap::IndexMap;
use prism_schema::{MethodRegistry, Schema, Value, schema_to_value};

use crate::ast::{Def, Expr, Name, Program, def_name};
use crate::eval::Evaluator;
use crate::parse::parse_program;

/// chrysalis keywords + definers + modifiers — highlighted, and offered for
/// completion. (Mirrors the emacs `ys-mode` face taxonomy.)
const KEYWORDS: &[&str] = &[
    "def",
    "type",
    "process",
    "step",
    "composite",
    "reaction",
    "pattern",
    "contract",
    "unit",
    "context",
    "from",
    "import",
    "fulfills",
    "using",
    "with",
    "where",
    "replace",
    "let",
    "in",
    "if",
    "then",
    "else",
    "for",
    "not",
    "and",
    "or",
    "true",
    "false",
];

/// A REPL session: accumulated non-binding defs (the program the evaluator sees)
/// + the bound values (`def name = …`).
struct Session {
    defs: Vec<Def>,
    env: IndexMap<Name, Value>,
    methods: Arc<MethodRegistry>,
}

impl Session {
    fn new() -> Self {
        Self {
            defs: Vec::new(),
            env: IndexMap::new(),
            methods: Arc::new(crate::prelude::std_methods()),
        }
    }

    /// An evaluator over the current session program (functions / types resolve
    /// from `defs`; vars from the `env` passed at eval time).
    fn evaluator(&self) -> Evaluator {
        Evaluator::new(
            Arc::new(Program {
                defs: self.defs.clone(),
            }),
            Arc::clone(&self.methods),
        )
    }

    /// Completable names: language keywords + the session's bindings and defs.
    fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = KEYWORDS.iter().map(|s| s.to_string()).collect();
        names.extend(self.env.keys().map(|k| k.to_string()));
        names.extend(self.defs.iter().map(|d| def_name(d).to_string()));
        names.sort();
        names.dedup();
        names
    }

    /// Evaluate one `.ys` line, returning the output lines: definers accumulate,
    /// bindings bind, a bare expression is evaluated and reported.
    fn eval_line(&mut self, input: &str) -> Vec<String> {
        let mut out = Vec::new();
        let prog = match parse_program(input) {
            Ok(p) => p,
            Err(e) => {
                out.push(format!("parse error: {e}"));
                return out;
            }
        };

        let mut bindings: Vec<(Name, Expr)> = Vec::new();
        let mut main_expr: Option<Expr> = None;
        for d in prog.defs {
            match d {
                Def::Binding { name, value, .. } if name == "main" => main_expr = Some(value),
                Def::Binding { name, value, .. } => bindings.push((name, value)),
                other => {
                    let kind = def_kind(&other);
                    let name = def_name(&other).to_string();
                    // Redefinition OVERWRITES: drop any prior def (and any shadowed
                    // binding) of the same name, rather than appending a dead one
                    // that `lookup` (first-wins) would keep using.
                    self.defs.retain(|d| def_name(d) != name.as_str());
                    self.env.shift_remove(name.as_str());
                    self.defs.push(other);
                    out.push(format!("{kind} {name} defined"));
                }
            }
        }

        let evaluator = self.evaluator();
        for (name, value) in bindings {
            match evaluator.eval_value(&value, &self.env) {
                Ok(v) => {
                    out.push(format!("{name} : {} = {}", type_name(&v), render(&v)));
                    // A binding shadows any prior definer of the same name; the
                    // env insert overwrites a prior binding.
                    self.defs.retain(|d| def_name(d) != name.as_str());
                    self.env.insert(name, v);
                }
                Err(e) => out.push(format!("error: {e}")),
            }
        }
        if let Some(expr) = main_expr {
            match evaluator.eval_value(&expr, &self.env) {
                Ok(v) => out.push(format!("=> {} : {}", render(&v), type_name(&v))),
                Err(e) => out.push(format!("error: {e}")),
            }
        }
        out
    }

    /// Handle a `:command`, returning `(output lines, quit?)`.
    fn command(&mut self, cmd: &str) -> (Vec<String>, bool) {
        let mut parts = cmd.trim().splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        let mut out = Vec::new();
        match name {
            "q" | "quit" | "exit" => return (out, true),
            "h" | "help" => out.extend(help_lines()),
            "env" => {
                if self.env.is_empty() && self.defs.is_empty() {
                    out.push("(empty session)".to_string());
                }
                for (k, v) in &self.env {
                    out.push(format!("  {k} : {} = {}", type_name(v), render(v)));
                }
                for d in &self.defs {
                    out.push(format!("  {} {}", def_kind(d), def_name(d)));
                }
            }
            "type" | "t" => {
                if rest.is_empty() {
                    out.push("usage: :type <expr>".to_string());
                } else {
                    match self.eval_expr_str(rest) {
                        Ok(v) => out.push(render(&schema_to_value(&Schema::infer(&v)))),
                        Err(e) => out.push(format!("error: {e}")),
                    }
                }
            }
            "reset" => {
                self.defs.clear();
                self.env.clear();
                out.push("(session reset)".to_string());
            }
            other => out.push(format!("unknown command `:{other}` — try :help")),
        }
        (out, false)
    }

    /// Parse + evaluate a bare expression string against the session.
    fn eval_expr_str(&self, s: &str) -> Result<Value, String> {
        let prog = parse_program(s).map_err(|e| e.to_string())?;
        let expr = match prog.lookup("main") {
            Some(Def::Binding { value, .. }) => value.clone(),
            _ => return Err("not an expression".to_string()),
        };
        self.evaluator()
            .eval_value(&expr, &self.env)
            .map_err(|e| e.to_string())
    }
}

/// Run the REPL (full line editing + history via `rustyline`) until EOF
/// (Ctrl-D) or `:quit`.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    use rustyline::error::ReadlineError;

    let mut session = Session::new();
    println!("chrysalis repl — `:help` for commands, `:quit` to exit");

    let mut rl: rustyline::Editor<ChrysalisHelper, rustyline::history::FileHistory> =
        rustyline::Editor::new()?;
    rl.set_helper(Some(ChrysalisHelper {
        names: session.names(),
        hinter: rustyline::hint::HistoryHinter::new(),
    }));
    let history = history_path();
    if let Some(path) = &history {
        let _ = rl.load_history(path); // best-effort
    }

    loop {
        // Refresh completion names from the live session.
        if let Some(h) = rl.helper_mut() {
            h.names = session.names();
        }
        match rl.readline("ys> ") {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(input);
                let (lines, quit) = if let Some(cmd) = input.strip_prefix(':') {
                    session.command(cmd)
                } else {
                    (session.eval_line(input), false)
                };
                for l in &lines {
                    println!("{l}");
                }
                if quit {
                    break;
                }
            }
            Err(ReadlineError::Interrupted) => continue, // Ctrl-C: abandon the line
            Err(ReadlineError::Eof) => break,            // Ctrl-D: exit
            Err(e) => {
                eprintln!("repl: {e}");
                break;
            }
        }
    }

    if let Some(path) = &history {
        let _ = rl.save_history(path); // best-effort
    }
    Ok(())
}

/// `$HOME/.chrysalis_history` for cross-session command history (best-effort).
fn history_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".chrysalis_history"))
}

// ── Rich line editing (#18): highlighting, completion, hints, multi-line. ──

use std::borrow::Cow;

use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hinter, HistoryHinter};
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Helper};

/// rustyline helper providing live syntax highlighting, name completion (session
/// bindings/defs + keywords), history hints, and multi-line continuation when a
/// line has unbalanced brackets (so a `process` body can span lines).
struct ChrysalisHelper {
    names: Vec<String>,
    hinter: HistoryHinter,
}

impl Completer for ChrysalisHelper {
    type Candidate = Pair;
    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let start = line[..pos]
            .rfind(|c: char| !c.is_alphanumeric() && c != '_')
            .map_or(0, |i| i + 1);
        let word = &line[start..pos];
        if word.is_empty() {
            return Ok((start, Vec::new()));
        }
        let candidates = self
            .names
            .iter()
            .filter(|n| n.starts_with(word))
            .map(|n| Pair {
                display: n.clone(),
                replacement: n.clone(),
            })
            .collect();
        Ok((start, candidates))
    }
}

impl Hinter for ChrysalisHelper {
    type Hint = String;
    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<String> {
        self.hinter.hint(line, pos, ctx)
    }
}

impl Highlighter for ChrysalisHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        Cow::Owned(highlight_ys(line))
    }
    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        true // re-highlight on every keystroke → live coloring
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[90m{hint}\x1b[0m")) // dim ghost text
    }
}

impl Validator for ChrysalisHelper {
    fn validate(&self, ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        if unbalanced(ctx.input()) {
            Ok(ValidationResult::Incomplete) // open bracket — keep reading
        } else {
            Ok(ValidationResult::Valid(None))
        }
    }
}

impl Helper for ChrysalisHelper {}

/// Color a `.ys` line with ANSI escapes by token kind: keywords (magenta),
/// Capitalised controls/types (cyan), strings (green), numbers (yellow),
/// comments (grey), `?`/`~` variables (red).
fn highlight_ys(line: &str) -> String {
    const RESET: &str = "\x1b[0m";
    const KW: &str = "\x1b[35m";
    const CTRL: &str = "\x1b[36m";
    const STR: &str = "\x1b[32m";
    const NUM: &str = "\x1b[33m";
    const COMMENT: &str = "\x1b[90m";
    const VAR: &str = "\x1b[31m";

    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '#' {
            out.push_str(COMMENT);
            out.extend(&chars[i..]);
            out.push_str(RESET);
            break;
        } else if c == '\'' {
            out.push_str(STR);
            out.push(c);
            i += 1;
            while i < chars.len() {
                out.push(chars[i]);
                let closed = chars[i] == '\'';
                i += 1;
                if closed {
                    break;
                }
            }
            out.push_str(RESET);
        } else if (c == '?' || c == '~')
            && chars
                .get(i + 1)
                .is_some_and(|n| n.is_alphabetic() || *n == '_')
        {
            out.push_str(VAR);
            out.push(c);
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                out.push(chars[i]);
                i += 1;
            }
            out.push_str(RESET);
        } else if c.is_ascii_digit() {
            out.push_str(NUM);
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                out.push(chars[i]);
                i += 1;
            }
            out.push_str(RESET);
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if KEYWORDS.contains(&word.as_str()) {
                out.push_str(KW);
                out.push_str(&word);
                out.push_str(RESET);
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                out.push_str(CTRL);
                out.push_str(&word);
                out.push_str(RESET);
            } else {
                out.push_str(&word);
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// Are brackets unbalanced (more opens than closes, ignoring strings/comments)?
/// Drives multi-line continuation for definitions that span lines.
fn unbalanced(input: &str) -> bool {
    let mut depth: i32 = 0;
    for line in input.lines() {
        let mut in_str = false;
        for c in line.chars() {
            if in_str {
                if c == '\'' {
                    in_str = false;
                }
                continue;
            }
            match c {
                '\'' => in_str = true,
                '#' => break,
                '(' | '{' | '[' => depth += 1,
                ')' | '}' | ']' => depth -= 1,
                _ => {}
            }
        }
    }
    depth > 0
}

fn help_lines() -> Vec<String> {
    [
        "  <expr>             evaluate and print a value (with its type)",
        "  def name = <expr>  bind a value for reuse",
        "  def f(x) = <expr>  define a function; type/process/composite/… also accumulate",
        "  :type <expr>       show the inferred schema of an expression",
        "  :env               list the session's bindings and definitions",
        "  :reset             clear the session",
        "  :help   :quit",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Render a [`Value`] in chrysalis surface form (`{k: v}`, `[a, b]`, `'s'`, …).
fn render(v: &Value) -> String {
    match v {
        Value::None => "()".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            let x = f.into_inner();
            if x.fract() == 0.0 {
                format!("{x:.1}")
            } else {
                format!("{x}")
            }
        }
        Value::String(s) => format!("'{s}'"),
        Value::List(items) => format!(
            "[{}]",
            items.iter().map(render).collect::<Vec<_>>().join(", ")
        ),
        Value::Map(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{k}: {}", render(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("{other:?}"),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::None => "unit",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::String(_) => "string",
        Value::List(_) => "list",
        Value::Map(_) => "map",
        Value::Struct { .. } => "struct",
        Value::Foreign(_) => "foreign",
        Value::Bytes(_) => "bytes",
    }
}

fn def_kind(d: &Def) -> &'static str {
    match d {
        Def::Process(_) => "process",
        Def::Step(_) => "step",
        Def::Composite(_) => "composite",
        Def::Type(_) => "type",
        Def::Contract(_) => "contract",
        Def::Reaction(_) => "reaction",
        Def::Pattern(_) => "pattern",
        Def::Unit(_) => "unit",
        Def::Context(_) => "context",
        Def::Function(_) => "function",
        Def::Protocol(_) => "protocol",
        Def::Binding { .. } => "def",
        Def::Use { .. } => "import",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_binds_calls_and_inspects() {
        let mut s = Session::new();
        assert_eq!(s.eval_line("1 + 2"), vec!["=> 3 : int"]);
        assert_eq!(
            s.eval_line("def x = {a: 1.0, b: 2.0}"),
            vec!["x : map = {a: 1.0, b: 2.0}"]
        );
        assert_eq!(s.eval_line("x"), vec!["=> {a: 1.0, b: 2.0} : map"]);

        // First-class functions: define, then call.
        assert_eq!(
            s.eval_line("def double(n) = n * 2"),
            vec!["function double defined"]
        );
        assert_eq!(s.eval_line("double(21)"), vec!["=> 42 : int"]);

        // `:type` renders the inferred schema (homoiconic — the type is a value).
        let (ty, _) = s.command("type x");
        assert!(ty[0].contains("Tree"), "schema of x; got {ty:?}");

        // `:env` lists the binding + the function.
        let (env, _) = s.command("env");
        assert!(env.iter().any(|l| l.contains("x : map")), "env: {env:?}");
        assert!(
            env.iter().any(|l| l.contains("function double")),
            "env: {env:?}"
        );

        assert!(s.command("quit").1, ":quit signals exit");
    }

    #[test]
    fn errors_do_not_kill_the_session() {
        let mut s = Session::new();
        let out = s.eval_line("1 +");
        assert!(out[0].starts_with("parse error"), "got {out:?}");
        // Session still usable after an error.
        assert_eq!(s.eval_line("2 + 2"), vec!["=> 4 : int"]);
    }

    #[test]
    fn highlights_keywords_controls_and_strings() {
        let h = highlight_ys("def x = Cell['a']");
        assert!(h.contains("\x1b[35mdef\x1b[0m"), "keyword magenta: {h:?}");
        assert!(h.contains("\x1b[36mCell\x1b[0m"), "control cyan: {h:?}");
        assert!(h.contains("\x1b[32m'a'\x1b[0m"), "string green: {h:?}");
    }

    #[test]
    fn redefinition_overwrites_instead_of_shadowing() {
        let mut s = Session::new();
        assert_eq!(s.eval_line("def f(x) = x + 1"), vec!["function f defined"]);
        assert_eq!(s.eval_line("f(10)"), vec!["=> 11 : int"]);

        // Correct a mistake: the redefinition must WIN, not be dropped.
        assert_eq!(
            s.eval_line("def f(x) = x * 100"),
            vec!["function f defined"]
        );
        assert_eq!(
            s.eval_line("f(10)"),
            vec!["=> 1000 : int"],
            "the redefinition overwrites the old one"
        );
        assert_eq!(
            s.defs.iter().filter(|d| def_name(d) == "f").count(),
            1,
            "only one `f` definition remains (no dead duplicate)"
        );
    }

    #[test]
    fn multiline_continues_only_on_open_brackets() {
        assert!(unbalanced("process Foo ("), "open paren → keep reading");
        assert!(unbalanced("a = {b: ["), "nested opens");
        assert!(!unbalanced("def x = 5"), "balanced");
        assert!(!unbalanced("f(5)"), "balanced call");
        assert!(
            !unbalanced("s = 'a ( in a string'"),
            "paren in string ignored"
        );
        assert!(
            !unbalanced("x = 1 # ( in a comment"),
            "paren in comment ignored"
        );
    }
}
