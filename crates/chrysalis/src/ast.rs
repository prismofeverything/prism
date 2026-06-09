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
    /// `protocol Name = stream<Cell, path: 'cell.ys'>` — bind a composite/process
    /// to a transport protocol + its typed address fields, producing a reusable
    /// addressed control. `Name[config] ~{} ->{}` places the wrapped composite at
    /// the typed address `{_type: protocol, …fields}` (the transport realizes it
    /// into a live process). See docs/protocols-as-types.md.
    Protocol(ProtocolDef),
    /// `from <module> import <name>, …` — the ONE import form (#50). `module`
    /// is a dotted path; `names` are the EXPLICITLY selected defs (no whole-file
    /// "dump" — every name traces to its import). Resolved in one of two ways,
    /// FILE-module first:
    ///   - **file module**: a backing `.ys` exists (a dotted `<pkg>.<sub>.<file>`,
    ///     or a single-segment sibling that exists) — [`crate::parse::parse_file`]
    ///     merges the named defs + their transitive value-deps + the file's
    ///     type/contract/unit/context/`use` vocabulary; no file-module `Use`
    ///     survives resolution.
    ///   - **native host module** (`from core import RunProcess`,
    ///     `from integrators import rk4, euler`): no backing file — resolved at
    ///     compile time against the host-supplied module registry (the `extern`
    ///     replacement). A `.ys` `process`/`step` body then calls the imported
    ///     object (`rk4.integrate(network, state, interval)`).
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

/// `protocol Name = <protocol><Wrapped, field: value, …>` — a composite/process
/// bound to a transport protocol with its typed address fields. The wrapped
/// control is placed at the typed address `{_type: protocol, …fields}`.
#[derive(Clone, Debug)]
pub struct ProtocolDef {
    /// The alias name (`StreamingCell`).
    pub name: Name,
    /// The transport / address-type tag (`stream`, `rest`, `parallel`, `local`).
    pub protocol: Name,
    /// The composite/process control being addressed (`Cell`).
    pub wrapped: Name,
    /// The protocol's address fields (`path: 'cell.ys'`, `host`/`port`, …), each an
    /// expression evaluated against the program's bindings.
    pub fields: Vec<(Name, Expr)>,
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

    /// Owned form of [`Self::entity`] — same shape, cloned. Useful when the
    /// caller wants to mutate / transform / serialize the entity without
    /// holding a borrow on the program (e.g. to expose programs as data).
    pub fn entity_owned(&self, name: &str) -> Option<EntityDef> {
        let mut found = false;
        let mut ent = EntityDef::new(name);
        for def in &self.defs {
            if def_name(def) == name {
                ent.absorb_def(def);
                found = true;
            }
        }
        found.then_some(ent)
    }

    /// The whole program as a chrysalis [`prism_schema::Value`] — the
    /// homoiconic entity-registry view. Shape:
    /// `{_type: "Program", entities: [EntityDef…]}`. The summary form for
    /// each entity is [`EntityDef::to_value`]; body Exprs aren't recursed
    /// here (deeper AST-as-value is the stretch slice of #32). Pairs with
    /// `load(path)._entities` so loaded programs are inspectable from `.ys`.
    pub fn to_value(&self) -> prism_schema::Value {
        use indexmap::IndexMap;
        use prism_schema::{Key, Value};
        let mut fields: IndexMap<Key, Value> = IndexMap::new();
        fields.insert(Key::from("_type"), Value::String("Program".into()));
        fields.insert(
            Key::from("entities"),
            Value::List(self.owned_entities().iter().map(EntityDef::to_value).collect()),
        );
        Value::Map(fields)
    }

    /// All entities in the program, owned and ready to manipulate. Preserves
    /// the FIRST-APPEARANCE order of each name in `defs` (so the result
    /// reads top-down like the source). Different definers contributing to
    /// the same name fold into one entity.
    pub fn owned_entities(&self) -> Vec<EntityDef> {
        let mut order: Vec<Name> = Vec::new();
        let mut map: indexmap::IndexMap<Name, EntityDef> = indexmap::IndexMap::new();
        for def in &self.defs {
            // `use` imports don't contribute slots — skip name registration.
            if matches!(def, Def::Use { .. }) {
                continue;
            }
            let name = def_name(def).to_string();
            if !map.contains_key(&name) {
                order.push(name.clone());
                map.insert(name.clone(), EntityDef::new(name.clone()));
            }
            map.get_mut(&name).unwrap().absorb_def(def);
        }
        order.into_iter().filter_map(|n| map.shift_remove(&n)).collect()
    }

    /// One named entity, viewed as the union of every Def in this program that
    /// shares `name` — the homoiconic frame "a type is a control with extras":
    /// one identity carrying optional slots (`process` / `composite` /
    /// `reaction` / `type` / `function` / …) filled by whichever lowercase
    /// definers contributed them. `None` if no Def names `name`.
    ///
    /// The view borrows — it doesn't restructure the AST, just indexes it.
    /// For an owned / serializable form, see [`Self::entity_owned`] /
    /// [`Self::owned_entities`].
    pub fn entity(&self, name: &str) -> Option<EntityView<'_>> {
        // The view borrows the Program's own name (lifetime `'self`) so the
        // returned EntityView isn't tied to the caller-supplied `name`'s
        // lifetime — letting consumers compare by short-lived string.
        let first = self.defs.iter().find(|d| def_name(d) == name)?;
        let mut view = EntityView::empty(def_name(first));
        for def in &self.defs {
            if def_name(def) == name {
                view.absorb(def);
            }
        }
        Some(view)
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

/// A unified view of every Def in a [`Program`] that shares a name — the
/// concrete shape of #30's "a type is a control with extras". One name maps
/// to one `EntityView` with optional slots; consumers ask for the slots they
/// care about and the view tells them which definer kinds contributed.
///
/// Borrowed view; lifetime tied to the program. See [`Program::entity`].
#[derive(Debug, Clone)]
pub struct EntityView<'a> {
    pub name: &'a str,
    pub function: Option<&'a FunctionDef>,
    pub process: Option<&'a ProcessDef>,
    pub step: Option<&'a StepDef>,
    pub composite: Option<&'a CompositeDef>,
    pub reaction: Option<&'a ReactionDef>,
    pub pattern: Option<&'a PatternDef>,
    pub type_def: Option<&'a TypeDef>,
    pub contract: Option<&'a ContractDef>,
    pub protocol: Option<&'a ProtocolDef>,
    pub unit: Option<&'a UnitDef>,
    pub context: Option<&'a ContextDef>,
    /// `def name [:: T] = expr` — a top-level binding. The pair is
    /// `(optional type ascription, value expression)`.
    pub binding: Option<(&'a Option<SchemaExpr>, &'a Expr)>,
}

/// The OWNED form of `EntityView` — the same shape, cloned. Lets callers
/// build entities programmatically ("start with an empty entity, add slots
/// until it's whatever program you want") and pass them around without
/// borrowing a Program. Pairs with [`Program::entity_owned`] /
/// [`Program::owned_entities`] for the read direction, and with hand-construction
/// via [`EntityDef::new`] + slot-setters for the write direction.
#[derive(Debug, Clone, Default)]
pub struct EntityDef {
    pub name: Name,
    pub function: Option<FunctionDef>,
    pub process: Option<ProcessDef>,
    pub step: Option<StepDef>,
    pub composite: Option<CompositeDef>,
    pub reaction: Option<ReactionDef>,
    pub pattern: Option<PatternDef>,
    pub type_def: Option<TypeDef>,
    pub contract: Option<ContractDef>,
    pub protocol: Option<ProtocolDef>,
    pub unit: Option<UnitDef>,
    pub context: Option<ContextDef>,
    pub binding: Option<(Option<SchemaExpr>, Expr)>,
}

impl EntityDef {
    /// An empty entity — just a name, no slots filled. The user's mental
    /// model "start with an empty Entity, then add things to it until it
    /// was whatever ys program" is *literally* this struct plus the
    /// `with_…` slot-setters below. The unified registry is the answer to
    /// "where do I put the next piece?" — every contribution is one slot.
    pub fn new(name: impl Into<Name>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Slot-builders — append-style, return `Self` so contributions chain.
    pub fn with_function(mut self, def: FunctionDef) -> Self {
        self.function = Some(def);
        self
    }
    pub fn with_process(mut self, def: ProcessDef) -> Self {
        self.process = Some(def);
        self
    }
    pub fn with_step(mut self, def: StepDef) -> Self {
        self.step = Some(def);
        self
    }
    pub fn with_composite(mut self, def: CompositeDef) -> Self {
        self.composite = Some(def);
        self
    }
    pub fn with_reaction(mut self, def: ReactionDef) -> Self {
        self.reaction = Some(def);
        self
    }
    pub fn with_type(mut self, def: TypeDef) -> Self {
        self.type_def = Some(def);
        self
    }
    pub fn with_binding(mut self, schema: Option<SchemaExpr>, value: Expr) -> Self {
        self.binding = Some((schema, value));
        self
    }

    /// True if every slot is empty — a "name-only" placeholder (the bare
    /// `control Foo` case from slice 4, when that lands).
    pub fn is_empty(&self) -> bool {
        self.function.is_none()
            && self.process.is_none()
            && self.step.is_none()
            && self.composite.is_none()
            && self.reaction.is_none()
            && self.pattern.is_none()
            && self.type_def.is_none()
            && self.contract.is_none()
            && self.protocol.is_none()
            && self.unit.is_none()
            && self.context.is_none()
            && self.binding.is_none()
    }

    /// Absorb a Def into the appropriate slot — the owned counterpart of
    /// [`EntityView::absorb`]. Later defs of the same kind overwrite.
    pub fn absorb_def(&mut self, def: &Def) {
        match def {
            Def::Function(d) => self.function = Some(d.clone()),
            Def::Process(d) => self.process = Some(d.clone()),
            Def::Step(d) => self.step = Some(d.clone()),
            Def::Composite(d) => self.composite = Some(d.clone()),
            Def::Reaction(d) => self.reaction = Some(d.clone()),
            Def::Pattern(d) => self.pattern = Some(d.clone()),
            Def::Type(d) => self.type_def = Some(d.clone()),
            Def::Contract(d) => self.contract = Some(d.clone()),
            Def::Protocol(d) => self.protocol = Some(d.clone()),
            Def::Unit(d) => self.unit = Some(d.clone()),
            Def::Context(d) => self.context = Some(d.clone()),
            Def::Binding { schema, value, .. } => {
                self.binding = Some((schema.clone(), value.clone()));
            }
            Def::Use { .. } => {}
        }
    }

    /// True if this entity has a value-form slot — matches
    /// [`EntityView::has_value_form`].
    pub fn has_value_form(&self) -> bool {
        self.function.is_some()
            || self.composite.is_some()
            || self.process.is_some()
            || self.step.is_some()
            || self.reaction.is_some()
            || self.protocol.is_some()
    }

    /// Render this entity as a chrysalis [`Value`] — the homoiconic
    /// summary shape. The structure mirrors the `.ys` surface: name +
    /// which slots are filled + each slot's key structural metadata
    /// (ports, param names). Body Exprs aren't serialized in this slice
    /// (deep AST-as-value is the stretch); names/ports/structure are
    /// enough for "programs as data" inspection / programmatic construction
    /// / round-trip identity at the entity-registry level.
    pub fn to_value(&self) -> prism_schema::Value {
        use indexmap::IndexMap;
        use prism_schema::{Key, Value};
        let mut fields: IndexMap<Key, Value> = IndexMap::new();
        fields.insert(Key::from("_type"), Value::String("EntityDef".into()));
        fields.insert(Key::from("name"), Value::String(self.name.clone()));
        let mut slots: Vec<Value> = Vec::new();
        if let Some(p) = &self.process {
            slots.push(Value::String("process".into()));
            fields.insert(
                Key::from("process"),
                slot_def_to_value(&p.params, &p.interface, &p.body),
            );
        }
        if let Some(s) = &self.step {
            slots.push(Value::String("step".into()));
            fields.insert(
                Key::from("step"),
                slot_def_to_value(&s.params, &s.interface, &s.body),
            );
        }
        if let Some(c) = &self.composite {
            slots.push(Value::String("composite".into()));
            fields.insert(
                Key::from("composite"),
                slot_def_to_value(&c.params, &c.interface, &c.body),
            );
        }
        if let Some(r) = &self.reaction {
            slots.push(Value::String("reaction".into()));
            let mut rmap: IndexMap<Key, Value> = IndexMap::new();
            rmap.insert(
                Key::from("params"),
                Value::List(
                    r.params
                        .iter()
                        .map(|p| Value::String(p.name.clone()))
                        .collect(),
                ),
            );
            rmap.insert(Key::from("redex"), r.redex.to_value());
            rmap.insert(Key::from("reactum"), r.reactum.to_value());
            fields.insert(Key::from("reaction"), Value::Map(rmap));
        }
        if let Some(f) = &self.function {
            slots.push(Value::String("function".into()));
            let mut fmap: IndexMap<Key, Value> = IndexMap::new();
            fmap.insert(
                Key::from("params"),
                Value::List(
                    f.params
                        .iter()
                        .map(|p| Value::String(p.name.clone()))
                        .collect(),
                ),
            );
            fmap.insert(Key::from("body"), f.body.to_value());
            fields.insert(Key::from("function"), Value::Map(fmap));
        }
        if self.type_def.is_some() {
            slots.push(Value::String("type".into()));
        }
        if self.contract.is_some() {
            slots.push(Value::String("contract".into()));
        }
        if self.protocol.is_some() {
            slots.push(Value::String("protocol".into()));
        }
        if self.unit.is_some() {
            slots.push(Value::String("unit".into()));
        }
        if self.context.is_some() {
            slots.push(Value::String("context".into()));
        }
        if let Some((schema, value)) = &self.binding {
            slots.push(Value::String("binding".into()));
            // Emit the binding's VALUE (and optional type) — not just the slot
            // name. A trailing `Environment[…]` entry parses to `def main = …`
            // (a binding), so dropping the value here loses a program's entry /
            // initial state through `quote` (the bug the `definer_equals_its_
            // quote_then_eval` proof drove out). Mirrors the `function` slot.
            let mut bmap: IndexMap<Key, Value> = IndexMap::new();
            bmap.insert(Key::from("value"), value.to_value());
            if let Some(s) = schema {
                bmap.insert(
                    Key::from("schema"),
                    Value::String(crate::unparse::unparse_schema(s)),
                );
            }
            fields.insert(Key::from("binding"), Value::Map(bmap));
        }
        fields.insert(Key::from("slots"), Value::List(slots));
        Value::Map(fields)
    }
}

/// Summarise an `Interface` (input/output port names) as a Value. Keeps the
/// shape readable without recursing into port-schema details.
fn interface_summary(iface: &Interface) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    m.insert(
        Key::from("inputs"),
        Value::List(
            iface
                .inputs
                .keys()
                .map(|k| Value::String(k.clone()))
                .collect(),
        ),
    );
    m.insert(
        Key::from("outputs"),
        Value::List(
            iface
                .outputs
                .keys()
                .map(|k| Value::String(k.clone()))
                .collect(),
        ),
    );
    Value::Map(m)
}

/// Slot summary + body: interface ports plus the body Expr as data. The
/// body is recursed via [`Expr::to_value`] so the whole AST is walkable.
fn slot_with_body(iface: &Interface, body: &Expr) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = match interface_summary(iface) {
        Value::Map(m) => m,
        _ => IndexMap::new(),
    };
    m.insert(Key::from("body"), body.to_value());
    Value::Map(m)
}

/// Full slot serialization (params + per-port full info + body). The
/// **lossless** form — paired with [`slot_def_from_value`] to round-trip a
/// `ProcessDef` / `StepDef` / `CompositeDef`'s contents.
fn slot_def_to_value(params: &[Param], iface: &Interface, body: &Expr) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    m.insert(
        Key::from("params"),
        Value::List(params.iter().map(param_to_value).collect()),
    );
    m.insert(Key::from("inputs"), ports_to_value(&iface.inputs));
    m.insert(Key::from("outputs"), ports_to_value(&iface.outputs));
    m.insert(Key::from("body"), body.to_value());
    Value::Map(m)
}

fn param_to_value(p: &Param) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    m.insert(Key::from("name"), Value::String(p.name.clone()));
    m.insert(
        Key::from("schema"),
        Value::String(crate::unparse::unparse_schema(&p.schema)),
    );
    if let Some(d) = &p.default {
        m.insert(Key::from("default"), d.to_value());
    }
    Value::Map(m)
}

fn ports_to_value(ports: &indexmap::IndexMap<Name, PortDecl>) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    for (k, port) in ports {
        m.insert(Key::from(k.as_str()), port_decl_to_value(port));
    }
    Value::Map(m)
}

fn port_decl_to_value(p: &PortDecl) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    m.insert(
        Key::from("schema"),
        Value::String(crate::unparse::unparse_schema(&p.schema)),
    );
    if let Some(d) = &p.default {
        m.insert(Key::from("default"), d.to_value());
    }
    if let Some(c) = &p.contract {
        m.insert(Key::from("contract"), contract_ref_to_value(c));
    }
    if let Some(b) = &p.bridge {
        m.insert(
            Key::from("bridge"),
            Value::List(b.iter().map(|s| Value::String(s.clone())).collect()),
        );
    }
    Value::Map(m)
}

fn contract_ref_to_value(c: &ContractRef) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    m.insert(Key::from("name"), Value::String(c.name.clone()));
    if !c.pins.is_empty() {
        let mut pins: IndexMap<Key, Value> = IndexMap::new();
        for (k, v) in &c.pins {
            pins.insert(Key::from(k.as_str()), Value::String(v.clone()));
        }
        m.insert(Key::from("pins"), Value::Map(pins));
    }
    Value::Map(m)
}

impl<'a> EntityView<'a> {
    fn empty(name: &'a str) -> Self {
        Self {
            name,
            function: None,
            process: None,
            step: None,
            composite: None,
            reaction: None,
            pattern: None,
            type_def: None,
            contract: None,
            protocol: None,
            unit: None,
            context: None,
            binding: None,
        }
    }

    /// Fill the slot named by `def`'s kind. Later defs of the same kind
    /// overwrite earlier ones (matching today's "first match wins" if callers
    /// scan with [`Program::lookup`], extended to "last contributor wins"
    /// here — semantically a duplicate-name overwrite, which the parser
    /// doesn't yet reject).
    fn absorb(&mut self, def: &'a Def) {
        match def {
            Def::Function(d) => self.function = Some(d),
            Def::Process(d) => self.process = Some(d),
            Def::Step(d) => self.step = Some(d),
            Def::Composite(d) => self.composite = Some(d),
            Def::Reaction(d) => self.reaction = Some(d),
            Def::Pattern(d) => self.pattern = Some(d),
            Def::Type(d) => self.type_def = Some(d),
            Def::Contract(d) => self.contract = Some(d),
            Def::Protocol(d) => self.protocol = Some(d),
            Def::Unit(d) => self.unit = Some(d),
            Def::Context(d) => self.context = Some(d),
            Def::Binding { schema, value, .. } => self.binding = Some((schema, value)),
            // `use` imports are directives, not entities: they don't fill a slot.
            Def::Use { .. } => {}
        }
    }

    /// Methods declared on this entity's type, if any. (Other definer kinds
    /// may eventually attach methods too — composites already have implicit
    /// methods via the algebra; for now only `type Name = … with { … }`
    /// carries explicit methods.)
    pub fn methods(&self) -> &[MethodDef] {
        self.type_def.map_or(&[], |t| t.methods.as_slice())
    }

    /// True if a bare reference (`Expr::Var(name)`) to this entity should
    /// resolve to a value — i.e. some value-form slot is filled. The set
    /// matches the arms of eval's Var lookup.
    pub fn has_value_form(&self) -> bool {
        self.function.is_some()
            || self.composite.is_some()
            || self.process.is_some()
            || self.step.is_some()
            || self.reaction.is_some()
            || self.protocol.is_some()
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
        Def::Protocol(d) => &d.name,
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

    /// `link name :: T = default` — declare a named value-bearing **hyperedge**
    /// (the bigraph link graph, first-class). Lives in a composite body; lowers
    /// to a shared pool slot `name: default` PLUS a `_links` marker on the scope,
    /// so the engine resolves every `~name` attachment up the place graph to that
    /// one slot (depth-independent). The optional `schema` types the slot.
    LinkDecl {
        name: Name,
        schema: Option<SchemaExpr>,
        /// A `mesh` modifier → a REPLICATED (CRDT) link (#62): gated by
        /// `mesh_safety` at compile, replicated across peers at runtime.
        mesh: bool,
        default: Box<Expr>,
    },

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
    /// A comprehension over a list OR a map — the surface iteration construct.
    ///
    /// - `[ body for v in src (if p) ]` ⇒ a **list** comprehension (`key: None`):
    ///   `v` binds each element of a list (or each value of a map); kept bodies
    ///   are collected into a list.
    /// - `{ k: body for v in src (if p) }` ⇒ a **map** comprehension
    ///   (`key: Some(k)`): each kept iteration inserts `k → body`, so later keys
    ///   overwrite earlier ones (a constant key collapses to the last match — how
    ///   the env-side division enactor selects one `_divide` per tick).
    /// - `for kv, v in src` binds the key/index too (`key_var: Some(kv)`): for a
    ///   map `kv` is the entry key, for a list it is the index.
    ///
    /// Makes traversal/queries (a graph type's `neighbors`, the cells-map
    /// division scan) expressible *in chrysalis* — see environment.ys.
    Comprehension {
        /// `for kv, v in …` — the key/index binding, when present.
        key_var: Option<Name>,
        /// The value binding (each element / each map value).
        var: Name,
        source: Box<Expr>,
        filter: Option<Box<Expr>>,
        body: Box<Expr>,
        /// `Some(k)` ⇒ a MAP comprehension keyed by `k` (evaluated per iteration,
        /// must be a string); `None` ⇒ a LIST comprehension.
        key: Option<Box<Expr>>,
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

// =============================================================================
// AST as data — Expr → Value serialization (#32 stretch)
// =============================================================================
//
// Every `Expr` variant renders as `{_type: "<Variant>", …fields}` — a plain
// tree-of-maps that any chrysalis caller can walk, transform, or build by
// hand. Round-trips with [`Expr::from_value`] (subset; see #32). Body Exprs
// inside `EntityDef::to_value`'s slot summaries are recursed via this same
// serializer, so the whole AST is data, all the way down.

impl Expr {
    /// Serialize this expression as a chrysalis Value — the homoiconic AST
    /// shape. Recursive; every sub-expression becomes another `{_type, …}`
    /// map. Inverse: [`Expr::from_value`] (covers the build-up-a-program
    /// subset; full coverage will land alongside compile-from-value).
    pub fn to_value(&self) -> prism_schema::Value {
        use indexmap::IndexMap;
        use prism_schema::{Key, Value};
        let tag = |variant: &str, fields: &[(&str, Value)]| -> Value {
            let mut m: IndexMap<Key, Value> = IndexMap::new();
            m.insert(Key::from("_type"), Value::String(variant.into()));
            for (k, v) in fields {
                m.insert(Key::from(*k), v.clone());
            }
            Value::Map(m)
        };
        let exprs = |es: &[Expr]| -> Value {
            Value::List(es.iter().map(Expr::to_value).collect())
        };
        match self {
            Expr::Unit => tag("Unit", &[]),
            Expr::Bool(b) => tag("Bool", &[("value", Value::Bool(*b))]),
            Expr::Int(n) => tag("Int", &[("value", Value::Int(*n))]),
            Expr::Float(f) => tag("Float", &[("value", Value::float(*f))]),
            Expr::Str(s) => tag("Str", &[("value", string_lit_to_value(s))]),
            Expr::Var(n) => tag("Var", &[("name", Value::String(n.clone()))]),
            Expr::Path(p) => tag("Path", &[("path", place_path_to_value(p))]),
            Expr::Term { control, args, ports, body } => {
                let mut fields: Vec<(&str, Value)> = vec![
                    ("control", Value::String(control.clone())),
                    ("args", Value::List(args.iter().map(term_arg_to_value).collect())),
                    ("ports", port_bindings_to_value(ports)),
                ];
                if let Some(b) = body {
                    fields.push(("body", b.to_value()));
                }
                tag("Term", &fields)
            }
            Expr::Parallel(items) => tag("Parallel", &[("items", exprs(items))]),
            Expr::KeyedEntry { key, value } => tag(
                "KeyedEntry",
                &[("key", string_lit_to_value(key)), ("value", value.to_value())],
            ),
            Expr::Map(entries) => {
                let list: Vec<Value> = entries
                    .iter()
                    .map(|(k, v)| {
                        let mut m: IndexMap<Key, Value> = IndexMap::new();
                        m.insert(Key::from("key"), string_lit_to_value(k));
                        m.insert(Key::from("value"), v.to_value());
                        Value::Map(m)
                    })
                    .collect();
                tag("Map", &[("entries", Value::List(list))])
            }
            Expr::Record(fields) => {
                let mut rec: IndexMap<Key, Value> = IndexMap::new();
                for (k, v) in fields {
                    rec.insert(Key::from(k.as_str()), v.to_value());
                }
                tag("Record", &[("fields", Value::Map(rec))])
            }
            Expr::List(items) => tag("List", &[("items", exprs(items))]),
            Expr::Site { name, sort } => {
                let mut fields: Vec<(&str, Value)> =
                    vec![("name", Value::String(name.clone()))];
                if let Some(s) = sort {
                    fields.push(("sort", s.to_value()));
                }
                tag("Site", &fields)
            }
            Expr::Unbound => tag("Unbound", &[]),
            Expr::LinkVar(n) => tag("LinkVar", &[("name", Value::String(n.clone()))]),
            Expr::LinkDecl { name, schema, mesh, default } => {
                let mut fields: Vec<(&str, Value)> = vec![
                    ("name", Value::String(name.clone())),
                    ("default", default.to_value()),
                ];
                if let Some(s) = schema {
                    fields.push((
                        "schema",
                        Value::String(crate::unparse::unparse_schema(s)),
                    ));
                }
                if *mesh {
                    fields.push(("mesh", Value::Bool(true)));
                }
                tag("LinkDecl", &fields)
            }
            Expr::Rule { redex, reactum } => tag(
                "Rule",
                &[("redex", redex.to_value()), ("reactum", reactum.to_value())],
            ),
            Expr::Let { bindings, body } => tag(
                "Let",
                &[
                    ("bindings", named_bindings_to_value(bindings)),
                    ("body", body.to_value()),
                ],
            ),
            Expr::Block(b) => tag(
                "Block",
                &[
                    ("bindings", named_bindings_to_value(&b.bindings)),
                    ("value", b.value.to_value()),
                ],
            ),
            Expr::If { cond, then_, else_ } => {
                let mut fields: Vec<(&str, Value)> = vec![
                    ("cond", cond.to_value()),
                    ("then", then_.to_value()),
                ];
                if let Some(e) = else_ {
                    fields.push(("else", e.to_value()));
                }
                tag("If", &fields)
            }
            Expr::BinOp { op, lhs, rhs } => tag(
                "BinOp",
                &[
                    ("op", Value::String(binop_tag(*op).into())),
                    ("lhs", lhs.to_value()),
                    ("rhs", rhs.to_value()),
                ],
            ),
            Expr::UnaryOp { op, operand } => tag(
                "UnaryOp",
                &[
                    ("op", Value::String(unaryop_tag(*op).into())),
                    ("operand", operand.to_value()),
                ],
            ),
            Expr::Method { receiver, method, args } => tag(
                "Method",
                &[
                    ("receiver", receiver.to_value()),
                    ("method", Value::String(method.clone())),
                    ("args", exprs(args)),
                ],
            ),
            Expr::Field { base, name } => tag(
                "Field",
                &[
                    ("base", base.to_value()),
                    ("name", Value::String(name.clone())),
                ],
            ),
            Expr::Call { func, args } => tag(
                "Call",
                &[("func", func.to_value()), ("args", exprs(args))],
            ),
            Expr::Comprehension {
                key_var,
                var,
                source,
                filter,
                body,
                key,
            } => {
                let mut fields: Vec<(&str, Value)> = vec![
                    ("var", Value::String(var.clone())),
                    ("source", source.to_value()),
                    ("body", body.to_value()),
                ];
                if let Some(k) = key_var {
                    fields.push(("key_var", Value::String(k.clone())));
                }
                if let Some(f) = filter {
                    fields.push(("filter", f.to_value()));
                }
                if let Some(k) = key {
                    fields.push(("key", k.to_value()));
                }
                tag("Comprehension", &fields)
            }
            Expr::ReplaceWith { id, with } => tag(
                "ReplaceWith",
                &[("id", id.to_value()), ("with", with.to_value())],
            ),
            Expr::Where { inner, predicate } => tag(
                "Where",
                &[
                    ("inner", inner.to_value()),
                    ("predicate", predicate.to_value()),
                ],
            ),
        }
    }
}

fn place_path_to_value(p: &PlacePath) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    m.insert(
        Key::from("root"),
        Value::String(
            match &p.root {
                PathRoot::Here => "Here".into(),
                PathRoot::Parent => "Parent".into(),
                PathRoot::Local(n) => format!("Local({n})"),
            },
        ),
    );
    m.insert(
        Key::from("segments"),
        Value::List(p.segments.iter().map(|s| Value::String(s.clone())).collect()),
    );
    Value::Map(m)
}

fn string_lit_to_value(s: &StringLit) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    if let Some(plain) = s.as_plain() {
        return Value::String(plain);
    }
    // Template literal: emit segments as a list of {kind, …}.
    let segs: Vec<Value> = s
        .segments
        .iter()
        .map(|seg| {
            let mut m: IndexMap<Key, Value> = IndexMap::new();
            match seg {
                StringSeg::Lit(s) => {
                    m.insert(Key::from("kind"), Value::String("lit".into()));
                    m.insert(Key::from("text"), Value::String(s.clone()));
                }
                StringSeg::Expr(e) => {
                    m.insert(Key::from("kind"), Value::String("expr".into()));
                    m.insert(Key::from("expr"), e.to_value());
                }
            }
            Value::Map(m)
        })
        .collect();
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    m.insert(Key::from("_type"), Value::String("Template".into()));
    m.insert(Key::from("segments"), Value::List(segs));
    Value::Map(m)
}

fn term_arg_to_value(a: &TermArg) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    match a {
        TermArg::Positional(e) => {
            m.insert(Key::from("kind"), Value::String("positional".into()));
            m.insert(Key::from("value"), e.to_value());
        }
        TermArg::Named { name, value } => {
            m.insert(Key::from("kind"), Value::String("named".into()));
            m.insert(Key::from("name"), Value::String(name.clone()));
            m.insert(Key::from("value"), value.to_value());
        }
    }
    Value::Map(m)
}

fn port_bindings_to_value(p: &PortBindings) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    let mut bindings_for = |fields: &indexmap::IndexMap<Name, Expr>| -> Value {
        let mut bm: IndexMap<Key, Value> = IndexMap::new();
        for (k, v) in fields {
            bm.insert(Key::from(k.as_str()), v.to_value());
        }
        Value::Map(bm)
    };
    m.insert(Key::from("inputs"), bindings_for(&p.inputs));
    m.insert(Key::from("outputs"), bindings_for(&p.outputs));
    Value::Map(m)
}

fn named_bindings_to_value(bs: &[(Name, Expr)]) -> prism_schema::Value {
    use indexmap::IndexMap;
    use prism_schema::{Key, Value};
    let mut m: IndexMap<Key, Value> = IndexMap::new();
    for (n, e) in bs {
        m.insert(Key::from(n.as_str()), e.to_value());
    }
    Value::Map(m)
}

fn binop_tag(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "Add",
        BinOp::Sub => "Sub",
        BinOp::Mul => "Mul",
        BinOp::Div => "Div",
        BinOp::Eq => "Eq",
        BinOp::Ne => "Ne",
        BinOp::Lt => "Lt",
        BinOp::Le => "Le",
        BinOp::Gt => "Gt",
        BinOp::Ge => "Ge",
        BinOp::And => "And",
        BinOp::Or => "Or",
        BinOp::Concat => "Concat",
        BinOp::In => "In",
    }
}

fn unaryop_tag(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "Neg",
        UnaryOp::Not => "Not",
    }
}

// ── Expr ← Value: the inverse direction (round-trip with `to_value`) ──
//
// Lets a chrysalis program receive an AST shape as data (`load(path)._entities`,
// or a hand-constructed map literal) and feed it back into the runtime as a
// real Expr — the tier-2 substrate: process bodies as first-class values you
// can build at runtime, transform, and run. Coverage focuses on the common
// body variants (literals / Var / Term / Map / Record / Parallel / BinOp /
// Method / Call / etc.); rare/pattern-only variants return an explicit
// "not-yet-supported" error so the gap is visible, not silent.

/// Error from [`Expr::from_value`] — which variant or field failed to parse,
/// and why.
#[derive(Debug, Clone)]
pub struct ExprFromValueError(pub String);

impl std::fmt::Display for ExprFromValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Expr::from_value: {}", self.0)
    }
}

impl std::error::Error for ExprFromValueError {}

impl Expr {
    /// Materialize an Expr from its `to_value` shape. Inverse of
    /// [`Expr::to_value`] over the supported subset (common body variants);
    /// rare variants return [`ExprFromValueError`]. The shape every variant
    /// expects is `{_type: "<Variant>", …fields}`.
    pub fn from_value(v: &prism_schema::Value) -> Result<Expr, ExprFromValueError> {
        let map = v.as_map().ok_or_else(|| err("expected a Map"))?;
        let tag = map
            .get("_type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| err("missing `_type`"))?;
        match tag {
            "Unit" => Ok(Expr::Unit),
            "Bool" => {
                let b = field_of(map, "value")
                    .and_then(|v| v.as_bool())
                    .ok_or_else(|| err("Bool.value must be a Bool"))?;
                Ok(Expr::Bool(b))
            }
            "Int" => {
                let n = field_of(map, "value")
                    .and_then(|v| v.as_i64())
                    .ok_or_else(|| err("Int.value must be an Int"))?;
                Ok(Expr::Int(n))
            }
            "Float" => {
                let f = field_of(map, "value")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| err("Float.value must be a Float"))?;
                Ok(Expr::Float(f))
            }
            "Str" => {
                let s = field_of(map, "value").ok_or_else(|| err("Str.value missing"))?;
                Ok(Expr::Str(string_lit_from_value(s)?))
            }
            "Var" => {
                let name = field_str(map, "name")?;
                Ok(Expr::Var(name))
            }
            "Path" => {
                let p = field_of(map, "path").ok_or_else(|| err("Path.path missing"))?;
                Ok(Expr::Path(place_path_from_value(p)?))
            }
            "Term" => {
                let control = field_str(map, "control")?;
                let args = field_of(map, "args")
                    .and_then(|v| v.as_list())
                    .ok_or_else(|| err("Term.args must be a List"))?
                    .iter()
                    .map(term_arg_from_value)
                    .collect::<Result<Vec<_>, _>>()?;
                let ports = field_of(map, "ports")
                    .map(port_bindings_from_value)
                    .unwrap_or_else(|| Ok(PortBindings::default()))?;
                let body = field_of(map, "body")
                    .map(|b| Expr::from_value(b).map(Box::new))
                    .transpose()?;
                Ok(Expr::Term { control, args, ports, body })
            }
            "Parallel" => Ok(Expr::Parallel(exprs_from_value_field(map, "items")?)),
            "List" => Ok(Expr::List(exprs_from_value_field(map, "items")?)),
            "KeyedEntry" => {
                let key = string_lit_from_value(
                    field_of(map, "key").ok_or_else(|| err("KeyedEntry.key missing"))?,
                )?;
                let value = Expr::from_value(
                    field_of(map, "value").ok_or_else(|| err("KeyedEntry.value missing"))?,
                )?;
                Ok(Expr::KeyedEntry { key, value: Box::new(value) })
            }
            "Map" => {
                let entries = field_of(map, "entries")
                    .and_then(|v| v.as_list())
                    .ok_or_else(|| err("Map.entries must be a List"))?;
                let mut out: Vec<(StringLit, Expr)> = Vec::with_capacity(entries.len());
                for e in entries {
                    let m = e.as_map().ok_or_else(|| err("Map entry must be a Map"))?;
                    let k = string_lit_from_value(
                        m.get("key").ok_or_else(|| err("Map entry.key missing"))?,
                    )?;
                    let v = Expr::from_value(
                        m.get("value").ok_or_else(|| err("Map entry.value missing"))?,
                    )?;
                    out.push((k, v));
                }
                Ok(Expr::Map(out))
            }
            "Record" => {
                let fields = field_of(map, "fields")
                    .and_then(|v| v.as_map())
                    .ok_or_else(|| err("Record.fields must be a Map"))?;
                let mut out: indexmap::IndexMap<Name, Expr> = indexmap::IndexMap::new();
                for (k, v) in fields {
                    out.insert(k.to_string(), Expr::from_value(v)?);
                }
                Ok(Expr::Record(out))
            }
            "Block" => {
                let bindings = field_of(map, "bindings")
                    .map(named_bindings_from_value)
                    .unwrap_or_else(|| Ok(Vec::new()))?;
                let value = Expr::from_value(
                    field_of(map, "value").ok_or_else(|| err("Block.value missing"))?,
                )?;
                Ok(Expr::Block(Block::from_parts(bindings, value)))
            }
            "Let" => {
                let bindings = field_of(map, "bindings")
                    .map(named_bindings_from_value)
                    .unwrap_or_else(|| Ok(Vec::new()))?;
                let body = Expr::from_value(
                    field_of(map, "body").ok_or_else(|| err("Let.body missing"))?,
                )?;
                Ok(Expr::Let { bindings, body: Box::new(body) })
            }
            "If" => {
                let cond = Expr::from_value(
                    field_of(map, "cond").ok_or_else(|| err("If.cond missing"))?,
                )?;
                let then_ = Expr::from_value(
                    field_of(map, "then").ok_or_else(|| err("If.then missing"))?,
                )?;
                let else_ = field_of(map, "else")
                    .map(|e| Expr::from_value(e).map(Box::new))
                    .transpose()?;
                Ok(Expr::If {
                    cond: Box::new(cond),
                    then_: Box::new(then_),
                    else_,
                })
            }
            "BinOp" => {
                let op = binop_from_tag(&field_str(map, "op")?)?;
                let lhs = Expr::from_value(
                    field_of(map, "lhs").ok_or_else(|| err("BinOp.lhs missing"))?,
                )?;
                let rhs = Expr::from_value(
                    field_of(map, "rhs").ok_or_else(|| err("BinOp.rhs missing"))?,
                )?;
                Ok(Expr::BinOp {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                })
            }
            "UnaryOp" => {
                let op = unaryop_from_tag(&field_str(map, "op")?)?;
                let operand = Expr::from_value(
                    field_of(map, "operand").ok_or_else(|| err("UnaryOp.operand missing"))?,
                )?;
                Ok(Expr::UnaryOp { op, operand: Box::new(operand) })
            }
            "Method" => {
                let receiver = Expr::from_value(
                    field_of(map, "receiver").ok_or_else(|| err("Method.receiver missing"))?,
                )?;
                let method = field_str(map, "method")?;
                let args = exprs_from_value_field(map, "args")?;
                Ok(Expr::Method {
                    receiver: Box::new(receiver),
                    method,
                    args,
                })
            }
            "Field" => {
                let base = Expr::from_value(
                    field_of(map, "base").ok_or_else(|| err("Field.base missing"))?,
                )?;
                let name = field_str(map, "name")?;
                Ok(Expr::Field { base: Box::new(base), name })
            }
            "Call" => {
                let func = Expr::from_value(
                    field_of(map, "func").ok_or_else(|| err("Call.func missing"))?,
                )?;
                let args = exprs_from_value_field(map, "args")?;
                Ok(Expr::Call { func: Box::new(func), args })
            }
            "Unbound" => Ok(Expr::Unbound),
            "LinkVar" => Ok(Expr::LinkVar(field_str(map, "name")?)),
            "LinkDecl" => {
                let name = field_str(map, "name")?;
                let default = Box::new(Expr::from_value(
                    field_of(map, "default").ok_or_else(|| err("LinkDecl.default missing"))?,
                )?);
                let schema = match map.get("schema").and_then(|v| v.as_str()) {
                    Some(s) => Some(
                        crate::parse::parse_schema_expr(s)
                            .map_err(|e| err(&format!("LinkDecl.schema: {e:?}")))?,
                    ),
                    None => None,
                };
                let mesh = map.get("mesh").and_then(|v| v.as_bool()).unwrap_or(false);
                Ok(Expr::LinkDecl { name, schema, mesh, default })
            }
            // Pattern / reaction-defining variants — `?c` sites, the `=>` rule
            // split, the guard. Closing these is what makes a REACTION fully
            // assemblable as data: `{_type:"Rule", redex:{…}, reactum:{…}}` round-
            // trips to an `Expr::Rule`, which compiles (`eval_pattern` →
            // `ReactionRule`) and runs. The reaction analog of hand-built-cell.
            "Site" => {
                let name = field_str(map, "name")?;
                let sort = match field_of(map, "sort") {
                    Some(s) => Some(Box::new(Expr::from_value(s)?)),
                    None => None,
                };
                Ok(Expr::Site { name, sort })
            }
            "Rule" => {
                let redex = Box::new(Expr::from_value(
                    field_of(map, "redex").ok_or_else(|| err("Rule.redex missing"))?,
                )?);
                let reactum = Box::new(Expr::from_value(
                    field_of(map, "reactum").ok_or_else(|| err("Rule.reactum missing"))?,
                )?);
                Ok(Expr::Rule { redex, reactum })
            }
            "ReplaceWith" => {
                let id = Box::new(Expr::from_value(
                    field_of(map, "id").ok_or_else(|| err("ReplaceWith.id missing"))?,
                )?);
                let with = Box::new(Expr::from_value(
                    field_of(map, "with").ok_or_else(|| err("ReplaceWith.with missing"))?,
                )?);
                Ok(Expr::ReplaceWith { id, with })
            }
            "Where" => {
                let inner = Box::new(Expr::from_value(
                    field_of(map, "inner").ok_or_else(|| err("Where.inner missing"))?,
                )?);
                let predicate = Box::new(Expr::from_value(
                    field_of(map, "predicate").ok_or_else(|| err("Where.predicate missing"))?,
                )?);
                Ok(Expr::Where { inner, predicate })
            }
            // The list/map iteration construct. Closing it makes `quote`/`reify`
            // (`to_value`/`from_value`) a TOTAL inverse iso over every `Expr`
            // variant — the keystone of homoiconic-unification Stage 1a: with the
            // round-trip total, `eval(quote(e)) = eval(e)` for *every* `e`, so a
            // quoted comprehension (and any program/reaction containing one)
            // survives the data round-trip. Mirrors the `to_value` arm (`:1784`).
            "Comprehension" => {
                let var = field_str(map, "var")?;
                let source = Box::new(Expr::from_value(
                    field_of(map, "source").ok_or_else(|| err("Comprehension.source missing"))?,
                )?);
                let body = Box::new(Expr::from_value(
                    field_of(map, "body").ok_or_else(|| err("Comprehension.body missing"))?,
                )?);
                let key_var = map.get("key_var").and_then(|v| v.as_str()).map(|s| s.to_string());
                let filter = field_of(map, "filter")
                    .map(|f| Expr::from_value(f).map(Box::new))
                    .transpose()?;
                let key = field_of(map, "key")
                    .map(|k| Expr::from_value(k).map(Box::new))
                    .transpose()?;
                Ok(Expr::Comprehension { key_var, var, source, filter, body, key })
            }
            other => Err(err(&format!(
                "unknown `_type` `{other}` in Expr::from_value"
            ))),
        }
    }
}

fn err(msg: &str) -> ExprFromValueError {
    ExprFromValueError(msg.to_string())
}

fn field_of<'a>(
    map: &'a indexmap::IndexMap<prism_schema::Key, prism_schema::Value>,
    key: &str,
) -> Option<&'a prism_schema::Value> {
    map.get(key)
}

fn field_str(
    map: &indexmap::IndexMap<prism_schema::Key, prism_schema::Value>,
    key: &str,
) -> Result<String, ExprFromValueError> {
    map.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| err(&format!("missing string field `{key}`")))
}

fn exprs_from_value_field(
    map: &indexmap::IndexMap<prism_schema::Key, prism_schema::Value>,
    key: &str,
) -> Result<Vec<Expr>, ExprFromValueError> {
    let list = map
        .get(key)
        .and_then(|v| v.as_list())
        .ok_or_else(|| err(&format!("field `{key}` must be a List")))?;
    list.iter().map(Expr::from_value).collect()
}

fn string_lit_from_value(v: &prism_schema::Value) -> Result<StringLit, ExprFromValueError> {
    // A plain string is the shorthand `to_value` form for non-template literals.
    if let Some(s) = v.as_str() {
        return Ok(StringLit::plain(s));
    }
    // Template: `{_type: "Template", segments: [{kind: lit|expr, …}]}`.
    let map = v
        .as_map()
        .ok_or_else(|| err("StringLit must be a string or template Map"))?;
    let segs = map
        .get("segments")
        .and_then(|v| v.as_list())
        .ok_or_else(|| err("Template.segments must be a List"))?;
    let mut out: Vec<StringSeg> = Vec::with_capacity(segs.len());
    for seg in segs {
        let m = seg.as_map().ok_or_else(|| err("template segment must be a Map"))?;
        match m.get("kind").and_then(|v| v.as_str()) {
            Some("lit") => {
                let text = m
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| err("lit segment.text missing"))?;
                out.push(StringSeg::Lit(text.to_string()));
            }
            Some("expr") => {
                let e = m
                    .get("expr")
                    .ok_or_else(|| err("expr segment.expr missing"))?;
                out.push(StringSeg::Expr(Expr::from_value(e)?));
            }
            other => return Err(err(&format!("unknown segment kind: {other:?}"))),
        }
    }
    Ok(StringLit::template(out))
}

fn place_path_from_value(v: &prism_schema::Value) -> Result<PlacePath, ExprFromValueError> {
    let m = v.as_map().ok_or_else(|| err("PlacePath must be a Map"))?;
    let root_str = m
        .get("root")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("PlacePath.root missing"))?;
    let root = match root_str {
        "Here" => PathRoot::Here,
        "Parent" => PathRoot::Parent,
        s if s.starts_with("Local(") && s.ends_with(')') => {
            PathRoot::Local(s[6..s.len() - 1].to_string())
        }
        other => return Err(err(&format!("unknown PathRoot tag: {other:?}"))),
    };
    let segments = m
        .get("segments")
        .and_then(|v| v.as_list())
        .ok_or_else(|| err("PlacePath.segments must be a List"))?
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    Ok(PlacePath { root, segments })
}

fn term_arg_from_value(v: &prism_schema::Value) -> Result<TermArg, ExprFromValueError> {
    let m = v.as_map().ok_or_else(|| err("TermArg must be a Map"))?;
    let kind = m
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("TermArg.kind missing"))?;
    let value = Expr::from_value(
        m.get("value").ok_or_else(|| err("TermArg.value missing"))?,
    )?;
    match kind {
        "positional" => Ok(TermArg::Positional(value)),
        "named" => {
            let name = m
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| err("named TermArg.name missing"))?;
            Ok(TermArg::Named { name, value })
        }
        other => Err(err(&format!("unknown TermArg kind: {other:?}"))),
    }
}

fn port_bindings_from_value(
    v: &prism_schema::Value,
) -> Result<PortBindings, ExprFromValueError> {
    let m = v.as_map().ok_or_else(|| err("PortBindings must be a Map"))?;
    let parse = |key: &str| -> Result<indexmap::IndexMap<Name, Expr>, ExprFromValueError> {
        let mut out: indexmap::IndexMap<Name, Expr> = indexmap::IndexMap::new();
        if let Some(bindings) = m.get(key).and_then(|v| v.as_map()) {
            for (k, v) in bindings {
                out.insert(k.to_string(), Expr::from_value(v)?);
            }
        }
        Ok(out)
    };
    Ok(PortBindings {
        inputs: parse("inputs")?,
        outputs: parse("outputs")?,
    })
}

fn named_bindings_from_value(
    v: &prism_schema::Value,
) -> Result<Vec<(Name, Expr)>, ExprFromValueError> {
    let m = v.as_map().ok_or_else(|| err("bindings must be a Map"))?;
    let mut out: Vec<(Name, Expr)> = Vec::with_capacity(m.len());
    for (k, v) in m {
        out.push((k.to_string(), Expr::from_value(v)?));
    }
    Ok(out)
}

fn binop_from_tag(tag: &str) -> Result<BinOp, ExprFromValueError> {
    Ok(match tag {
        "Add" => BinOp::Add,
        "Sub" => BinOp::Sub,
        "Mul" => BinOp::Mul,
        "Div" => BinOp::Div,
        "Eq" => BinOp::Eq,
        "Ne" => BinOp::Ne,
        "Lt" => BinOp::Lt,
        "Le" => BinOp::Le,
        "Gt" => BinOp::Gt,
        "Ge" => BinOp::Ge,
        "And" => BinOp::And,
        "Or" => BinOp::Or,
        "Concat" => BinOp::Concat,
        "In" => BinOp::In,
        other => return Err(err(&format!("unknown BinOp tag: {other:?}"))),
    })
}

fn unaryop_from_tag(tag: &str) -> Result<UnaryOp, ExprFromValueError> {
    Ok(match tag {
        "Neg" => UnaryOp::Neg,
        "Not" => UnaryOp::Not,
        other => return Err(err(&format!("unknown UnaryOp tag: {other:?}"))),
    })
}

// ── EntityDef ← Value: the lossless round-trip (#34 slice A) ──
//
// A hand-built `{_type: "EntityDef", name, process: {…}, …}` becomes a real
// EntityDef. With this, a `.ys` caller can build a Cell composite from map
// literals + map literals for its body + map literals for the inner Grow's
// expression — and feed it to compile + run. The pre-req for "run the
// streaming environment with a HAND-CONSTRUCTED Cell".

impl Program {
    /// Materialize a `Program` from its [`Self::to_value`] shape — the
    /// inverse direction. Each entity in the value becomes the appropriate
    /// `Def` variant (Process / Step / Composite / …) in the resulting
    /// program. With this, a `.ys` caller can build a whole program from map
    /// literals and feed it through compile + run (#34 slice B's substrate).
    pub fn from_value(v: &prism_schema::Value) -> Result<Program, ExprFromValueError> {
        let map = v
            .as_map()
            .ok_or_else(|| err("Program must be a Map"))?;
        let tag = map
            .get("_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err("Program missing _type"))?;
        if tag != "Program" {
            return Err(err(&format!(
                "expected _type='Program', got {tag:?}"
            )));
        }
        let entities = map
            .get("entities")
            .and_then(|v| v.as_list())
            .ok_or_else(|| err("Program.entities must be a List"))?;
        let mut prog = Program::new();
        for ev in entities {
            let ent = EntityDef::from_value(ev)?;
            // An entity contributes ONE Def per filled slot. Priority is
            // arbitrary today (composite/process/step/reaction) since one
            // entity-Value typically fills one slot; multi-slot entities
            // contribute multiple defs in this order.
            if let Some(p) = ent.process {
                prog.push(Def::Process(p));
            }
            if let Some(s) = ent.step {
                prog.push(Def::Step(s));
            }
            if let Some(c) = ent.composite {
                prog.push(Def::Composite(c));
            }
            // A value binding — incl. a program's trailing `main` entry, which
            // carries the initial state. Without this a whole program's entry is
            // lost through `quote ↔ reify` (the `definer_equals_its_quote_then_eval`
            // proof). Reaction / Function / unit / … — add as their slot
            // serializations round-trip.
            if let Some((schema, value)) = ent.binding {
                prog.push(Def::Binding {
                    name: ent.name.clone(),
                    schema,
                    value,
                });
            }
        }
        Ok(prog)
    }
}

impl EntityDef {
    /// Materialize an `EntityDef` from its [`Self::to_value`] shape. Inverse
    /// of `to_value` over the slots that have round-trippable serializations
    /// today: `process`, `step`, `composite`. Other slots are recognized but
    /// not yet round-trippable (`from_value` ignores them; full coverage
    /// expands as needed).
    pub fn from_value(v: &prism_schema::Value) -> Result<EntityDef, ExprFromValueError> {
        let map = v
            .as_map()
            .ok_or_else(|| err("EntityDef must be a Map"))?;
        let tag = map
            .get("_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err("EntityDef missing _type"))?;
        if tag != "EntityDef" {
            return Err(err(&format!(
                "expected _type='EntityDef', got {tag:?}"
            )));
        }
        let name = map
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| err("EntityDef.name missing"))?
            .to_string();
        let mut ent = EntityDef::new(name.clone());
        if let Some(p) = map.get("process") {
            let (params, interface, body) = slot_def_from_value(p)?;
            ent.process = Some(ProcessDef {
                name: name.clone(),
                params,
                interface,
                body,
            });
        }
        if let Some(s) = map.get("step") {
            let (params, interface, body) = slot_def_from_value(s)?;
            ent.step = Some(StepDef {
                name: name.clone(),
                params,
                interface,
                body,
            });
        }
        if let Some(c) = map.get("composite") {
            let (params, interface, body) = slot_def_from_value(c)?;
            ent.composite = Some(CompositeDef {
                name: name.clone(),
                params,
                using: Vec::new(),
                interface,
                body,
            });
        }
        // A value binding (`def X = expr`, incl. a program's trailing `main`
        // entry). Reconstructing it is what lets a whole program — its definers
        // AND its entry/initial-state — survive `quote ↔ reify ↔ run`
        // (`definer_equals_its_quote_then_eval`). The inverse of `to_value`'s
        // `binding` emission.
        if let Some(b) = map.get("binding") {
            let bm = b.as_map().ok_or_else(|| err("EntityDef.binding must be a Map"))?;
            let value = Expr::from_value(
                bm.get("value").ok_or_else(|| err("binding.value missing"))?,
            )?;
            let schema = match bm.get("schema").and_then(|v| v.as_str()) {
                Some(s) => Some(
                    crate::parse::parse_schema_expr(s)
                        .map_err(|e| err(&format!("binding.schema: {e:?}")))?,
                ),
                None => None,
            };
            ent.binding = Some((schema, value));
        }
        Ok(ent)
    }
}

/// Parse a slot's full serialization (`params`, `inputs`, `outputs`, `body`)
/// — the inverse of [`slot_def_to_value`].
fn slot_def_from_value(
    v: &prism_schema::Value,
) -> Result<(Vec<Param>, Interface, Expr), ExprFromValueError> {
    let m = v.as_map().ok_or_else(|| err("slot must be a Map"))?;
    let params = m
        .get("params")
        .and_then(|v| v.as_list())
        .map(|list| list.iter().map(param_from_value).collect::<Result<Vec<_>, _>>())
        .unwrap_or_else(|| Ok(Vec::new()))?;
    let inputs = m
        .get("inputs")
        .map(ports_from_value)
        .unwrap_or_else(|| Ok(indexmap::IndexMap::new()))?;
    let outputs = m
        .get("outputs")
        .map(ports_from_value)
        .unwrap_or_else(|| Ok(indexmap::IndexMap::new()))?;
    let body = m
        .get("body")
        .map(Expr::from_value)
        .unwrap_or(Ok(Expr::Unit))?;
    Ok((params, Interface { inputs, outputs }, body))
}

fn param_from_value(v: &prism_schema::Value) -> Result<Param, ExprFromValueError> {
    let m = v.as_map().ok_or_else(|| err("Param must be a Map"))?;
    let name = m
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("Param.name missing"))?
        .to_string();
    let schema_src = m
        .get("schema")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("Param.schema must be a string"))?;
    let schema = crate::parse::parse_schema_expr(schema_src)
        .map_err(|e| err(&format!("Param.schema parse: {e}")))?;
    let default = m
        .get("default")
        .map(Expr::from_value)
        .transpose()?;
    Ok(Param {
        name,
        schema,
        default,
    })
}

fn ports_from_value(
    v: &prism_schema::Value,
) -> Result<indexmap::IndexMap<Name, PortDecl>, ExprFromValueError> {
    let m = v.as_map().ok_or_else(|| err("ports must be a Map"))?;
    let mut out: indexmap::IndexMap<Name, PortDecl> = indexmap::IndexMap::new();
    for (k, v) in m {
        out.insert(k.to_string(), port_decl_from_value(v)?);
    }
    Ok(out)
}

fn port_decl_from_value(v: &prism_schema::Value) -> Result<PortDecl, ExprFromValueError> {
    let m = v.as_map().ok_or_else(|| err("PortDecl must be a Map"))?;
    let schema_src = m
        .get("schema")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("PortDecl.schema must be a string"))?;
    let schema = crate::parse::parse_schema_expr(schema_src)
        .map_err(|e| err(&format!("PortDecl.schema parse: {e}")))?;
    let default = m
        .get("default")
        .map(Expr::from_value)
        .transpose()?;
    let contract = m
        .get("contract")
        .map(contract_ref_from_value)
        .transpose()?;
    let bridge = m.get("bridge").map(|b| {
        b.as_list()
            .map(|l| {
                l.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    });
    Ok(PortDecl {
        schema,
        default,
        contract,
        bridge,
    })
}

fn contract_ref_from_value(
    v: &prism_schema::Value,
) -> Result<ContractRef, ExprFromValueError> {
    let m = v
        .as_map()
        .ok_or_else(|| err("ContractRef must be a Map"))?;
    let name = m
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err("ContractRef.name missing"))?
        .to_string();
    let mut pins: indexmap::IndexMap<Name, Name> = indexmap::IndexMap::new();
    if let Some(pmap) = m.get("pins").and_then(|v| v.as_map()) {
        for (k, v) in pmap {
            if let Some(s) = v.as_str() {
                pins.insert(k.to_string(), s.to_string());
            }
        }
    }
    Ok(ContractRef { name, pins })
}
