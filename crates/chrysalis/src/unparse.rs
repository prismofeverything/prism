//! Unparser: `AST → .ys` surface text — the inverse of [`crate::parse`].
//!
//! Two uses: (1) GENERATE `.ys` files from hand-built AST fixtures
//! (`fixtures::mapk`/`mr`); (2) a round-trip property — `parse(unparse(p))`
//! reproduces `p`, checked as a text fixpoint `unparse(parse(unparse(p))) ==
//! unparse(p)`, which validates the parser AND the unparser against each other.
//!
//! Output is deterministic (a fixed format) so the fixpoint is meaningful.

use crate::ast::{
    BinOp, Block, ContractRef, Def, Expr, Interface, PathRoot, PlacePath, PortBindings, PortDecl,
    Program, SchemaExpr, StringLit, StringSeg, TermArg, UnaryOp, UnitExpr,
};

/// Unparse a whole program: each definition blank-line separated, except
/// consecutive imports (`from … import …` / `import … from …`), which group
/// together with a single newline.
pub fn unparse(program: &Program) -> String {
    let is_import = |d: &Def| matches!(d, Def::Use { .. } | Def::Import { .. });
    let mut out = String::new();
    for (i, def) in program.defs.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            // Blank line between defs, unless this and the previous are imports.
            if !(is_import(def) && is_import(&program.defs[i - 1])) {
                out.push('\n');
            }
        }
        out.push_str(&unparse_def(def));
    }
    out.push('\n');
    out
}

fn unparse_def(def: &Def) -> String {
    match def {
        Def::Type(t) => {
            let mut s = format!("type {} = {}", t.name, unparse_type_repr(&t.representation));
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
        Def::Process(p) => unparse_definer("process", &p.name, &p.params, &p.interface, &p.body),
        Def::Step(p) => unparse_definer("step", &p.name, &p.params, &p.interface, &p.body),
        Def::Function(f) => {
            format!(
                "def {}({}) = {}",
                f.name,
                unparse_fn_params(&f.params),
                unparse_expr(&f.body)
            )
        }
        Def::Composite(c) => {
            let using: String = c
                .using
                .iter()
                .map(|u| format!(" using {}({})", u.name, unparse_term_args(&u.args)))
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
            if c.axes.len() > 1 {
                let axes = c
                    .axes
                    .iter()
                    .map(|(a, v)| format!("  {a}: {v}"))
                    .collect::<Vec<_>>()
                    .join(",\n");
                format!("contract {} (\n{axes}\n)", c.name)
            } else {
                let axes = c
                    .axes
                    .iter()
                    .map(|(a, v)| format!("{a}: {v}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("contract {} ({axes})", c.name)
            }
        }
        Def::Import { name, path } => format!("import {name} from '{path}'"),
        Def::Use { module, names } => format!("from {module} import {}", names.join(", ")),
        Def::Protocol(p) => {
            let fields = p
                .fields
                .iter()
                .map(|(f, v)| format!("{f}: {}", unparse_expr(v)))
                .collect::<Vec<_>>()
                .join(", ");
            if fields.is_empty() {
                format!("protocol {} = {}<{}>", p.name, p.protocol, p.wrapped)
            } else {
                format!("protocol {} = {}<{}, {fields}>", p.name, p.protocol, p.wrapped)
            }
        }
        Def::Binding {
            name,
            schema,
            value,
        } => {
            if name == "main" {
                // The trailing-value form.
                unparse_expr(value)
            } else if let Some(s) = schema {
                format!(
                    "def {name} :: {} = {}",
                    unparse_schema(s),
                    unparse_expr(value)
                )
            } else {
                format!("def {name} = {}", unparse_expr(value))
            }
        }
    }
}

// ── process / step definer (canonical multi-line) ────────────────────

/// A `process`/`step` definer. When all outputs share one contract it hoists to
/// a `fulfills C` clause and the header goes multi-line (fulfills + interface on
/// their own lines); otherwise the header stays single-line. A simple-expression
/// body renders inline `( e )`; a record / parallel / block body multi-line.
fn unparse_definer(
    kw: &str,
    name: &str,
    params: &[crate::ast::Param],
    interface: &Interface,
    body: &Expr,
) -> String {
    match common_output_contract(interface) {
        Some(c) => format!(
            "{kw} {name}{}\n  fulfills {}\n  {}\n  {}",
            unparse_bracket_params(params),
            unparse_contract_ref(&c),
            unparse_interface_inner(interface, Some(&c)).trim_start(),
            unparse_body(body)
        ),
        None => format!(
            "{kw} {name}{}{} {}",
            unparse_bracket_params(params),
            unparse_interface_inner(interface, None),
            unparse_body(body)
        ),
    }
}

/// If every output port carries the *same* contract, return it (so it hoists to
/// one `fulfills C` clause). Otherwise `None` (contracts stay per-port `:: C`).
fn common_output_contract(iface: &Interface) -> Option<ContractRef> {
    let mut outputs = iface.outputs.values();
    let first = outputs.next()?.contract.clone()?;
    for decl in outputs {
        if decl.contract.as_ref() != Some(&first) {
            return None;
        }
    }
    Some(first)
}

fn unparse_contract_ref(c: &ContractRef) -> String {
    if c.pins.is_empty() {
        c.name.clone()
    } else {
        let pins: Vec<String> = c.pins.iter().map(|(a, v)| format!("{a}: {v}")).collect();
        format!("{}[{}]", c.name, pins.join(", "))
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

/// Function parameters: `name` for an untyped (`any`) param, `name: schema`
/// otherwise — so `def f(x) = …` round-trips without gaining a `: any`.
fn unparse_fn_params(params: &[crate::ast::Param]) -> String {
    params
        .iter()
        .map(|p| {
            if matches!(p.schema, SchemaExpr::Any) {
                p.name.clone()
            } else {
                format!("{} :: {}", p.name, unparse_schema(&p.schema))
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn unparse_interface(iface: &Interface) -> String {
    unparse_interface_inner(iface, None)
}

/// Render `~{…} ->{…}`. Port types use `::` (`name :: Schema`); a port's contract
/// is emitted as `fulfills C` unless it equals `hoisted` (already raised to a
/// `fulfills C` clause on the header by the caller).
fn unparse_interface_inner(iface: &Interface, hoisted: Option<&ContractRef>) -> String {
    // Emit in the order the parser reads: schema, `@ bridge`, `fulfills C`,
    // `= default`. (Contract before default matters — `fulfills` is not a valid
    // expr continuation, so `= d fulfills C` would not re-parse.)
    let port = |(name, decl): (&String, &PortDecl)| {
        let mut s = format!("{name} :: {}", unparse_schema(&decl.schema));
        if let Some(b) = &decl.bridge {
            s.push_str(&format!(" @ {}", b.join(".")));
        }
        if let Some(c) = &decl.contract {
            if hoisted != Some(c) {
                s.push_str(&format!(" fulfills {}", unparse_contract_ref(c)));
            }
        }
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

/// A declaration body `( … )`. Block/Parallel (multiple statements) and
/// record/map literals render multi-line; a simple expression renders inline
/// `( e )` (e.g. a one-liner `( rk4.integrate(network, state, interval) )`).
fn unparse_body(body: &Expr) -> String {
    match body {
        Expr::Block(Block { bindings, value }) => {
            let mut items: Vec<String> = bindings
                .iter()
                .map(|(n, e)| format!("{n} = {}", unparse_expr(e)))
                .collect();
            items.push(unparse_expr(value));
            format!("(\n  {}\n)", items.join(" |\n  "))
        }
        Expr::Parallel(items) if !items.is_empty() => {
            let parts: Vec<String> = items.iter().map(unparse_expr).collect();
            format!("(\n  {}\n)", parts.join(" |\n  "))
        }
        Expr::Record(_) | Expr::Map(_) => format!("(\n  {}\n)", unparse_expr(body)),
        other => format!("( {} )", unparse_expr(other)),
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
        Expr::Term {
            control,
            args,
            ports,
            body,
        } => unparse_term(control, args, ports, body),
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
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", unparse_expr(v)))
                .collect();
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
            let bs: Vec<String> = bindings
                .iter()
                .map(|(n, e)| format!("{n} = {}", unparse_expr(e)))
                .collect();
            format!("({} | {})", bs.join(" | "), unparse_expr(body))
        }
        Expr::Block(b) => {
            let mut items: Vec<String> = b
                .bindings
                .iter()
                .map(|(n, e)| format!("{n} = {}", unparse_expr(e)))
                .collect();
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
            format!(
                "({} {} {})",
                unparse_expr(lhs),
                unparse_binop(*op),
                unparse_expr(rhs)
            )
        }
        Expr::UnaryOp { op, operand } => match op {
            UnaryOp::Neg => format!("-{}", unparse_expr(operand)),
            UnaryOp::Not => format!("not {}", unparse_expr(operand)),
        },
        Expr::Method {
            receiver,
            method,
            args,
        } => {
            let a: Vec<String> = args.iter().map(unparse_arg).collect();
            format!("{}.{method}({})", unparse_expr(receiver), a.join(", "))
        }
        Expr::Field { base, name } => format!("{}.{name}", unparse_expr(base)),
        Expr::Call { func, args } => {
            let a: Vec<String> = args.iter().map(unparse_arg).collect();
            format!("{}({})", unparse_expr(func), a.join(", "))
        }
        Expr::Comprehension {
            key_var,
            var,
            source,
            filter,
            body,
            key,
        } => {
            // `for kv, v in …` when the key/index is bound.
            let binder = match key_var {
                Some(kv) => format!("{kv}, {var}"),
                None => var.clone(),
            };
            let mut s = match key {
                // Map comprehension: `{ k: body for … }`.
                Some(k) => format!("{{{}: {} for {binder} in {}", unparse_expr(k), unparse_expr(body), unparse_expr(source)),
                // List comprehension: `[ body for … ]`.
                None => format!("[{} for {binder} in {}", unparse_expr(body), unparse_expr(source)),
            };
            if let Some(f) = filter {
                s.push_str(&format!(" if {}", unparse_expr(f)));
            }
            s.push(if key.is_some() { '}' } else { ']' });
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
        let ps: Vec<String> = ports
            .inputs
            .iter()
            .map(|(p, t)| format!("{p}: {}", unparse_expr(t)))
            .collect();
        s.push_str(&format!(" ~{{{}}}", ps.join(", ")));
    }
    if !ports.outputs.is_empty() {
        let ps: Vec<String> = ports
            .outputs
            .iter()
            .map(|(p, t)| format!("{p}: {}", unparse_expr(t)))
            .collect();
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
            format!(
                "({})",
                items
                    .iter()
                    .map(unparse_expr)
                    .collect::<Vec<_>>()
                    .join(" | ")
            )
        }
        Expr::Block(bl) => {
            let mut items: Vec<String> = bl
                .bindings
                .iter()
                .map(|(n, e)| format!("{n} = {}", unparse_expr(e)))
                .collect();
            items.push(unparse_expr(&bl.value));
            format!("({})", items.join(" | "))
        }
        other => format!("({})", unparse_expr(other)),
    }
}

fn unparse_term_args(args: &[TermArg]) -> String {
    args.iter()
        .map(|a| match a {
            TermArg::Positional(e) => unparse_arg(e),
            TermArg::Named { name, value } => format!("{name}: {}", unparse_arg(value)),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Unparse an expression in an argument position (method/term arg). The
/// surrounding `( )` already delimits it, so a top-level binary op needn't be
/// re-parenthesized: `f(a / b)`, not `f((a / b))`. Nested operands keep their
/// own parens via `unparse_expr`.
fn unparse_arg(e: &Expr) -> String {
    match e {
        Expr::BinOp { op, lhs, rhs } => {
            format!(
                "{} {} {}",
                unparse_expr(lhs),
                unparse_binop(*op),
                unparse_expr(rhs)
            )
        }
        other => unparse_expr(other),
    }
}

fn unparse_path(p: &PlacePath) -> String {
    let mut s = match &p.root {
        PathRoot::Here => "%".to_string(),
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

/// A `type Name = …` representation: a record with more than one field renders
/// multi-line (one field per line); everything else stays inline.
fn unparse_type_repr(s: &SchemaExpr) -> String {
    match s {
        SchemaExpr::Record(fields) if fields.len() > 1 => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("  {k}: {}", unparse_schema(v)))
                .collect();
            format!("{{\n{}\n}}", parts.join(",\n"))
        }
        other => unparse_schema(other),
    }
}

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
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", unparse_schema(v)))
                .collect();
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
        SchemaExpr::SelfType => "%".into(),
        SchemaExpr::Quantity {
            unit,
            extensive,
            affine,
        } => {
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
        SchemaExpr::Overwrite(inner) => format!("overwrite[{}]", unparse_schema(inner)),
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
