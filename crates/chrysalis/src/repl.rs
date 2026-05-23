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
use prism_schema::{schema_to_value, MethodRegistry, Schema, Value};

use crate::ast::{def_name, Def, Expr, Name, Program};
use crate::eval::Evaluator;
use crate::parse::parse_program;

/// A REPL session: accumulated non-binding defs (the program the evaluator sees)
/// + the bound values (`def name = …`).
struct Session {
    defs: Vec<Def>,
    env: IndexMap<Name, Value>,
    methods: Arc<MethodRegistry>,
}

impl Session {
    fn new() -> Self {
        Self { defs: Vec::new(), env: IndexMap::new(), methods: Arc::new(crate::prelude::std_methods()) }
    }

    /// An evaluator over the current session program (functions / types resolve
    /// from `defs`; vars from the `env` passed at eval time).
    fn evaluator(&self) -> Evaluator {
        Evaluator::new(Arc::new(Program { defs: self.defs.clone() }), Arc::clone(&self.methods))
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
        self.evaluator().eval_value(&expr, &self.env).map_err(|e| e.to_string())
    }
}

/// Run the REPL (full line editing + history via `rustyline`) until EOF
/// (Ctrl-D) or `:quit`.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    use rustyline::error::ReadlineError;

    let mut session = Session::new();
    println!("chrysalis repl — `:help` for commands, `:quit` to exit");

    let mut rl = rustyline::DefaultEditor::new()?;
    let history = history_path();
    if let Some(path) = &history {
        let _ = rl.load_history(path); // best-effort
    }

    loop {
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
        Value::List(items) => format!("[{}]", items.iter().map(render).collect::<Vec<_>>().join(", ")),
        Value::Map(m) => format!(
            "{{{}}}",
            m.iter().map(|(k, v)| format!("{k}: {}", render(v))).collect::<Vec<_>>().join(", ")
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
        Def::Binding { .. } => "def",
        Def::Extern(_) => "extern",
        Def::Use { .. } | Def::Import { .. } => "import",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_binds_calls_and_inspects() {
        let mut s = Session::new();
        assert_eq!(s.eval_line("1 + 2"), vec!["=> 3 : int"]);
        assert_eq!(s.eval_line("def x = {a: 1.0, b: 2.0}"), vec!["x : map = {a: 1.0, b: 2.0}"]);
        assert_eq!(s.eval_line("x"), vec!["=> {a: 1.0, b: 2.0} : map"]);

        // First-class functions: define, then call.
        assert_eq!(s.eval_line("def double(n) = n * 2"), vec!["function double defined"]);
        assert_eq!(s.eval_line("double(21)"), vec!["=> 42 : int"]);

        // `:type` renders the inferred schema (homoiconic — the type is a value).
        let (ty, _) = s.command("type x");
        assert!(ty[0].contains("Tree"), "schema of x; got {ty:?}");

        // `:env` lists the binding + the function.
        let (env, _) = s.command("env");
        assert!(env.iter().any(|l| l.contains("x : map")), "env: {env:?}");
        assert!(env.iter().any(|l| l.contains("function double")), "env: {env:?}");

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
}
