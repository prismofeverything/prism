//! Typed AST for chrysalis.
//!
//! Populated by the parser (tier 1, future) or by hand for fixture
//! tests. Every node is plain data — the interpreter lives in
//! [`crate::eval`], the lowering to prism in [`crate::compile`].
//!
//! See `docs/chrysalis-design.md` for the surface syntax this models.
//! The kernel forms are:
//!
//! - `K[args](body)` — term construction with optional nested body
//! - `a | b` — parallel composition (symmetric monoidal tensor)
//! - `~{port: target}` / `->{port: target}` — input / output port bindings
//! - `?name`, `?name : Sort` — pattern variables (site / name vars)
//! - `~name` — link variable
//! - `!` — unbound port
//! - `=>` — reaction redex/reactum separator (inside `reaction` bodies)
//! - lowercase **definers** introduce entities; capitalized **controls** are
//!   the values being constructed.

use indexmap::IndexMap;

// =============================================================================
// Names, paths, schemas
// =============================================================================

/// Identifier name in the surface syntax. Lowercase identifiers are
/// bindings / definers; capitalized identifiers are controls.
pub type Name = String;

/// Root of a path expression.
#[derive(Clone, Debug, PartialEq)]
pub enum PathRoot {
    /// `@` — self / current composite
    Here,
    /// `^` — one place-graph level up
    Parent,
    /// A bare identifier interpreted as the head of a child path.
    Local(Name),
}

/// A path through the place graph: `@.field.sub`, `^.cells`, `mass`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacePath {
    pub root: PathRoot,
    pub segments: Vec<Name>,
}

impl PlacePath {
    pub fn local(name: impl Into<Name>) -> Self {
        Self {
            root: PathRoot::Local(name.into()),
            segments: vec![],
        }
    }

    pub fn here() -> Self {
        Self {
            root: PathRoot::Here,
            segments: vec![],
        }
    }

    pub fn parent() -> Self {
        Self {
            root: PathRoot::Parent,
            segments: vec![],
        }
    }

    pub fn dot(mut self, segment: impl Into<Name>) -> Self {
        self.segments.push(segment.into());
        self
    }
}

/// Surface-language type expression, used in parameter and port
/// schema declarations. Lowered to [`prism_schema::Schema`] at compile
/// time.
#[derive(Clone, Debug, PartialEq)]
pub enum SchemaExpr {
    Any,
    Bool,
    Int,
    Float,
    String,
    Map(Box<SchemaExpr>),
    List(Box<SchemaExpr>),
    /// `{ field: T, … }` — a record (named, individually-typed fields).
    /// Lowers to [`prism_schema::Schema::Tree`]. The natural representation
    /// for a structured `type` (e.g. a graph's `{nodes, edges}`).
    Record(IndexMap<Name, SchemaExpr>),
    /// `Custom[Name(params...)]` — a registered type, e.g. `Cell`,
    /// `Map[Cell]` (parameterized).
    Custom {
        name: Name,
        params: Vec<SchemaExpr>,
    },
    /// `@` in a `self: @` declaration — the enclosing composite's
    /// own schema.
    SelfType,
    /// `Quantity[unit: U, extensive?, affine?]` — a dimensioned scalar.
    /// The magnitude is stored as a `Float`; the unit lives here in the
    /// schema and is *erased* after the dimensional check (see
    /// docs/chrysalis-design.md, "Units and quantities"). `extensive`
    /// splits on divide (vs. copy); `affine` is a point quantity whose
    /// differences are deltas (vs. a vector quantity).
    Quantity {
        unit: UnitExpr,
        extensive: bool,
        affine: bool,
    },
    /// `Array[shape, element]` — a fixed-shape numeric array whose element
    /// may itself be dimensioned (`Array[[6,5], Quantity[unit: …]]`), so a
    /// spatial field carries units element-wise.
    Array {
        shape: Vec<usize>,
        element: Box<SchemaExpr>,
    },
    /// `overwrite[T]` — a [`prism_schema::Schema::Overwrite`] wrapper: updates
    /// REPLACE the value instead of accumulating (e.g. a Gillespie τ a step sets
    /// each tick). The additive default would wrongly sum successive writes.
    Overwrite(Box<SchemaExpr>),
}

impl SchemaExpr {
    pub fn map_of(inner: SchemaExpr) -> Self {
        Self::Map(Box::new(inner))
    }
    pub fn list_of(inner: SchemaExpr) -> Self {
        Self::List(Box::new(inner))
    }
    pub fn overwrite_of(inner: SchemaExpr) -> Self {
        Self::Overwrite(Box::new(inner))
    }
    pub fn custom(name: impl Into<Name>) -> Self {
        Self::Custom {
            name: name.into(),
            params: vec![],
        }
    }
    pub fn quantity(unit: UnitExpr, extensive: bool, affine: bool) -> Self {
        Self::Quantity {
            unit,
            extensive,
            affine,
        }
    }
    pub fn array(shape: Vec<usize>, element: SchemaExpr) -> Self {
        Self::Array {
            shape,
            element: Box::new(element),
        }
    }
}

/// A definer's configuration parameter: `name: schema = default?`.
#[derive(Clone, Debug)]
pub struct Param {
    pub name: Name,
    pub schema: SchemaExpr,
    pub default: Option<Expr>,
}

impl Param {
    pub fn required(name: impl Into<Name>, schema: SchemaExpr) -> Self {
        Self {
            name: name.into(),
            schema,
            default: None,
        }
    }

    pub fn with_default(name: impl Into<Name>, schema: SchemaExpr, default: Expr) -> Self {
        Self {
            name: name.into(),
            schema,
            default: Some(default),
        }
    }
}

/// A reference to a process contract: a named contract plus optional axis
/// pins layered on top (`DeterministicMassAction[method: Rk4]`). Axis
/// values are nominal members (lowered to parameter-free `Custom`s); pins
/// refine the named contract's axes. See docs/process-contracts.md.
#[derive(Clone, Debug, PartialEq)]
pub struct ContractRef {
    pub name: Name,
    pub pins: IndexMap<Name, Name>,
}

impl ContractRef {
    pub fn new(name: impl Into<Name>) -> Self {
        Self {
            name: name.into(),
            pins: IndexMap::new(),
        }
    }
    pub fn pin(mut self, axis: impl Into<Name>, value: impl Into<Name>) -> Self {
        self.pins.insert(axis.into(), value.into());
        self
    }
}

/// A port declaration on the interface of a process/step/composite
/// definition: `port_name: Schema = default?`.
#[derive(Clone, Debug)]
pub struct PortDecl {
    pub schema: SchemaExpr,
    pub default: Option<Expr>,
    /// `port :: Contract` — the process-contract this port carries (an
    /// output port) or demands (an input port). Substitutability at a wire
    /// is `refines` over the lowered contract schemas (see
    /// docs/process-contracts.md). `None` = no contract constraint.
    pub contract: Option<ContractRef>,
    /// `port :: Type @ internal.path` — the composite **bridge** target: the
    /// internal state path this interface port maps to (as dotted segments,
    /// e.g. `["fields", "values"]`). `None` = name-inferred (the port maps to
    /// the same-named top-level field), the backward-compatible default. See
    /// `eval::build_composite_outer`.
    pub bridge: Option<Vec<Name>>,
}

impl PortDecl {
    pub fn required(schema: SchemaExpr) -> Self {
        Self {
            schema,
            default: None,
            contract: None,
            bridge: None,
        }
    }
    pub fn with_default(schema: SchemaExpr, default: Expr) -> Self {
        Self {
            schema,
            default: Some(default),
            contract: None,
            bridge: None,
        }
    }
    /// Attach a contract constraint (`port :: Contract`).
    pub fn with_contract(mut self, contract: ContractRef) -> Self {
        self.contract = Some(contract);
        self
    }
    /// Attach an explicit bridge target (`port :: Type @ internal.path`).
    pub fn with_bridge(mut self, path: Vec<Name>) -> Self {
        self.bridge = Some(path);
        self
    }
}

/// `~{...} ->{...}` — the interface declaration on a definer. The
/// categorical reading: `inputs` is the morphism's domain, `outputs`
/// its codomain.
#[derive(Clone, Debug, Default)]
pub struct Interface {
    pub inputs: IndexMap<Name, PortDecl>,
    pub outputs: IndexMap<Name, PortDecl>,
}

impl Interface {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_input(mut self, name: impl Into<Name>, decl: PortDecl) -> Self {
        self.inputs.insert(name.into(), decl);
        self
    }

    pub fn with_output(mut self, name: impl Into<Name>, decl: PortDecl) -> Self {
        self.outputs.insert(name.into(), decl);
        self
    }
}

/// `~{...} ->{...}` — port bindings at a *call site*. The target
/// expressions are interpreted by context: a [`PlacePath`] (state
/// wiring) when binding a sub-process, or a link expression
/// (`Expr::LinkVar` / `Expr::Unbound`) when binding pattern ports.
#[derive(Clone, Debug, Default)]
pub struct PortBindings {
    pub inputs: IndexMap<Name, Expr>,
    pub outputs: IndexMap<Name, Expr>,
}

impl PortBindings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty()
    }

    pub fn with_input(mut self, name: impl Into<Name>, target: Expr) -> Self {
        self.inputs.insert(name.into(), target);
        self
    }

    pub fn with_output(mut self, name: impl Into<Name>, target: Expr) -> Self {
        self.outputs.insert(name.into(), target);
        self
    }
}

// =============================================================================
// Units, dimensions, contexts
// =============================================================================
//
// Units live in the schema, not in values; the magnitude is a plain
// `Float`. A dimension (a rational-exponent vector over base dimensions)
// is the compatibility layer; a unit is a scale/offset within one
// dimension; a `context` bridges *different* dimensions when a physical
// relation justifies it. See docs/chrysalis-design.md, "Units and
// quantities" + "Contexts".

/// A rational exponent on a base dimension (so √-dimensions are
/// representable). Most dimensions use integer powers (`den == 1`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Ratio {
    pub num: i32,
    pub den: i32,
}

impl Ratio {
    pub fn int(n: i32) -> Self {
        Self { num: n, den: 1 }
    }
    pub fn new(num: i32, den: i32) -> Self {
        Self { num, den }
    }
}

/// A dimension: base-dimension name → rational exponent. Empty is
/// dimensionless. `[mass]` is `{mass: 1}`; `[substance]/[length]^3` is
/// `{substance: 1, length: -3}`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Dimension {
    pub powers: IndexMap<Name, Ratio>,
}

impl Dimension {
    pub fn dimensionless() -> Self {
        Self::default()
    }

    /// Build from integer powers:
    /// `Dimension::of(&[("substance", 1), ("length", -3)])`.
    pub fn of(powers: &[(&str, i32)]) -> Self {
        Self {
            powers: powers
                .iter()
                .map(|(n, p)| ((*n).to_string(), Ratio::int(*p)))
                .collect(),
        }
    }
}

/// A unit expression: a product / quotient / power of named units and
/// scalar factors — `pg`, `1/s`, `molecule/fL`, `um^3`, `1e-12 kg`.
#[derive(Clone, Debug, PartialEq)]
pub enum UnitExpr {
    /// A named unit: `kg`, `s`, `mol`, `pg`, `um`, …
    Named(Name),
    /// A dimensionless scalar factor: the `1` in `1/s`, `1e-12` in `1e-12 kg`.
    Scalar(f64),
    Mul(Box<UnitExpr>, Box<UnitExpr>),
    Div(Box<UnitExpr>, Box<UnitExpr>),
    Pow(Box<UnitExpr>, Ratio),
}

impl UnitExpr {
    pub fn named(n: impl Into<Name>) -> Self {
        Self::Named(n.into())
    }
    pub fn scalar(f: f64) -> Self {
        Self::Scalar(f)
    }
    pub fn mul(self, rhs: UnitExpr) -> Self {
        Self::Mul(Box::new(self), Box::new(rhs))
    }
    pub fn div(self, rhs: UnitExpr) -> Self {
        Self::Div(Box::new(self), Box::new(rhs))
    }
    pub fn pow(self, exp: Ratio) -> Self {
        Self::Pow(Box::new(self), exp)
    }
    /// `1 / named` — the common reciprocal-unit form (`1/s`).
    pub fn per(n: impl Into<Name>) -> Self {
        Self::Scalar(1.0).div(Self::Named(n.into()))
    }
}

/// `unit name : [dimension] = definition` — declares a unit by its
/// relation to canonical units. `affine_offset` is `Some` for offset
/// units (`degC = K + 273.15`), `None` for purely multiplicative units.
#[derive(Clone, Debug)]
pub struct UnitDef {
    pub name: Name,
    pub dimension: Dimension,
    pub definition: UnitExpr,
    pub affine_offset: Option<f64>,
}

/// `context Name(params) ( from <-> to : transform | ... )` — named,
/// parameterized cross-dimension conversion rules. Parameters bind to
/// constants, place-graph paths, or type metadata at the use site.
#[derive(Clone, Debug)]
pub struct ContextDef {
    pub name: Name,
    pub params: Vec<Param>,
    pub rules: Vec<ContextRule>,
}

/// One rule in a context: `from <-> to : transform`. The transform
/// `Expr` may reference `value` (the source magnitude) and the
/// context's parameters.
#[derive(Clone, Debug)]
pub struct ContextRule {
    pub from: Dimension,
    pub to: Dimension,
    /// `<->` (true) installs both directions; `->` (false) is one-way.
    pub bidirectional: bool,
    pub transform: Expr,
}

/// `using Name(args)` — activates a context over a composite's body and
/// everything nested inside it. Args bind the context's params, e.g.
/// `concentration(volume: @.volume)`.
#[derive(Clone, Debug)]
pub struct ContextUse {
    pub name: Name,
    pub args: Vec<TermArg>,
}

// =============================================================================
// Top-level definitions
// =============================================================================

#[derive(Clone, Debug)]
pub enum Def {
    Process(ProcessDef),
    Step(StepDef),
    /// `def name(params) [:: Ret] = body` — a first-class function: a named
    /// param→value body, the same eval as a process/step body without the
    /// bigraph interface. Called as `name(args)` ([`Expr::Call`]).
    Function(FunctionDef),
    Composite(CompositeDef),
    Reaction(ReactionDef),
    Pattern(PatternDef),
    /// `unit name : [dim] = definition` — a unit declaration.
    Unit(UnitDef),
    /// `context Name(params) (...)` — cross-dimension conversion rules.
    Context(ContextDef),
    /// `type Name = <representation> with { method(args) = body, … }` — a
    /// first-class user-defined type. The representation is the schema the
    /// type is made of (the algebra delegates structural ops to it); the
    /// methods are its operations. A method `m(self, args) → body` is used as
    /// a **query** (`v.m(args)` evaluates it) and/or as a **write action**
    /// (an update directive `{_call: {method, args}}` on a slot of this type,
    /// which `apply` interprets — generalizing `_add`/`_remove`/`_divide`).
    Type(TypeDef),
    /// `contract Name (axis: value, …)` — a named process contract: a record
    /// of axes (target / method / claims / advance). Substitutability between
    /// contracts is `refines` over their lowered `Tree` schemas — no new
    /// algebra op. See [`ContractDef`] and docs/process-contracts.md.
    Contract(ContractDef),
    /// `import Name from "path.ys"` — pull another file's definitions into
    /// scope. Resolved by [`crate::parse::parse_file`] (load the file, merge
    /// its defs); after resolution no `Import` remains in a `Program`.
    Import {
        name: Name,
        path: String,
    },
    /// `from <module> import <name>, …` — pull NATIVE host capabilities into
    /// scope: either a whole process (`from core import RunProcess`, used as-is
    /// with no interface redeclaration) or functions/objects
    /// (`from integrators import rk4, euler`) that a `.ys` `process`/`step` body
    /// then calls (`rk4.integrate(network, state, interval)`). Resolved at
    /// compile time against the host-supplied native module registry — the
    /// replacement for `extern`. (Distinct from [`Def::Import`], which pulls in
    /// another `.ys` *file*.)
    Use {
        module: Name,
        names: Vec<Name>,
    },
    /// Top-level binding `name = expr`, or type-ascribed `name :: Type = expr`
    /// (e.g. `network :: CRN = {species: …}` — a shared, typed value realized
    /// through the consuming method). `schema` is the optional `:: Type`.
    Binding {
        name: Name,
        schema: Option<SchemaExpr>,
        value: Expr,
    },
}

/// `type Name = <representation> with { method(args) = body }`.
#[derive(Clone, Debug)]
pub struct TypeDef {
    pub name: Name,
    /// Type parameters (reserved for parameterized types; usually empty).
    pub params: Vec<Param>,
    /// What the type is made of — the algebra delegates `default`/`apply`/
    /// `divide`/`serialize`/`check` to this schema unless a method overrides.
    pub representation: SchemaExpr,
    /// The type's operations. Each is a function of `self` (the receiver) +
    /// `params`, returning a value. Reachable as a query and as an `apply`
    /// directive (see [`Def::Type`]).
    pub methods: Vec<MethodDef>,
}

/// A single method on a [`TypeDef`]: `name(params) = body`, with `self`
/// implicitly bound to the receiver.
#[derive(Clone, Debug)]
pub struct MethodDef {
    pub name: Name,
    pub params: Vec<Param>,
    pub body: Expr,
}

/// `contract Name (axis: value, …)` — a named process contract: a record of
/// axes (target / method / claims / advance), each pinned to a nominal
/// member. Lowers to a `Schema::Tree` of nominal `Custom`s; substitutability
/// between contracts is `refines` over those trees — no new algebra op. See
/// docs/process-contracts.md.
#[derive(Clone, Debug)]
pub struct ContractDef {
    pub name: Name,
    pub axes: IndexMap<Name, Name>,
}

#[derive(Clone, Debug)]
pub struct ProcessDef {
    pub name: Name,
    pub params: Vec<Param>,
    pub interface: Interface,
    pub body: Expr,
}

/// `def name(params) [:: Ret] = body` — a first-class function (see
/// [`Def::Function`]). No interface; the body is a param→value expression.
#[derive(Clone, Debug)]
pub struct FunctionDef {
    pub name: Name,
    pub params: Vec<Param>,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct StepDef {
    pub name: Name,
    pub params: Vec<Param>,
    pub interface: Interface,
    pub body: Expr,
}

#[derive(Clone, Debug)]
pub struct CompositeDef {
    pub name: Name,
    pub params: Vec<Param>,
    pub interface: Interface,
    /// Contexts activated over this composite's body and everything
    /// nested inside it (the place graph *is* the activation scope).
    pub using: Vec<ContextUse>,
    pub body: Expr,
}

/// `reaction R[args] (redex => reactum)` — registered as a function
/// returning a `ReactionRule` value at runtime.
#[derive(Clone, Debug)]
pub struct ReactionDef {
    pub name: Name,
    pub params: Vec<Param>,
    pub redex: Expr,
    pub reactum: Expr,
    /// Optional `where` guard attached to the redex. Predicate over
    /// bound pattern variables.
    pub guard: Option<Expr>,
    /// Rate expression — closes over matched bindings (future) and
    /// configured params (today).
    pub rate: Option<Expr>,
}

/// `pattern P[args] (body)` — function returning a `Pattern` value.
#[derive(Clone, Debug)]
pub struct PatternDef {
    pub name: Name,
    pub params: Vec<Param>,
    pub body: Expr,
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub defs: Vec<Def>,
}

impl Program {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, def: Def) {
        self.defs.push(def);
    }

    pub fn lookup(&self, name: &str) -> Option<&Def> {
        self.defs.iter().find(|d| def_name(d) == name)
    }

    /// The file's runnable **entry point** — its *last* top-level definition that
    /// carries an interface: a `composite`/`process`/`step` (a simulation) or a
    /// `def` function (a pure transform). This realizes the rule "a file's value
    /// is its last top-level term": vocabulary defs accumulate, and the final
    /// interfaced term is what `chrysalis run file.ys` invokes — its interface
    /// becomes the command's typed I/O (config + inputs in, outputs out). An
    /// explicit trailing expression (parsed as the `main` binding) still takes
    /// precedence; this is the no-`main` fallback. `None` ⇒ a pure package
    /// (importable, not runnable on its own). See chrysalis-design.md (#24).
    pub fn entry(&self) -> Option<&Def> {
        self.defs.iter().rev().find(|d| {
            matches!(
                d,
                Def::Composite(_) | Def::Process(_) | Def::Step(_) | Def::Function(_)
            )
        })
    }
}

pub fn def_name(def: &Def) -> &str {
    match def {
        Def::Process(d) => &d.name,
        Def::Step(d) => &d.name,
        Def::Function(d) => &d.name,
        Def::Composite(d) => &d.name,
        Def::Reaction(d) => &d.name,
        Def::Pattern(d) => &d.name,
        Def::Unit(d) => &d.name,
        Def::Context(d) => &d.name,
        Def::Type(d) => &d.name,
        Def::Contract(d) => &d.name,
        Def::Import { name, .. } => name,
        Def::Use { module, .. } => module,
        Def::Binding { name, .. } => name,
    }
}

// =============================================================================
// Expressions
// =============================================================================

/// The core syntactic form. Bodies are `Expr`s; the interpreter in
/// [`crate::eval`] reduces an `Expr` to a `prism_schema::Value` (or to
/// a `Pattern`, `ReactionRule`, etc. wrapped as a `Value::Foreign`).
#[derive(Clone, Debug)]
pub enum Expr {
    // ── Literals ──
    Unit,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(StringLit),

    // ── References ──
    /// A bare identifier lookup. Resolves against the lexical
    /// environment (params, lets, top-level bindings).
    Var(Name),
    /// A path through the place graph (`@`, `^`, dotted child).
    Path(PlacePath),

    // ── Construction (the K[args](body) kernel form) ──
    Term {
        control: Name,
        args: Vec<TermArg>,
        ports: PortBindings,
        body: Option<Box<Expr>>,
    },

    // ── Composition ──
    /// `a | b | c` — symmetric monoidal tensor. Lowers to a Map (if
    /// every element is a [`Expr::KeyedEntry`]) or a List (if all
    /// anonymous), per the compilation map.
    Parallel(Vec<Expr>),

    /// `name: value` — a named entry inside a parallel composition or
    /// inside a map/record literal.
    KeyedEntry {
        key: StringLit,
        value: Box<Expr>,
    },

    /// `{ 'key': expr, ... }` — runtime-keyed map. Keys are
    /// [`StringLit`]s so they can be computed via interpolation.
    Map(Vec<(StringLit, Expr)>),

    /// `{ field: expr, ... }` — compile-time-fielded record. Bare
    /// identifiers are literal field names.
    Record(IndexMap<Name, Expr>),

    /// `[a, b, c]` — list literal.
    List(Vec<Expr>),

    // ── Pattern-only forms ──
    /// `?name` or `?name : Sort` — a pattern variable. Distinguished
    /// at interpretation time between subtree-var and atom-var by
    /// position; sort, when present, constrains the bound shape.
    Site {
        name: Name,
        sort: Option<Box<Expr>>,
    },
    /// `!` — unbound port (absent link).
    Unbound,
    /// `~name` — link variable.
    LinkVar(Name),

    // ── Reaction bodies ──
    /// `redex => reactum` — only legal inside a `reaction` body. The
    /// surrounding [`ReactionDef`] supplies the guard and rate, so
    /// this variant only carries the two sides.
    Rule {
        redex: Box<Expr>,
        reactum: Box<Expr>,
    },

    // ── Bindings ──
    /// `let name = expr in body` or block-style chained bindings.
    Let {
        bindings: Vec<(Name, Expr)>,
        body: Box<Expr>,
    },

    /// `( stmt | stmt | ... | value )` — the body convention. Each
    /// statement is either a binding `name = expr` or a side
    /// expression (currently only used for emit-style positional
    /// children inside composite bodies; in expression bodies, a
    /// stray bare expression is an error). The final un-`|`-ed line
    /// is `value`.
    Block(Block),

    // ── Control flow ──
    If {
        cond: Box<Expr>,
        then_: Box<Expr>,
        else_: Option<Box<Expr>>,
    },

    // ── Operators (sugar over `Add[a, b]` etc.) ──
    BinOp {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },

    // ── Method dispatch ──
    /// `receiver.method(args)` — dispatched via
    /// [`prism_schema::MethodRegistry`] on the receiver's type.
    Method {
        receiver: Box<Expr>,
        method: Name,
        args: Vec<Expr>,
    },
    /// `base.field` where `base` is a *value* expression, not a place-path —
    /// value field access: evaluate `base`, then read field `name` (`None` if
    /// absent). Place-rooted access (`var.seg`, for wiring) stays a `Path`; this
    /// is the general case so *any* value composes under `.field` (e.g.
    /// `all[].config.bridge`), mirroring how `.method()` already works on any
    /// base.
    Field {
        base: Box<Expr>,
        name: Name,
    },
    /// `func(args)` — call a function. `func` is usually a `Var` naming a
    /// `def name(params) = body` ([`Def::Function`]); the body evaluates with
    /// `params` bound to `args` (the same mechanism as a process/step body).
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `[ body for var in source if filter ]` — a list comprehension (map +
    /// optional filter over a list). The iteration construct the surface
    /// language was missing; it makes traversal/queries (e.g. a graph type's
    /// `neighbors`) expressible *in chrysalis*. `var` is bound to each element
    /// of `source` (a list); when `filter` holds (or is absent), `body` is
    /// evaluated and collected.
    Comprehension {
        var: Name,
        source: Box<Expr>,
        filter: Option<Box<Expr>>,
        body: Box<Expr>,
    },

    // ── Sugar ──
    /// `replace id with { ... }` desugars to a `{_remove: [id], _add: { ... }}`
    /// map at eval time.
    ReplaceWith {
        id: Box<Expr>,
        with: Box<Expr>,
    },

    /// `inner where predicate` — attaches a guard. Currently used only
    /// inside redex positions of reactions; lifted into the enclosing
    /// `ReactionDef::guard` slot at compile time.
    Where {
        inner: Box<Expr>,
        predicate: Box<Expr>,
    },
}

#[derive(Clone, Debug)]
pub struct Block {
    pub bindings: Vec<(Name, Expr)>,
    pub value: Box<Expr>,
}

impl Block {
    pub fn just(value: Expr) -> Self {
        Self {
            bindings: vec![],
            value: Box::new(value),
        }
    }

    pub fn from_parts(bindings: Vec<(Name, Expr)>, value: Expr) -> Self {
        Self {
            bindings,
            value: Box::new(value),
        }
    }
}

/// An argument inside `K[args]`. Positional comes before any named
/// args in the surface syntax.
#[derive(Clone, Debug)]
pub enum TermArg {
    Positional(Expr),
    Named { name: Name, value: Expr },
}

impl TermArg {
    pub fn named(name: impl Into<Name>, value: Expr) -> Self {
        Self::Named {
            name: name.into(),
            value,
        }
    }
}

/// A `'literal text {expr}'` string literal. A `Lit`-only `StringLit`
/// is a plain string; intermixing `Expr` segments makes it a
/// template.
#[derive(Clone, Debug)]
pub struct StringLit {
    pub segments: Vec<StringSeg>,
}

#[derive(Clone, Debug)]
pub enum StringSeg {
    Lit(String),
    Expr(Expr),
}

impl StringLit {
    pub fn plain(s: impl Into<String>) -> Self {
        Self {
            segments: vec![StringSeg::Lit(s.into())],
        }
    }

    pub fn template(segments: Vec<StringSeg>) -> Self {
        Self { segments }
    }

    /// If every segment is a literal, return the concatenation.
    pub fn as_plain(&self) -> Option<String> {
        let mut out = String::new();
        for seg in &self.segments {
            match seg {
                StringSeg::Lit(s) => out.push_str(s),
                StringSeg::Expr(_) => return None,
            }
        }
        Some(out)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Concat, // `++`
    In,     // `x in xs` — membership (list contains value / map contains key)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

// =============================================================================
// Constructor sugar for fixtures
// =============================================================================

impl Expr {
    pub fn var(name: impl Into<Name>) -> Self {
        Self::Var(name.into())
    }

    pub fn int(n: i64) -> Self {
        Self::Int(n)
    }

    pub fn float(f: f64) -> Self {
        Self::Float(f)
    }

    pub fn string(s: impl Into<String>) -> Self {
        Self::Str(StringLit::plain(s))
    }

    pub fn bool(b: bool) -> Self {
        Self::Bool(b)
    }

    pub fn add(lhs: Expr, rhs: Expr) -> Self {
        Self::BinOp {
            op: BinOp::Add,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    pub fn mul(lhs: Expr, rhs: Expr) -> Self {
        Self::BinOp {
            op: BinOp::Mul,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    pub fn div(lhs: Expr, rhs: Expr) -> Self {
        Self::BinOp {
            op: BinOp::Div,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    pub fn gt(lhs: Expr, rhs: Expr) -> Self {
        Self::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    pub fn sub(lhs: Expr, rhs: Expr) -> Self {
        Self::BinOp {
            op: BinOp::Sub,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    pub fn neg(operand: Expr) -> Self {
        Self::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(operand),
        }
    }

    pub fn method(receiver: Expr, method: impl Into<Name>, args: Vec<Expr>) -> Self {
        Self::Method {
            receiver: Box::new(receiver),
            method: method.into(),
            args,
        }
    }

    pub fn term(control: impl Into<Name>) -> TermBuilder {
        TermBuilder::new(control)
    }

    pub fn site(name: impl Into<Name>) -> Self {
        Self::Site {
            name: name.into(),
            sort: None,
        }
    }

    pub fn site_typed(name: impl Into<Name>, sort: Expr) -> Self {
        Self::Site {
            name: name.into(),
            sort: Some(Box::new(sort)),
        }
    }

    pub fn entry(key: impl Into<String>, value: Expr) -> Self {
        Self::KeyedEntry {
            key: StringLit::plain(key),
            value: Box::new(value),
        }
    }

    pub fn parallel(elems: Vec<Expr>) -> Self {
        Self::Parallel(elems)
    }
}

/// Fluent builder for [`Expr::Term`] — used heavily by fixtures and
/// (eventually) by the parser. Lets you write
/// `Expr::term("Cell").arg_named("mass", …).body(…).build()`.
pub struct TermBuilder {
    control: Name,
    args: Vec<TermArg>,
    ports: PortBindings,
    body: Option<Box<Expr>>,
}

impl TermBuilder {
    pub fn new(control: impl Into<Name>) -> Self {
        Self {
            control: control.into(),
            args: vec![],
            ports: PortBindings::default(),
            body: None,
        }
    }

    pub fn arg(mut self, value: Expr) -> Self {
        self.args.push(TermArg::Positional(value));
        self
    }

    pub fn arg_named(mut self, name: impl Into<Name>, value: Expr) -> Self {
        self.args.push(TermArg::named(name, value));
        self
    }

    pub fn input(mut self, port: impl Into<Name>, target: Expr) -> Self {
        self.ports.inputs.insert(port.into(), target);
        self
    }

    pub fn output(mut self, port: impl Into<Name>, target: Expr) -> Self {
        self.ports.outputs.insert(port.into(), target);
        self
    }

    pub fn body(mut self, body: Expr) -> Self {
        self.body = Some(Box::new(body));
        self
    }

    pub fn build(self) -> Expr {
        Expr::Term {
            control: self.control,
            args: self.args,
            ports: self.ports,
            body: self.body,
        }
    }
}
