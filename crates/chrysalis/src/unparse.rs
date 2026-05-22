//! Unparser: `AST → .ys` surface text — the inverse of [`crate::parse`].
//!
//! Two uses: (1) GENERATE `.ys` files from hand-built AST fixtures
//! (`fixtures::mapk`/`mr`); (2) a round-trip property — `parse(unparse(p))`
//! reproduces `p`, checked as a text fixpoint `unparse(parse(unparse(p))) ==
//! unparse(p)`, which validates the parser AND the unparser against each other.
//!
//! Output is deterministic (a fixed format) so the fixpoint is meaningful.

use crate::ast::{
    BinOp, Block, Def, Expr, Interface, PathRoot, PlacePath, PortBindings, PortDecl, Program,
    SchemaExpr, StringLit, StringSeg, TermArg, UnaryOp, UnitExpr,
};

/// Unparse a whole program: each definition, blank-line separated.
pub fn unparse(program: &Program) -> String {
    let mut out = String::new();
    for def in &program.defs {
        out.push_str(&unparse_def(def));
        out.push_str("\n\n");
    }
    out
}

fn unparse_def(def: &Def) -> String {
    match def {
        Def::Type(t) => {
            let mut s = format!("type {} = {}", t.name, unparse_schema(&t.representation));
            if !t.methods.is_empty() {
                s.push_str(" with {\n");
                for m in &t.methods {
                    s.push_str(&format!(
                        "  {}({}) = {}\n",
                        m.name,
                        unparse_params_inner(&m.params),
                        unparse_expr(&m.body)
                    ));
                }
                s.push('}');
            }
            s
        }
        Def::Process(p) => format!(
            "process {}{}{} {}",
            p.name,
            unparse_bracket_params(&p.params),
            unparse_interface(&p.interface),
            unparse_body(&p.body)
        ),
        Def::Step(p) => format!(
            "step {}{}{} {}",
            p.name,
            unparse_bracket_params(&p.params),
            unparse_interface(&p.interface),
            unparse_body(&p.body)
        ),
        Def::Composite(c) => {
            let using: String = c
                .using
                .iter()
                .map(|u| {
                    format!(" using {}({})", u.name, unparse_term_args(&u.args))
                })
                .collect();
            format!(
                "composite {}{}{}{} {}",
                c.name,
                unparse_bracket_params(&c.params),
                using,
                unparse_interface(&c.interface),
                unparse_body(&c.body)
            )
        }
        Def::Extern(e) => format!(
            "extern {}{}{}",
            e.name,
            unparse_bracket_params(&e.params),
            unparse_interface(&e.interface)
        ),
        Def::Reaction(r) => {
            let redex = match &r.guard {
                Some(g) => format!("{} where {}", unparse_expr(&r.redex), unparse_expr(g)),
                None => unparse_expr(&r.redex),
            };
            format!(
                "reaction {}{} (\n  {} => {}\n)",
                r.name,
                unparse_bracket_params(&r.params),
                redex,
                unparse_expr(&r.reactum)
            )
        }
        Def::Unit(u) => format!(
            "unit {} : {} = {}",
            u.name,
            unparse_dimension(&u.dimension),
            unparse_unit_expr(&u.definition)
        ),
        Def::Context(c) => {
            let rules: String = c
                .rules
                .iter()
                .map(|rule| {
                    format!(
                        "  {} {} {} : {}",
                        unparse_dimension(&rule.from),
                        if rule.bidirectional { "<->" } else { "->" },
                        unparse_dimension(&rule.to),
                        unparse_expr(&rule.transform)
                    )
                })
                .collect::<Vec<_>>()
                .join(" |\n");
            format!(
                "context {} ({}) (\n{}\n)",
                c.name,
                unparse_params_inner(&c.params),
                rules
            )
        }
        Def::Pattern(p) => format!("# pattern {} (unparse: TODO)", p.name),
        Def::Contract(c) => {
            let axes = c
                .axes
                .iter()
                .map(|(a, v)| format!("{a}: {v}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("contract {} ({axes})", c.name)
        }
        Def::Import { name, path } => format!("import {name} from '{path}'"),
        Def::Binding { name, value } => {
            if name == "main" {
                // The trailing-value form.
                unparse_expr(value)
            } else {
                format!("{name} = {}", unparse_expr(value))
            }
        }
    }
}

// ── parameters / interfaces ──────────────────────────────────────────

fn unparse_bracket_params(params: &[crate::ast::Param]) -> String {
    if params.is_empty() {
        String::new()
    } else {
        format!("[{}]", unparse_params_inner(params))
    }
}

fn unparse_params_inner(params: &[crate::ast::Param]) -> String {
    params
        .iter()
        .map(|p| {
            let base = format!("{}: {}", p.name, unparse_schema(&p.schema));
            match &p.default {
                Some(d) => format!("{base} = {}", unparse_expr(d)),
                None => base,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn unparse_interface(iface: &Interface) -> String {
    let port = |(name, decl): (&String, &PortDecl)| {
        let mut s = format!("{name}: {}", unparse_schema(&decl.schema));
        if let Some(d) = &decl.default {
            s.push_str(&format!(" = {}", unparse_expr(d)));
        }
        s
    };
    let mut s = String::new();
    if !iface.inputs.is_empty() {
        let ports: Vec<_> = iface.inputs.iter().map(port).collect();
        s.push_str(&format!(" ~{{{}}}", ports.join(", ")));
    }
    if !iface.outputs.is_empty() {
        let ports: Vec<_> = iface.outputs.iter().map(port).collect();
        s.push_str(&format!(" ->{{{}}}", ports.join(", ")));
    }
    s
}

// ── bodies ───────────────────────────────────────────────────────────

/// A declaration body `( … )`: Block → bindings + value; Parallel → entries;
/// anything else → a single value.
fn unparse_body(body: &Expr) -> String {
    match body {
        Expr::Block(Block { bindings, value }) => {
            let mut items: Vec<String> =
                bindings.iter().map(|(n, e)| format!("{n} = {}", unparse_expr(e))).collect();
            items.push(unparse_expr(value));
            format!("(\n  {}\n)", items.join(" |\n  "))
        }
        Expr::Parallel(items) => {
            let parts: Vec<String> = items.iter().map(unparse_expr).collect();
            format!("(\n  {}\n)", parts.join(" |\n  "))
        }
        other => format!("(\n  {}\n)", unparse_expr(other)),
    }
}

// ── expressions ──────────────────────────────────────────────────────

pub fn unparse_expr(e: &Expr) -> String {
    match e {
        Expr::Unit => "()".into(),
        Expr::Bool(b) => b.to_string(),
        Expr::Int(n) => n.to_string(),
        Expr::Float(f) => unparse_float(*f),
        Expr::Str(s) => unparse_string(s),
        Expr::Var(n) => n.clone(),
        Expr::Path(p) => unparse_path(p),
        Expr::Term { control, args, ports, body } => unparse_term(control, args, ports, body),
        Expr::Parallel(items) => {
            let parts: Vec<String> = items.iter().map(unparse_expr).collect();
            format!("({})", parts.join(" | "))
        }
        Expr::KeyedEntry { key, value } => format!("{}: {}", unparse_key(key), unparse_expr(value)),
        Expr::Map(entries) => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{}: {}", unparse_string(k), unparse_expr(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        Expr::Record(fields) => {
            let parts: Vec<String> =
                fields.iter().map(|(k, v)| format!("{k}: {}", unparse_expr(v))).collect();
            format!("{{{}}}", parts.join(", "))
        }
        Expr::List(items) => {
            let parts: Vec<String> = items.iter().map(unparse_expr).collect();
            format!("[{}]", parts.join(", "))
        }
        // The site `name` already carries its `?` prefix (e.g. `?f`).
        Expr::Site { name, sort } => match sort {
            Some(s) => format!("{name} :: {}", unparse_expr(s)),
            None => name.clone(),
        },
        Expr::Unbound => "!".into(),
        Expr::LinkVar(n) => format!("~{n}"),
        Expr::Rule { redex, reactum } => {
            format!("{} => {}", unparse_expr(redex), unparse_expr(reactum))
        }
        Expr::Let { bindings, body } => {
            let bs: Vec<String> =
                bindings.iter().map(|(n, e)| format!("{n} = {}", unparse_expr(e))).collect();
            format!("({} | {})", bs.join(" | "), unparse_expr(body))
        }
        Expr::Block(b) => {
            let mut items: Vec<String> =
                b.bindings.iter().map(|(n, e)| format!("{n} = {}", unparse_expr(e))).collect();
            items.push(unparse_expr(&b.value));
            format!("({})", items.join(" | "))
        }
        Expr::If { cond, then_, else_ } => {
            let mut s = format!("if {} then {}", unparse_expr(cond), unparse_expr(then_));
            if let Some(e) = else_ {
                s.push_str(&format!(" else {}", unparse_expr(e)));
            }
            s
        }
        Expr::BinOp { op, lhs, rhs } => {
            format!("({} {} {})", unparse_expr(lhs), unparse_binop(*op), unparse_expr(rhs))
        }
        Expr::UnaryOp { op, operand } => match op {
            UnaryOp::Neg => format!("-{}", unparse_expr(operand)),
            UnaryOp::Not => format!("not {}", unparse_expr(operand)),
        },
        Expr::Method { receiver, method, args } => {
            let a: Vec<String> = args.iter().map(unparse_expr).collect();
            format!("{}.{method}({})", unparse_expr(receiver), a.join(", "))
        }
        Expr::Comprehension { var, source, filter, body } => {
            let mut s = format!("[{} for {var} in {}", unparse_expr(body), unparse_expr(source));
            if let Some(f) = filter {
                s.push_str(&format!(" if {}", unparse_expr(f)));
            }
            s.push(']');
            s
        }
        Expr::ReplaceWith { id, with } => {
            format!("replace {} with {}", unparse_expr(id), unparse_expr(with))
        }
        Expr::Where { inner, predicate } => {
            format!("{} where {}", unparse_expr(inner), unparse_expr(predicate))
        }
    }
}

fn unparse_term(
    control: &str,
    args: &[TermArg],
    ports: &PortBindings,
    body: &Option<Box<Expr>>,
) -> String {
    let mut s = control.to_string();
    if !args.is_empty() {
        s.push_str(&format!("[{}]", unparse_term_args(args)));
    }
    if !ports.inputs.is_empty() {
        let ps: Vec<String> =
            ports.inputs.iter().map(|(p, t)| format!("{p}: {}", unparse_expr(t))).collect();
        s.push_str(&format!(" ~{{{}}}", ps.join(", ")));
    }
    if !ports.outputs.is_empty() {
        let ps: Vec<String> =
            ports.outputs.iter().map(|(p, t)| format!("{p}: {}", unparse_expr(t))).collect();
        s.push_str(&format!(" ->{{{}}}", ps.join(", ")));
    }
    if let Some(b) = body {
        s.push_str(&format!(" {}", inline_body(b)));
    }
    s
}

/// A term/reaction body rendered inline + parenthesized: `(a | b | c)`.
fn inline_body(b: &Expr) -> String {
    match b {
        Expr::Parallel(items) => {
            format!("({})", items.iter().map(unparse_expr).collect::<Vec<_>>().join(" | "))
        }
        Expr::Block(bl) => {
            let mut items: Vec<String> =
                bl.bindings.iter().map(|(n, e)| format!("{n} = {}", unparse_expr(e))).collect();
            items.push(unparse_expr(&bl.value));
            format!("({})", items.join(" | "))
        }
        other => format!("({})", unparse_expr(other)),
    }
}

fn unparse_term_args(args: &[TermArg]) -> String {
    args.iter()
        .map(|a| match a {
            TermArg::Positional(e) => unparse_expr(e),
            TermArg::Named { name, value } => format!("{name}: {}", unparse_expr(value)),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn unparse_path(p: &PlacePath) -> String {
    let mut s = match &p.root {
        PathRoot::Here => "@".to_string(),
        PathRoot::Parent => "^".to_string(),
        PathRoot::Local(n) => n.clone(),
    };
    for seg in &p.segments {
        s.push_str(&format!(".{seg}"));
    }
    s
}

fn unparse_key(k: &StringLit) -> String {
    // A keyed-entry key: a bare identifier if it's a plain single segment,
    // else a quoted/interpolated string.
    match k.as_plain() {
        Some(plain) if is_ident(&plain) => plain,
        _ => unparse_string(k),
    }
}

fn unparse_string(s: &StringLit) -> String {
    let mut out = String::from("'");
    for seg in &s.segments {
        match seg {
            StringSeg::Lit(t) => out.push_str(t),
            StringSeg::Expr(e) => out.push_str(&format!("{{{}}}", unparse_expr(e))),
        }
    }
    out.push('\'');
    out
}

fn unparse_binop(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::Concat => "++",
        BinOp::In => "in",
    }
}

// ── schemas / units / dimensions ─────────────────────────────────────

fn unparse_schema(s: &SchemaExpr) -> String {
    match s {
        SchemaExpr::Any => "any".into(),
        SchemaExpr::Bool => "bool".into(),
        SchemaExpr::Int => "int".into(),
        SchemaExpr::Float => "float".into(),
        SchemaExpr::String => "string".into(),
        SchemaExpr::Map(inner) => format!("map[{}]", unparse_schema(inner)),
        SchemaExpr::List(inner) => format!("list[{}]", unparse_schema(inner)),
        SchemaExpr::Record(fields) => {
            let parts: Vec<String> =
                fields.iter().map(|(k, v)| format!("{k}: {}", unparse_schema(v))).collect();
            format!("{{{}}}", parts.join(", "))
        }
        SchemaExpr::Custom { name, params } => {
            if params.is_empty() {
                name.clone()
            } else {
                let ps: Vec<String> = params.iter().map(unparse_schema).collect();
                format!("{name}[{}]", ps.join(", "))
            }
        }
        SchemaExpr::SelfType => "@".into(),
        SchemaExpr::Quantity { unit, extensive, affine } => {
            let mut s = format!("Quantity[unit: {}", unparse_unit_expr(unit));
            if *extensive {
                s.push_str(", extensive");
            }
            if *affine {
                s.push_str(", affine");
            }
            s.push(']');
            s
        }
        SchemaExpr::Array { shape, element } => {
            let dims: Vec<String> = shape.iter().map(|d| d.to_string()).collect();
            format!("array[[{}], {}]", dims.join(", "), unparse_schema(element))
        }
    }
}

fn unparse_unit_expr(u: &UnitExpr) -> String {
    match u {
        UnitExpr::Named(n) => n.clone(),
        UnitExpr::Scalar(f) => unparse_float(*f),
        UnitExpr::Mul(a, b) => format!("{} * {}", unparse_unit_expr(a), unparse_unit_expr(b)),
        UnitExpr::Div(a, b) => format!("{} / {}", unparse_unit_expr(a), unparse_unit_expr(b)),
        UnitExpr::Pow(a, exp) => format!("{}^{}", unparse_unit_expr(a), exp.num),
    }
}

fn unparse_dimension(d: &crate::ast::Dimension) -> String {
    // Numerator factors (`[a] * [b]^k`) then denominator factors (`/ [c]^k`),
    // so the first factor is always positive and re-parses (the parser reads
    // `^ <int>`, with `/` flipping the sign).
    if d.powers.is_empty() {
        return "[1]".into();
    }
    let factor = |base: &str, p: i32| {
        if p == 1 {
            format!("[{base}]")
        } else {
            format!("[{base}]^{p}")
        }
    };
    let mut s = String::new();
    let mut first = true;
    for (base, r) in d.powers.iter().filter(|(_, r)| r.num > 0) {
        let part = factor(base, r.num);
        if first {
            s.push_str(&part);
            first = false;
        } else {
            s.push_str(&format!(" * {part}"));
        }
    }
    if first {
        s.push_str("[1]");
    }
    for (base, r) in d.powers.iter().filter(|(_, r)| r.num < 0) {
        s.push_str(&format!(" / {}", factor(base, -r.num)));
    }
    s
}

fn unparse_float(f: f64) -> String {
    if !f.is_finite() {
        return format!("{f}");
    }
    let abs = f.abs();
    if f != 0.0 && (abs >= 1e15 || abs < 1e-4) {
        // Scientific, so large/small magnitudes re-parse as floats (not as a
        // giant integer that overflows) — e.g. `6.02214076e23`, `1e-12`.
        format!("{f:e}")
    } else if f == f.trunc() {
        format!("{f:.1}") // integer-valued → keep a decimal (`5.0`, not `5`)
    } else {
        format!("{f}")
    }
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
