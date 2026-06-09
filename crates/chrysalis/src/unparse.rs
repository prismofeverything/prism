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
/// consecutive imports (`from … import …`), which group together with a single
/// newline.
pub fn unparse(program: &Program) -> String {
    let is_import = |d: &Def| matches!(d, Def::Use { .. });
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
            let prefix = format!("composite {}{}{using}", c.name, unparse_bracket_params(&c.params));
            format!(
                "{prefix}{} {}",
                unparse_interface(&c.interface, prefix.len()),
                unparse_body(&c.body)
            )
        }
        Def::Reaction(r) => {
            // The rule body lives at indent 2 (inside `reaction NAME (\n  …`).
            // Render redex/reactum with that indent so they break when long,
            // pipes-at-end-of-line matching composite bodies.
            let redex = match &r.guard {
                Some(g) => format!("{} where {}", unparse_expr_at(&r.redex, 2), unparse_expr(g)),
                None => unparse_expr_at(&r.redex, 2),
            };
            let reactum = unparse_expr_at(&r.reactum, 2);
            let rule_inline = format!("{redex} => {reactum}");
            let multi = redex.contains('\n')
                || reactum.contains('\n')
                || rule_inline.len() + 2 > MAX_WIDTH;
            let rule = if multi {
                format!("{redex}\n  =>\n  {reactum}")
            } else {
                rule_inline
            };
            // A `rate ( expr )` clause rides after the body (mirrors the parser).
            let rate = match &r.rate {
                Some(expr) => format!(" rate ( {} )", unparse_expr(expr)),
                None => String::new(),
            };
            format!(
                "reaction {}{} (\n  {rule}\n){rate}",
                r.name,
                unparse_bracket_params(&r.params),
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
        Def::Pattern(p) => {
            let params = if p.params.is_empty() {
                String::new()
            } else {
                let names: Vec<String> = p.params.iter().map(|x| x.name.clone()).collect();
                format!("[{}]", names.join(", "))
            };
            format!(
                "pattern {}{} (\n  {}\n)",
                p.name,
                params,
                unparse_expr_at(&p.body, 2)
            )
        }
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
            unparse_interface_inner(interface, Some(&c), 2, 2).trim_start(),
            unparse_body(body)
        ),
        None => {
            let prefix = format!("{kw} {name}{}", unparse_bracket_params(params));
            format!(
                "{prefix}{} {}",
                unparse_interface_inner(interface, None, prefix.len(), 0),
                unparse_body(body)
            )
        }
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

fn unparse_interface(iface: &Interface, start_col: usize) -> String {
    unparse_interface_inner(iface, None, start_col, 0)
}

/// Render `~{…} ->{…}`. Port types use `::` (`name :: Schema`); a port's contract
/// is emitted as `fulfills C` unless it equals `hoisted` (already raised to a
/// `fulfills C` clause on the header by the caller).
fn unparse_interface_inner(
    iface: &Interface,
    hoisted: Option<&ContractRef>,
    start_col: usize,
    base_indent: usize,
) -> String {
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
    let input_parts: Vec<String> = iface.inputs.iter().map(port).collect();
    let output_parts: Vec<String> = iface.outputs.iter().map(port).collect();
    fmt_ports(&input_parts, &output_parts, start_col, base_indent)
}

// ── bodies ───────────────────────────────────────────────────────────

/// A declaration body `( … )`. Block/Parallel (multiple statements) render
/// multi-line with items at indent 2 (so nested terms know their column and
/// can break at the same canonical indent); record/map and simple
/// expressions render inline `( e )`.
fn unparse_body(body: &Expr) -> String {
    match body {
        Expr::Block(Block { bindings, value }) => {
            let mut items: Vec<String> = bindings
                .iter()
                .map(|(n, e)| format!("{n} = {}", unparse_expr_at(e, 2)))
                .collect();
            items.push(unparse_expr_at(value, 2));
            pipe_join_multiline(&items, 0)
        }
        Expr::Parallel(items) if !items.is_empty() => {
            let parts: Vec<String> = items.iter().map(|i| unparse_expr_at(i, 2)).collect();
            pipe_join_multiline(&parts, 0)
        }
        Expr::Record(_) | Expr::Map(_) => format!("(\n  {}\n)", unparse_expr(body)),
        other => format!("( {} )", unparse_expr(other)),
    }
}

// ── expressions ──────────────────────────────────────────────────────

pub fn unparse_expr(e: &Expr) -> String {
    unparse_expr_at(e, 0)
}

/// `unparse_expr` aware of its column position. Terms / parallels / blocks /
/// lists / rules break across lines when the inline form would push past
/// [`MAX_WIDTH`] at the given indent; other expressions stay inline (they have
/// no natural break points). Children of a multi-line construct recurse at
/// `indent + 2` so nested terms know how deep they are.
pub fn unparse_expr_at(e: &Expr, indent: usize) -> String {
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
        } => unparse_term(control, args, ports, body, indent),
        Expr::Parallel(items) => {
            let parts: Vec<String> =
                items.iter().map(|i| unparse_expr_at(i, indent + 2)).collect();
            fmt_pipe_join(&parts, indent)
        }
        Expr::KeyedEntry { key, value } => {
            // Block-indent: a multi-line value's nested items hang at the
            // ENTRY's `indent + 2`, not at the value's start column — matching
            // composite/process body style (one canonical indent across the
            // language). The width budget is conservative (it ignores the
            // `key: ` prefix), which makes us slightly eager to break — fine.
            format!("{}: {}", unparse_key(key), unparse_expr_at(value, indent))
        }
        Expr::Map(entries) => {
            let parts: Vec<String> = entries
                .iter()
                .map(|(k, v)| {
                    // A static key renders bare (or quoted if not an identifier),
                    // converging with Record; only an INTERPOLATED key stays a
                    // quoted template — so quoting a static key never flips the
                    // round-trip representation.
                    let key = match k.as_plain() {
                        Some(s) => unparse_field_key(&s),
                        None => unparse_string(k),
                    };
                    format!("{key}: {}", unparse_expr_at(v, indent + 2))
                })
                .collect();
            fmt_braced("{", "}", &parts, indent, indent)
        }
        Expr::Record(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(k, v)| {
                    format!("{}: {}", unparse_field_key(k), unparse_expr_at(v, indent + 2))
                })
                .collect();
            fmt_braced("{", "}", &parts, indent, indent)
        }
        Expr::List(items) => {
            let parts: Vec<String> =
                items.iter().map(|i| unparse_expr_at(i, indent + 2)).collect();
            fmt_list_join(&parts, indent)
        }
        // The site `name` already carries its `?` prefix (e.g. `?f`).
        Expr::Site { name, sort } => match sort {
            Some(s) => format!("{name} :: {}", unparse_expr(s)),
            None => name.clone(),
        },
        Expr::Unbound => "!".into(),
        Expr::LinkVar(n) => format!("~{n}"),
        Expr::LinkDecl {
            name,
            schema,
            mesh,
            default,
        } => {
            let m = if *mesh { " mesh" } else { "" };
            match schema {
                Some(s) => format!(
                    "link {name} :: {}{m} = {}",
                    unparse_schema(s),
                    unparse_expr_at(default, indent)
                ),
                None => format!("link {name}{m} = {}", unparse_expr_at(default, indent)),
            }
        }
        Expr::Rule { redex, reactum } => fmt_rule(redex, reactum, indent),
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
                .map(|(n, e)| format!("{n} = {}", unparse_expr_at(e, indent + 2)))
                .collect();
            items.push(unparse_expr_at(&b.value, indent + 2));
            fmt_pipe_join(&items, indent)
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
    indent: usize,
) -> String {
    // Port entries + their inline width up front: a term that overshoots breaks
    // its ARGS (the bracketed config), keeping short ports on the closing `]`
    // line — so `fmt_term_args_at` needs to know how much trails the args.
    let input_parts: Vec<String> = ports
        .inputs
        .iter()
        .map(|(p, t)| format!("{p}: {}", unparse_expr(t)))
        .collect();
    let output_parts: Vec<String> = ports
        .outputs
        .iter()
        .map(|(p, t)| format!("{p}: {}", unparse_expr(t)))
        .collect();
    let mut ports_inline = String::new();
    if !input_parts.is_empty() {
        ports_inline.push_str(&format!(" ~{{{}}}", input_parts.join(", ")));
    }
    if !output_parts.is_empty() {
        ports_inline.push_str(&format!(" ->{{{}}}", output_parts.join(", ")));
    }
    let body_inline_len = body.as_ref().map(|b| unparse_expr(b).len() + 3).unwrap_or(0);

    let mut s = control.to_string();
    if !args.is_empty() {
        let trailing = ports_inline.len() + body_inline_len;
        s.push_str(&fmt_term_args_at(args, indent, control.len() + trailing, ports_inline.len()));
    }
    if !input_parts.is_empty() || !output_parts.is_empty() {
        // Column where ` ~{` begins: the end of `control` + args (which may
        // itself be multi-line — then the ports sit on the closing `]` line).
        let last_line = s.rsplit('\n').next().unwrap_or("");
        let port_start_col = if s.contains('\n') { last_line.len() } else { indent + s.len() };
        s.push_str(&fmt_ports(&input_parts, &output_parts, port_start_col, indent));
    }
    if let Some(b) = body {
        s.push_str(&format!(" {}", fmt_body(b, indent)));
    }
    s
}

/// A term/reaction body. Inline `(a | b | c)` when short; otherwise multi-line
/// with **pipes at end of line**, matching composite/process bodies.
fn fmt_body(b: &Expr, indent: usize) -> String {
    match b {
        Expr::Parallel(items) => {
            let parts: Vec<String> =
                items.iter().map(|i| unparse_expr_at(i, indent + 2)).collect();
            fmt_pipe_join(&parts, indent)
        }
        Expr::Block(bl) => {
            let mut items: Vec<String> = bl
                .bindings
                .iter()
                .map(|(n, e)| format!("{n} = {}", unparse_expr_at(e, indent + 2)))
                .collect();
            items.push(unparse_expr_at(&bl.value, indent + 2));
            fmt_pipe_join(&items, indent)
        }
        other => format!("({})", unparse_expr_at(other, indent + 2)),
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

/// Indent-aware term-args renderer: `[a, b, c]` inline when short, or one
/// entry per line when the inline form (placed at column `indent`) overshoots
/// [`MAX_WIDTH`]. Each entry recurses at `indent + 2` so a long sub-list (e.g.
/// `rules: [r1, r2, …]`) can itself break.
fn fmt_term_args_at(args: &[TermArg], indent: usize, context_width: usize, peer_width: usize) -> String {
    if args.is_empty() {
        return String::new();
    }
    let inline = format!("[{}]", unparse_term_args(args));
    if !inline.contains('\n') {
        // Keep inline if the whole term line (control + these args + the trailing
        // ports/body = `context_width`) fits, OR if the trailing ports are the
        // BIGGER group — then they break and these (shorter) args stay inline.
        let fits = indent + context_width + inline.len() + KEYED_SLOT_MARGIN <= MAX_WIDTH;
        if fits || inline.len() < peer_width {
            return inline;
        }
    }
    let inner = indent_str(indent + 2);
    let outer = indent_str(indent);
    let parts: Vec<String> = args
        .iter()
        .map(|a| match a {
            TermArg::Positional(e) => unparse_expr_at(e, indent + 2),
            TermArg::Named { name, value } => {
                // Block-indent: a multi-line value hangs at the arg's column
                // (`indent + 2`), not at the value's text-start. Matches the
                // KeyedEntry style above.
                format!("{name}: {}", unparse_expr_at(value, indent + 2))
            }
        })
        .collect();
    format!("[\n{inner}{}\n{outer}]", parts.join(&format!(",\n{inner}")))
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

/// Render a record/map key: a bare identifier when it is one, else a quoted
/// string. Bare and static-quoted keys converge to one form, so a static key is
/// a record field whether written bare or quoted, and round-trips stably.
fn unparse_field_key(name: &str) -> String {
    if is_bare_ident(name) {
        name.to_string()
    } else {
        format!("'{name}'")
    }
}

fn is_bare_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        // A keyword (`then`, `else`, `in`, …) is char-wise an identifier but
        // can't be a BARE field key — it must stay quoted to re-parse.
        && !crate::parse::is_keyword(s)
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

pub fn unparse_schema(s: &SchemaExpr) -> String {
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

// ── width-aware formatting helpers ──────────────────────────────────
//
// The unparser tries inline forms first and switches to multi-line when the
// inline form (placed at the current column `indent`) would exceed
// [`MAX_WIDTH`]. Multi-line bodies use **pipes at end of line**, matching the
// composite/process body style — one canonical layout across the language.

/// Target source-line width. Lines past this break onto multiple lines.
const MAX_WIDTH: usize = 100;

/// A composite body's children are keyed (`slot: Term …`); the term renderer
/// can't see that `slot: ` prefix, so it under-counts its column by a few chars.
/// This margin keeps a borderline line from staying inline then overflowing.
const KEYED_SLOT_MARGIN: usize = 4;

fn indent_str(n: usize) -> String {
    " ".repeat(n)
}

/// Render `items` joined by ` | ` inside `( … )`. Inline when short; multi-
/// line with **pipes at end of line** when the inline form (placed at column
/// `indent`) exceeds [`MAX_WIDTH`] or any item itself spans multiple lines.
/// Render a braced group (`{…}` record/map, `[…]` args) at column `start_col`:
/// inline `{open}a, b{close}` when it fits, else one entry per line at
/// `base_indent + 2` with the close at `base_indent` and **no trailing comma**
/// (the hand-written house style). `parts` are the already-rendered entries
/// (rendered at `base_indent + 2` so a long entry breaks the group too).
fn fmt_braced(open: &str, close: &str, parts: &[String], base_indent: usize, start_col: usize) -> String {
    let inline = format!("{open}{}{close}", parts.join(", "));
    if !parts.iter().any(|p| p.contains('\n')) && start_col + inline.len() <= MAX_WIDTH {
        return inline;
    }
    let inner = indent_str(base_indent + 2);
    let outer = indent_str(base_indent);
    format!("{open}\n{inner}{}\n{outer}{close}", parts.join(&format!(",\n{inner}")))
}

/// Render an interface `~{inputs} ->{outputs}` whose `~` begins at column
/// `start_col`. Inline when the whole thing fits at [`MAX_WIDTH`]; otherwise the
/// input group breaks one-entry-per-line (at `base_indent + 2`), and the output
/// group stays inline on the closing `}` line unless it too overshoots. No
/// trailing comma. `*_parts` are the already-rendered `name :: …` / `port: …`
/// entries.
fn fmt_ports(input_parts: &[String], output_parts: &[String], start_col: usize, base_indent: usize) -> String {
    let group = |arrow: &str, parts: &[String]| {
        if parts.is_empty() { String::new() } else { format!(" {arrow}{{{}}}", parts.join(", ")) }
    };
    let input_inline = group("~", input_parts);
    let output_inline = group("->", output_parts);
    let any_multi = input_parts.iter().chain(output_parts).any(|p| p.contains('\n'));
    if !any_multi && start_col + input_inline.len() + output_inline.len() + 2 <= MAX_WIDTH {
        return format!("{input_inline}{output_inline}");
    }
    let inner = indent_str(base_indent + 2);
    let outer = indent_str(base_indent);
    let broken = |arrow: &str, parts: &[String]| {
        format!(" {arrow}{{\n{inner}{}\n{outer}}}", parts.join(&format!(",\n{inner}")))
    };
    let mut s = String::new();
    if !input_parts.is_empty() {
        s.push_str(&broken("~", input_parts));
    }
    if !output_parts.is_empty() {
        // After a broken input the output sits on the `}` line (≈ `base_indent`);
        // with no input it sits at `start_col`.
        let out_col = if input_parts.is_empty() { start_col } else { base_indent + 1 };
        if !output_parts.iter().any(|p| p.contains('\n'))
            && out_col + output_inline.len() <= MAX_WIDTH
        {
            s.push_str(&output_inline);
        } else {
            s.push_str(&broken("->", output_parts));
        }
    }
    s
}

/// Pipe-join body items. Inline `(a | b)` when short; otherwise multi-line with
/// **pipes at end of line**, and a **blank line setting off a multi-line child**
/// from its neighbours (consecutive single-line children stay adjacent) — the
/// hand-written house style.
fn fmt_pipe_join(items: &[String], indent: usize) -> String {
    if items.is_empty() {
        return "()".into();
    }
    let inline = format!("({})", items.join(" | "));
    let inline_ok =
        !items.iter().any(|s| s.contains('\n')) && inline.len() + indent <= MAX_WIDTH;
    if inline_ok {
        return inline;
    }
    pipe_join_multiline(items, indent)
}

/// The multi-line body form `(\n  a |\n  b\n)`: **pipes at end of line**, with a
/// blank line setting off any multi-line child from its neighbours (consecutive
/// single-line children stay adjacent) — the hand-written house style.
fn pipe_join_multiline(items: &[String], indent: usize) -> String {
    let inner = indent_str(indent + 2);
    let outer = indent_str(indent);
    let mut joined = String::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            joined.push_str(" |\n");
            if item.contains('\n') || items[i - 1].contains('\n') {
                joined.push('\n');
            }
            joined.push_str(&inner);
        }
        joined.push_str(item);
    }
    format!("(\n{inner}{joined}\n{outer})")
}

/// Render `items` joined by `, ` inside `[ … ]`. Inline when short; multi-line
/// (one entry per line, trailing comma) when too long or any item is multi-
/// line. Symmetric with [`fmt_pipe_join`] for parallel composition.
fn fmt_list_join(items: &[String], indent: usize) -> String {
    if items.is_empty() {
        return "[]".into();
    }
    let inline = format!("[{}]", items.join(", "));
    let inline_ok =
        !items.iter().any(|s| s.contains('\n')) && inline.len() + indent <= MAX_WIDTH;
    if inline_ok {
        return inline;
    }
    let inner = indent_str(indent + 2);
    let outer = indent_str(indent);
    format!(
        "[\n{inner}{},\n{outer}]",
        items.join(&format!(",\n{inner}"))
    )
}

/// Render `redex => reactum` inline, or break across lines (redex / `=>` /
/// reactum each on its own line) when either side is multi-line or the inline
/// form is too long. The `=>` keeps the same indent as the redex so the
/// rewrite arrow reads vertically.
fn fmt_rule(redex: &Expr, reactum: &Expr, indent: usize) -> String {
    let r1 = unparse_expr_at(redex, indent);
    let r2 = unparse_expr_at(reactum, indent);
    let inline = format!("{r1} => {r2}");
    let inline_ok =
        !r1.contains('\n') && !r2.contains('\n') && inline.len() + indent <= MAX_WIDTH;
    if inline_ok {
        return inline;
    }
    let outer = indent_str(indent);
    format!("{r1}\n{outer}=>\n{outer}{r2}")
}
