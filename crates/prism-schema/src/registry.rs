//! Type registry for dynamic schema lookup and process registration.
//!
//! Mirrors bigraph-schema's Core — a central place where types and
//! process factories are registered, enabling runtime composition
//! of simulations from configuration.
//!
//! ## Rich type method dispatch (Stage RT.2, 2026-05-13)
//!
//! Each registered type may carry a `TypeMethods` implementation
//! providing the six lean axioms — `default`, `apply`, `divide`,
//! `serialize`, `realize`, `check`. The registry walks inheritance
//! chains when a type lacks a direct method impl.

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;

use crate::schema::{json_to_value, value_to_json, Schema};
use crate::value::{Foreign, Key, StateMap, Value};

/// Context for `TypeMethods::divide` — informs partitioning strategy.
/// Different types interpret it differently: mesh types read a
/// per-element partition (vertex i → daughter 0 or 1); extensive
/// scalars halve unconditionally; intensive scalars copy.
///
/// Lean v1: just a string-keyed partition assignment. Extend with
/// fields as real biology pulls them in (e.g. an RNG seed for
/// stochastic divides, a sister-chromatid map, etc.).
#[derive(Clone, Debug, Default)]
pub struct DivideContext {
    /// Per-key partition assignment (e.g. "domain_0" → 0). Empty for
    /// default-strategy types that don't need it.
    pub partition: IndexMap<String, usize>,
    /// Number of daughters (default 2).
    pub n_daughters: usize,
}

impl DivideContext {
    pub fn binary() -> Self {
        Self {
            partition: IndexMap::new(),
            n_daughters: 2,
        }
    }
}

/// The six lean axioms of rich-type dispatch (RT.2). Implementations
/// register against a named type via `TypeRegistry::register_methods`.
///
/// Each method takes a reference to the `TypeRegistry` so impls can
/// recursively dispatch to nested types or to parent types in an
/// inheritance chain.
pub trait TypeMethods: Send + Sync {
    /// Initial value for this type.
    fn default(&self, registry: &TypeRegistry, schema: &Schema) -> Value;

    /// Merge an update into state. Semantics per-type: additive for
    /// numeric, structural rewrite for meshes, etc.
    fn apply(
        &self,
        registry: &TypeRegistry,
        schema: &Schema,
        state: &Value,
        update: &Value,
    ) -> Value;

    /// Partition state into N daughter states.
    fn divide(
        &self,
        registry: &TypeRegistry,
        schema: &Schema,
        state: &Value,
        ctx: &DivideContext,
    ) -> Vec<Value>;

    /// Encode state to a JSON-portable Value (using only the data
    /// variants — `None`, `Bool`, `Int`, `Float`, `String`, `List`,
    /// `Map`).
    fn serialize(&self, registry: &TypeRegistry, schema: &Schema, state: &Value) -> Value;

    /// Decode a JSON-portable Value back to typed state (the inverse
    /// of `serialize`). Used to reconstruct state from saved files.
    fn realize(&self, registry: &TypeRegistry, schema: &Schema, encoded: &Value) -> Value;

    /// Validate that a value conforms to this type's invariants.
    /// Default impl returns true.
    fn check(&self, _registry: &TypeRegistry, _schema: &Schema, _state: &Value) -> bool {
        true
    }

    /// Combine two values of this type into one — the **inverse of
    /// `divide`** (the merge / `fold` direction). Default impl is the
    /// schema-driven `tensor_by_schema` over the type's registered
    /// schema (`Float` shares, `Delta`/`Integer` sum, containers
    /// recurse per field). Rich types override for type-specific
    /// composition — e.g. `Qubits.tensor` is the quantum cross-product
    /// over basis bitstrings.
    fn tensor(
        &self,
        registry: &TypeRegistry,
        schema: &Schema,
        a: &Value,
        b: &Value,
    ) -> Value {
        tensor_by_schema(schema, a, b, registry)
    }
}

/// A registered type: its schema, optional default value, optional
/// method dispatch impl, and inheritance chain.
#[derive(Clone)]
pub struct TypeEntry {
    pub schema: Schema,
    pub default: Option<Value>,
    /// Method implementations dispatched for this type. `None` means
    /// fall back to `Schema::apply_update` / `Schema::default_value`
    /// (the built-in semantics for non-Custom Schema variants).
    pub methods: Option<Arc<dyn TypeMethods>>,
    /// Type names this type inherits methods from, in resolution
    /// order (depth-first left-to-right).
    pub inherits: Vec<String>,
}

impl std::fmt::Debug for TypeEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TypeEntry")
            .field("schema", &self.schema)
            .field("default", &self.default)
            .field("has_methods", &self.methods.is_some())
            .field("inherits", &self.inherits)
            .finish()
    }
}

/// Central registry for types and schemas.
///
/// This is the Rust equivalent of bigraph-schema's `Core` object.
/// It holds registered types that can be looked up by name at runtime,
/// enabling dynamic composition from configuration files.
#[derive(Clone, Debug)]
pub struct TypeRegistry {
    types: HashMap<String, TypeEntry>,
}

impl TypeRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            types: HashMap::new(),
        };
        registry.register_builtins();
        registry
    }

    fn register_builtins(&mut self) {
        self.register("any", Schema::Any, None);
        self.register("bool", Schema::bool(), Some(Value::Bool(false)));
        self.register("boolean", Schema::bool(), Some(Value::Bool(false)));
        self.register("integer", Schema::integer(), Some(Value::Int(0)));
        self.register("int", Schema::integer(), Some(Value::Int(0)));
        self.register("float", Schema::float(), Some(Value::float(0.0)));
        self.register("string", Schema::string(), Some(Value::String(String::new())));
        self.register("delta", Schema::delta(), Some(Value::float(0.0)));
        // RT.3: schema-as-state primitive. Schemas live in the place
        // graph as `Value::Foreign(Arc<Schema>)`.
        self.register_full(
            "schema",
            Schema::Any,
            None,
            Some(Arc::new(SchemaTypeMethods) as Arc<dyn TypeMethods>),
            Vec::new(),
        );
        // RT.4: generic catalog types.
        self.register_full(
            "chain",
            Schema::Any,
            None,
            Some(Arc::new(ChainTypeMethods) as Arc<dyn TypeMethods>),
            Vec::new(),
        );
        self.register_full(
            "graph",
            Schema::Any,
            None,
            Some(Arc::new(GraphTypeMethods) as Arc<dyn TypeMethods>),
            Vec::new(),
        );
    }

    /// Register an alias from a new name to an existing registered
    /// type. The alias inherits the existing type's schema, default,
    /// and methods (via inheritance chain lookup). Useful for
    /// domain-flavored naming over generic primitives (e.g.,
    /// `register_alias("polymer_chain", "chain")`).
    pub fn register_alias(&mut self, alias: impl Into<String>, existing: impl Into<String>) {
        let existing_name = existing.into();
        let alias_name = alias.into();
        let entry = match self.types.get(&existing_name) {
            Some(e) => TypeEntry {
                schema: e.schema.clone(),
                default: e.default.clone(),
                methods: e.methods.clone(),
                inherits: vec![existing_name],
            },
            None => TypeEntry {
                schema: Schema::Any,
                default: None,
                methods: None,
                inherits: vec![existing_name],
            },
        };
        self.types.insert(alias_name, entry);
    }

    /// Register a named type without method dispatch (data-only).
    /// For rich types with dispatch, use `register_full`.
    pub fn register(
        &mut self,
        name: impl Into<String>,
        schema: Schema,
        default: Option<Value>,
    ) {
        self.types.insert(
            name.into(),
            TypeEntry {
                schema,
                default,
                methods: None,
                inherits: Vec::new(),
            },
        );
    }

    /// Register a named type with full configuration: schema,
    /// optional default, optional method dispatch, inheritance chain.
    pub fn register_full(
        &mut self,
        name: impl Into<String>,
        schema: Schema,
        default: Option<Value>,
        methods: Option<Arc<dyn TypeMethods>>,
        inherits: Vec<String>,
    ) {
        self.types.insert(
            name.into(),
            TypeEntry {
                schema,
                default,
                methods,
                inherits,
            },
        );
    }

    /// Attach method dispatch to an already-registered type, or
    /// register a new minimal entry if the name doesn't exist yet.
    pub fn register_methods(
        &mut self,
        name: impl Into<String>,
        methods: Arc<dyn TypeMethods>,
    ) {
        let name_str = name.into();
        if let Some(entry) = self.types.get_mut(&name_str) {
            entry.methods = Some(methods);
        } else {
            self.types.insert(
                name_str,
                TypeEntry {
                    schema: Schema::Any,
                    default: None,
                    methods: Some(methods),
                    inherits: Vec::new(),
                },
            );
        }
    }

    /// Set the inheritance chain for a registered type.
    pub fn set_inherits(&mut self, name: &str, inherits: Vec<String>) {
        if let Some(entry) = self.types.get_mut(name) {
            entry.inherits = inherits;
        }
    }

    /// Look up the `TypeMethods` for a type, walking the inheritance
    /// chain depth-first left-to-right if the type itself doesn't
    /// have one. Returns `None` if no ancestor has methods either.
    pub fn methods(&self, name: &str) -> Option<Arc<dyn TypeMethods>> {
        self.methods_with_visited(name, &mut std::collections::HashSet::new())
    }

    fn methods_with_visited(
        &self,
        name: &str,
        visited: &mut std::collections::HashSet<String>,
    ) -> Option<Arc<dyn TypeMethods>> {
        if !visited.insert(name.to_string()) {
            // Cycle in inheritance chain — bail.
            return None;
        }
        let entry = self.types.get(name)?;
        if let Some(m) = &entry.methods {
            return Some(m.clone());
        }
        for parent in &entry.inherits {
            if let Some(m) = self.methods_with_visited(parent, visited) {
                return Some(m);
            }
        }
        None
    }

    // ── Dispatch helpers ──────────────────────────────────────────
    //
    // These thin wrappers route through `TypeMethods` if registered;
    // otherwise fall back to the Schema's built-in semantics for the
    // type's registered schema. Use them to keep call sites unaware of
    // whether a type has rich method dispatch or not.

    /// Compute the default value for a registered type.
    pub fn type_default(&self, name: &str) -> Value {
        if let Some(methods) = self.methods(name) {
            if let Some(entry) = self.types.get(name) {
                return methods.default(self, &entry.schema);
            }
        }
        self.types
            .get(name)
            .map(|e| e.default.clone().unwrap_or_else(|| e.schema.default_value()))
            .unwrap_or(Value::None)
    }

    /// Apply an update to a state value through the type's methods.
    pub fn type_apply(&self, name: &str, state: &Value, update: &Value) -> Value {
        if let Some(methods) = self.methods(name) {
            if let Some(entry) = self.types.get(name) {
                return methods.apply(self, &entry.schema, state, update);
            }
        }
        self.types
            .get(name)
            .map(|e| e.schema.apply_update(state, update))
            .unwrap_or_else(|| update.clone())
    }

    /// Divide a state value into daughters via the type's methods.
    pub fn type_divide(
        &self,
        name: &str,
        state: &Value,
        ctx: &DivideContext,
    ) -> Vec<Value> {
        if let Some(methods) = self.methods(name) {
            if let Some(entry) = self.types.get(name) {
                return methods.divide(self, &entry.schema, state, ctx);
            }
        }
        // No custom methods: divide by the type's schema — the faithful,
        // schema-driven default (see `divide_by_schema`).
        if let Some(entry) = self.types.get(name) {
            return divide_by_schema(&entry.schema, state, ctx, self);
        }
        // Unknown type — share.
        vec![state.clone(); ctx.n_daughters.max(2)]
    }

    /// Combine two state values via the type's `tensor` method (the
    /// merge / `fold` direction; inverse of `divide`). Same fallback
    /// shape: `TypeMethods.tensor` if registered, else schema-driven
    /// `tensor_by_schema` on the type's registered schema, else share.
    pub fn type_tensor(&self, name: &str, a: &Value, b: &Value) -> Value {
        if let Some(methods) = self.methods(name) {
            if let Some(entry) = self.types.get(name) {
                return methods.tensor(self, &entry.schema, a, b);
            }
        }
        if let Some(entry) = self.types.get(name) {
            return tensor_by_schema(&entry.schema, a, b, self);
        }
        // Unknown type — share the left.
        a.clone()
    }

    /// Serialize a state value via the type's methods (or pass-through
    /// for data-only types).
    pub fn type_serialize(&self, name: &str, state: &Value) -> Value {
        if let Some(methods) = self.methods(name) {
            if let Some(entry) = self.types.get(name) {
                return methods.serialize(self, &entry.schema, state);
            }
        }
        state.clone()
    }

    /// Realize a state value via the type's methods (or pass-through).
    pub fn type_realize(&self, name: &str, encoded: &Value) -> Value {
        if let Some(methods) = self.methods(name) {
            if let Some(entry) = self.types.get(name) {
                return methods.realize(self, &entry.schema, encoded);
            }
        }
        encoded.clone()
    }

    /// Validate via the type's `check` method (or true for data-only).
    pub fn type_check(&self, name: &str, state: &Value) -> bool {
        if let Some(methods) = self.methods(name) {
            if let Some(entry) = self.types.get(name) {
                return methods.check(self, &entry.schema, state);
            }
        }
        true
    }

    /// Look up a type by name.
    pub fn get(&self, name: &str) -> Option<&TypeEntry> {
        self.types.get(name)
    }

    /// Look up just the schema for a type name.
    pub fn schema(&self, name: &str) -> Option<&Schema> {
        self.types.get(name).map(|e| &e.schema)
    }

    /// Get the default value for a named type.
    pub fn default_value(&self, name: &str) -> Option<Value> {
        self.types.get(name).map(|e| {
            e.default.clone().unwrap_or_else(|| e.schema.default_value())
        })
    }

    /// List all registered type names.
    pub fn type_names(&self) -> Vec<&str> {
        self.types.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ════════════════════════════════════════════════════════════════════
// Schema-driven divide — faithful port of bigraph-schema methods/divide.py
// ════════════════════════════════════════════════════════════════════

/// Split a value into `ctx.n_daughters` daughters, driven entirely by its
/// **schema** — the Rust port of bigraph-schema's `methods/divide.py`.
///
/// Extensivity is encoded by the schema TYPE, not a side flag:
/// - `Delta` / `Integer` are **extensive** → split so the daughters sum to
///   the mother (the additive types: mass, counts, cumulative growth).
/// - `Float` / `Bool` / `String` / `Enum` / names / interfaces are
///   **intensive / opaque** → shared (copied) unchanged. (Plain floats are
///   usually rates/ratios/positions; halving them corrupts the value — so a
///   field that must halve declares itself `Delta`.)
/// - `Tree` / `Map` / `RecursiveTree` / `Array` / `Tuple` recurse, dividing
///   each field by ITS schema. Keys present in the value but absent from the
///   schema are shared (they carry no divide semantics) — mirroring the
///   upstream dict/Node walker.
/// - `Link` / `ProcessLink` / `StepLink` / `CompositeLink` share the spec;
///   the daughters re-realize fresh instances from `address` + `config`
///   (upstream `divide(Link)` strips the live instance, keeps the
///   declaration). Extensive *contents* therefore live in the divisible
///   container around a composite, not inside its encapsulated spec.
/// - `Custom` dispatches through the registry's `TypeMethods` (rich types).
pub fn divide_by_schema(
    schema: &Schema,
    state: &Value,
    ctx: &DivideContext,
    registry: &TypeRegistry,
) -> Vec<Value> {
    let n = ctx.n_daughters.max(2);
    match schema {
        // ── Extensive scalars: split, preserving the sum ──
        Schema::Delta { .. } => split_float(state, n),
        Schema::Integer { .. } => split_int(state, n),

        // ── Intensive / opaque scalars: share ──
        Schema::Float { .. }
        | Schema::Bool { .. }
        | Schema::String { .. }
        | Schema::Enum { .. }
        | Schema::Any
        | Schema::Site { .. }
        | Schema::InnerName { .. }
        | Schema::OuterName { .. }
        | Schema::Interface { .. }
        | Schema::Bridge { .. }
        | Schema::Const { .. }
        | Schema::Quote { .. } => share(state, n),

        // ── Wrappers: delegate to inner ──
        Schema::Maybe { inner } | Schema::Overwrite { inner } => {
            divide_by_schema(inner, state, ctx, registry)
        }

        // ── Containers: recurse per field, each by its own schema ──
        Schema::Tree { branches } => divide_named(branches, state, ctx, registry, n),
        Schema::Map { value } => divide_uniform(value, state, ctx, registry, n),
        Schema::RecursiveTree { leaf } => divide_recursive(leaf, state, ctx, registry, n),
        Schema::List { element } | Schema::Array { element, .. } => {
            divide_seq(element, state, n, ctx, registry)
        }
        Schema::Tuple { elements } => divide_tuple(elements, state, ctx, registry, n),

        // ── Any Link-kind NODE: split its self-exported data face (extensive
        //    `Delta`/`Integer` split, intensive `Float` share) and SHARE the spec
        //    (`address`/`config`/wiring) so daughters re-realize fresh instances.
        //    Uniform across kinds — a node with no self-exported face has empty
        //    `node_data_branches`, so `divide_named` walks only the spec keys and
        //    shares each one (equivalent to the old `share` arm), while a self-
        //    exporting node (e.g. a cell at `%.mass`) splits its face correctly.
        Schema::Link { .. }
        | Schema::ProcessLink { .. }
        | Schema::StepLink { .. }
        | Schema::CompositeLink { .. } => {
            divide_named(&schema.node_data_branches(), state, ctx, registry, n)
        }

        // ── Rich type: dispatch through the registry ──
        Schema::Custom { name, .. } => registry.type_divide(name, state, ctx),
    }
}

fn share(state: &Value, n: usize) -> Vec<Value> {
    vec![state.clone(); n]
}

/// Extensive float: each daughter gets `total / n` (daughters sum to total).
fn split_float(state: &Value, n: usize) -> Vec<Value> {
    match state.as_f64() {
        Some(total) => vec![Value::float(total / n as f64); n],
        None => share(state, n),
    }
}

/// Extensive integer: deterministic split preserving the sum — base `total/n`
/// per daughter, the remainder distributed one apiece. (Upstream uses a
/// binomial draw; this preserves the invariant `Σ daughters = total` without
/// needing an rng — stochastic splitting can layer on via `DivideContext`.)
fn split_int(state: &Value, n: usize) -> Vec<Value> {
    match state.as_i64() {
        Some(total) => {
            let base = total / n as i64;
            let extra = (total - base * n as i64).unsigned_abs() as usize;
            let step = if total >= 0 { 1 } else { -1 };
            (0..n)
                .map(|i| Value::Int(if i < extra { base + step } else { base }))
                .collect()
        }
        None => share(state, n),
    }
}

/// Distribute the per-field daughter parts into `n` daughter maps.
fn scatter(daughters: &mut [StateMap], key: &Key, parts: Vec<Value>) {
    for (i, d) in daughters.iter_mut().enumerate() {
        d.insert(key.clone(), parts.get(i).cloned().unwrap_or(Value::None));
    }
}

/// `Tree`: each named branch divided by its own schema; un-typed keys shared.
fn divide_named(
    branches: &IndexMap<Key, Schema>,
    state: &Value,
    ctx: &DivideContext,
    registry: &TypeRegistry,
    n: usize,
) -> Vec<Value> {
    let Some(map) = state.as_map() else {
        return share(state, n);
    };
    let mut daughters = vec![StateMap::new(); n];
    for (k, v) in map {
        match branches.get(k) {
            Some(sub) => scatter(&mut daughters, k, divide_by_schema(sub, v, ctx, registry)),
            None => scatter(&mut daughters, k, vec![v.clone(); n]),
        }
    }
    daughters.into_iter().map(Value::Map).collect()
}

/// `Map`: every entry divided by the single value schema.
fn divide_uniform(
    value: &Schema,
    state: &Value,
    ctx: &DivideContext,
    registry: &TypeRegistry,
    n: usize,
) -> Vec<Value> {
    let Some(map) = state.as_map() else {
        return share(state, n);
    };
    let mut daughters = vec![StateMap::new(); n];
    for (k, v) in map {
        scatter(&mut daughters, k, divide_by_schema(value, v, ctx, registry));
    }
    daughters.into_iter().map(Value::Map).collect()
}

/// `RecursiveTree`: nested maps with `leaf`-typed leaves.
fn divide_recursive(
    leaf: &Schema,
    state: &Value,
    ctx: &DivideContext,
    registry: &TypeRegistry,
    n: usize,
) -> Vec<Value> {
    let Some(map) = state.as_map() else {
        return divide_by_schema(leaf, state, ctx, registry);
    };
    let mut daughters = vec![StateMap::new(); n];
    for (k, v) in map {
        scatter(&mut daughters, k, divide_recursive(leaf, v, ctx, registry, n));
    }
    daughters.into_iter().map(Value::Map).collect()
}

/// `Array` (stored as a `List`): each element divided by the element schema.
fn divide_seq(
    element: &Schema,
    state: &Value,
    n: usize,
    ctx: &DivideContext,
    registry: &TypeRegistry,
) -> Vec<Value> {
    let Some(list) = state.as_list() else {
        return share(state, n);
    };
    let mut daughters: Vec<Vec<Value>> = vec![Vec::new(); n];
    for v in list {
        let parts = divide_by_schema(element, v, ctx, registry);
        for (i, d) in daughters.iter_mut().enumerate() {
            d.push(parts.get(i).cloned().unwrap_or(Value::None));
        }
    }
    daughters.into_iter().map(Value::List).collect()
}

/// `Tuple`: positional, each element divided by its own schema.
fn divide_tuple(
    elements: &[Schema],
    state: &Value,
    ctx: &DivideContext,
    registry: &TypeRegistry,
    n: usize,
) -> Vec<Value> {
    let Some(list) = state.as_list() else {
        return share(state, n);
    };
    if list.len() != elements.len() {
        return share(state, n);
    }
    let mut daughters: Vec<Vec<Value>> = vec![Vec::new(); n];
    for (v, sub) in list.iter().zip(elements) {
        let parts = divide_by_schema(sub, v, ctx, registry);
        for (i, d) in daughters.iter_mut().enumerate() {
            d.push(parts.get(i).cloned().unwrap_or(Value::None));
        }
    }
    daughters.into_iter().map(Value::List).collect()
}

// ════════════════════════════════════════════════════════════════════
// Schema-driven tensor — the dual of divide (the `fold` / merge direction)
// ════════════════════════════════════════════════════════════════════

/// Combine two values into one, driven entirely by their shared **schema**
/// — the **dual of `divide_by_schema`** (the `fold` direction of the
/// `fold`/`unfurl` pair from `docs/bigraphs-all-the-way-down.md`; the
/// merge of `docs/merge-protocol.md`).
///
/// Extensivity is encoded by the schema TYPE, mirroring `divide`:
/// - `Delta` / `Integer` are **extensive** → tensor SUMS the two values
///   (so `tensor(divide(state).0, divide(state).1) ≈ state`).
/// - `Float` / `Bool` / `String` / `Enum` / names / interfaces are
///   **intensive / opaque** → share the left (the caller is responsible
///   for consistency; intensive `tensor(x, x) = x` is idempotent).
/// - `Tree` / `Map` / `RecursiveTree` / `Array` / `List` / `Tuple` recurse,
///   tensoring each branch / element by its schema. For named branches and
///   maps, keys present in only one side are copied through (no
///   discarding).
/// - `Link` / `ProcessLink` / `StepLink` / `CompositeLink` tensor the
///   self-exported data face (the mirror of divide's split-face-share-spec
///   pattern), so two composites of the same shape merge their FACE while
///   the spec stays shared.
/// - `Custom` dispatches through the registry's `TypeMethods::tensor`
///   (rich types — `Qubits` cross-products amplitudes).
///
/// This is the schema-driven default; rich types override per-type in
/// their `TypeMethods` impl.
pub fn tensor_by_schema(
    schema: &Schema,
    a: &Value,
    b: &Value,
    registry: &TypeRegistry,
) -> Value {
    match schema {
        // ── Extensive scalars: sum ──
        Schema::Delta { .. } => sum_floats(a, b),
        Schema::Integer { .. } => sum_ints(a, b),

        // ── Intensive / opaque scalars: share the left ──
        Schema::Float { .. }
        | Schema::Bool { .. }
        | Schema::String { .. }
        | Schema::Enum { .. }
        | Schema::Any
        | Schema::Site { .. }
        | Schema::InnerName { .. }
        | Schema::OuterName { .. }
        | Schema::Interface { .. }
        | Schema::Bridge { .. }
        | Schema::Const { .. }
        | Schema::Quote { .. } => a.clone(),

        // ── Wrappers: delegate to inner ──
        Schema::Maybe { inner } | Schema::Overwrite { inner } => {
            tensor_by_schema(inner, a, b, registry)
        }

        // ── Containers: recurse per field, each by its own schema ──
        Schema::Tree { branches } => tensor_named(branches, a, b, registry),
        Schema::Map { value } => tensor_uniform(value, a, b, registry),
        Schema::RecursiveTree { leaf } => tensor_recursive(leaf, a, b, registry),
        Schema::List { element } | Schema::Array { element, .. } => {
            tensor_seq(element, a, b, registry)
        }
        Schema::Tuple { elements } => tensor_tuple(elements, a, b, registry),

        // ── Any Link-kind NODE: tensor its self-exported data face (the
        //    mirror of divide's `node_data_branches` split), and share the
        //    spec (`address`/`config`/wiring stays one description, two
        //    sub-bigraphs merge by *state*, not by spec rewrite).
        Schema::Link { .. }
        | Schema::ProcessLink { .. }
        | Schema::StepLink { .. }
        | Schema::CompositeLink { .. } => {
            tensor_named(&schema.node_data_branches(), a, b, registry)
        }

        // ── Rich type: dispatch through the registry ──
        Schema::Custom { name, .. } => registry.type_tensor(name, a, b),
    }
}

/// Extensive float tensor: `a + b` (so `tensor(divide(x, 2).0, divide(x, 2).1) = x`).
fn sum_floats(a: &Value, b: &Value) -> Value {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => Value::float(x + y),
        _ => a.clone(),
    }
}

/// Extensive integer tensor: `a + b` (so divide's `Σ daughters = total` inverts).
fn sum_ints(a: &Value, b: &Value) -> Value {
    match (a.as_i64(), b.as_i64()) {
        (Some(x), Some(y)) => Value::Int(x + y),
        _ => a.clone(),
    }
}

/// `Tree`: tensor each named branch by its own schema. Keys present in
/// only one side carry through unchanged (no discarding — composites
/// growing a slot one tick before the other shouldn't lose it).
fn tensor_named(
    branches: &IndexMap<Key, Schema>,
    a: &Value,
    b: &Value,
    registry: &TypeRegistry,
) -> Value {
    let (Some(ma), Some(mb)) = (a.as_map(), b.as_map()) else {
        return a.clone();
    };
    let mut out: StateMap = StateMap::new();
    // First pass: keys present in `a` get tensored with `b`'s value (or
    // copied through if absent in `b`).
    for (k, va) in ma {
        let new_val = match mb.get(k) {
            Some(vb) => match branches.get(k) {
                Some(sub) => tensor_by_schema(sub, va, vb, registry),
                None => va.clone(), // untyped key → share left
            },
            None => va.clone(),
        };
        out.insert(k.clone(), new_val);
    }
    // Second pass: keys only in `b` get copied through.
    for (k, vb) in mb {
        if !ma.contains_key(k) {
            out.insert(k.clone(), vb.clone());
        }
    }
    Value::Map(out)
}

/// `Map`: every entry tensored by the single value schema (key union).
fn tensor_uniform(
    value: &Schema,
    a: &Value,
    b: &Value,
    registry: &TypeRegistry,
) -> Value {
    let (Some(ma), Some(mb)) = (a.as_map(), b.as_map()) else {
        return a.clone();
    };
    let mut out: StateMap = StateMap::new();
    for (k, va) in ma {
        let new_val = match mb.get(k) {
            Some(vb) => tensor_by_schema(value, va, vb, registry),
            None => va.clone(),
        };
        out.insert(k.clone(), new_val);
    }
    for (k, vb) in mb {
        if !ma.contains_key(k) {
            out.insert(k.clone(), vb.clone());
        }
    }
    Value::Map(out)
}

/// `RecursiveTree`: nested maps with `leaf`-typed leaves. Recurse on
/// shared keys; pass through one-sided keys.
fn tensor_recursive(
    leaf: &Schema,
    a: &Value,
    b: &Value,
    registry: &TypeRegistry,
) -> Value {
    match (a.as_map(), b.as_map()) {
        (Some(ma), Some(mb)) => {
            let mut out: StateMap = StateMap::new();
            for (k, va) in ma {
                let new_val = match mb.get(k) {
                    Some(vb) => tensor_recursive(leaf, va, vb, registry),
                    None => va.clone(),
                };
                out.insert(k.clone(), new_val);
            }
            for (k, vb) in mb {
                if !ma.contains_key(k) {
                    out.insert(k.clone(), vb.clone());
                }
            }
            Value::Map(out)
        }
        // One or both sides are leaves: tensor by the leaf schema.
        _ => tensor_by_schema(leaf, a, b, registry),
    }
}

/// `Array` / `List`: element-wise tensor (mirrors `divide_seq`, which
/// distributes element-wise across daughters). Mismatched lengths fall
/// back to the longer side, padding with element-wise tensor where
/// indices align.
fn tensor_seq(
    element: &Schema,
    a: &Value,
    b: &Value,
    registry: &TypeRegistry,
) -> Value {
    let (Some(la), Some(lb)) = (a.as_list(), b.as_list()) else {
        return a.clone();
    };
    let len = la.len().max(lb.len());
    let mut out: Vec<Value> = Vec::with_capacity(len);
    for i in 0..len {
        match (la.get(i), lb.get(i)) {
            (Some(va), Some(vb)) => out.push(tensor_by_schema(element, va, vb, registry)),
            (Some(va), None) => out.push(va.clone()),
            (None, Some(vb)) => out.push(vb.clone()),
            (None, None) => out.push(Value::None),
        }
    }
    Value::List(out)
}

/// `Tuple`: positional, each element tensored by its own schema. Length
/// mismatch with the tuple's declared elements falls back to sharing.
fn tensor_tuple(
    elements: &[Schema],
    a: &Value,
    b: &Value,
    registry: &TypeRegistry,
) -> Value {
    let (Some(la), Some(lb)) = (a.as_list(), b.as_list()) else {
        return a.clone();
    };
    if la.len() != elements.len() || lb.len() != elements.len() {
        return a.clone();
    }
    let out: Vec<Value> = la
        .iter()
        .zip(lb.iter())
        .zip(elements.iter())
        .map(|((va, vb), sub)| tensor_by_schema(sub, va, vb, registry))
        .collect();
    Value::List(out)
}

// ════════════════════════════════════════════════════════════════════
// Generic catalog (RT.4 upstream) — types every domain can reuse.
// ════════════════════════════════════════════════════════════════════

/// `chain` — generic ordered kinetic sequence. Subunits live in
/// `Value::List`; updates can be:
///   - `Value::List(_)` — replace wholesale (initialization, large rewrites)
///   - `Value::Map({"_add": [items], "_remove": [indices]})` — structured
///     insert/remove
///   - `Value::Map({"_set": [(index, value)...]})` — in-place mutation
///
/// Divide partitions the chain extensively — daughters get
/// approximately equal halves by count. `DivideContext.partition`
/// can override (per-index daughter assignment).
///
/// Serializes as a plain `Value::List`. Realizes from same.
pub struct ChainTypeMethods;

impl TypeMethods for ChainTypeMethods {
    fn default(&self, _registry: &TypeRegistry, _schema: &Schema) -> Value {
        Value::List(Vec::new())
    }

    fn apply(
        &self,
        _registry: &TypeRegistry,
        _schema: &Schema,
        state: &Value,
        update: &Value,
    ) -> Value {
        // Replace if update is a List.
        if let Value::List(_) = update {
            return update.clone();
        }
        // Structured update map.
        if let Value::Map(upd) = update {
            let mut list: Vec<Value> = match state {
                Value::List(l) => l.clone(),
                _ => Vec::new(),
            };
            // _set: [(index, value)] — in-place mutation
            if let Some(Value::List(sets)) = upd.get("_set") {
                for entry in sets {
                    if let Value::List(pair) = entry {
                        if pair.len() == 2 {
                            if let Some(i) = pair[0].as_i64() {
                                let idx = i.max(0) as usize;
                                if idx < list.len() {
                                    list[idx] = pair[1].clone();
                                }
                            }
                        }
                    }
                }
            }
            // _remove: [indices] — drop by index, descending
            if let Some(Value::List(rms)) = upd.get("_remove") {
                let mut indices: Vec<usize> = rms
                    .iter()
                    .filter_map(|v| v.as_i64())
                    .map(|i| i.max(0) as usize)
                    .filter(|&i| i < list.len())
                    .collect();
                indices.sort_unstable_by(|a, b| b.cmp(a));
                indices.dedup();
                for i in indices {
                    list.remove(i);
                }
            }
            // _add: [items] — append
            if let Some(Value::List(adds)) = upd.get("_add") {
                list.extend(adds.iter().cloned());
            }
            return Value::List(list);
        }
        // Anything else: leave state unchanged.
        state.clone()
    }

    fn divide(
        &self,
        _registry: &TypeRegistry,
        _schema: &Schema,
        state: &Value,
        ctx: &DivideContext,
    ) -> Vec<Value> {
        let n = ctx.n_daughters.max(2);
        let list = match state {
            Value::List(l) => l.clone(),
            _ => Vec::new(),
        };
        // If partition assignments are provided, route each index.
        if !ctx.partition.is_empty() {
            let mut buckets: Vec<Vec<Value>> = vec![Vec::new(); n];
            for (i, item) in list.iter().enumerate() {
                let key = format!("{i}");
                let target = ctx.partition.get(&key).copied().unwrap_or(i % n);
                if target < n {
                    buckets[target].push(item.clone());
                }
            }
            return buckets.into_iter().map(Value::List).collect();
        }
        // Default: round-robin equal partition by count.
        let mut buckets: Vec<Vec<Value>> = vec![Vec::new(); n];
        for (i, item) in list.into_iter().enumerate() {
            buckets[i % n].push(item);
        }
        buckets.into_iter().map(Value::List).collect()
    }

    fn serialize(&self, _registry: &TypeRegistry, _schema: &Schema, state: &Value) -> Value {
        state.clone()
    }

    fn realize(&self, _registry: &TypeRegistry, _schema: &Schema, encoded: &Value) -> Value {
        match encoded {
            Value::List(_) => encoded.clone(),
            _ => Value::List(Vec::new()),
        }
    }
}

/// `graph` — generic vertex+edge structure with adjacency.
///
/// State shape: `Value::Map({"vertices": Map<id, vertex_data>,
/// "edges": List<(id_a, id_b)>})`. Apply supports:
///   - `{"_add_vertex": {id: data}, "_remove_vertex": [ids],
///      "_add_edge": [[a,b]...], "_remove_edge": [[a,b]...]}`
///
/// Divide partitions vertices by `ctx.partition` (vertex_id →
/// daughter index); edges go to the daughter that contains both
/// endpoints (cross-component edges are dropped — biological
/// cleavage).
///
/// Serializes as a plain Map. Realizes from same.
pub struct GraphTypeMethods;

impl GraphTypeMethods {
    fn empty_state() -> Value {
        Value::tree([("vertices", Value::map()), ("edges", Value::List(Vec::new()))])
    }
}

impl TypeMethods for GraphTypeMethods {
    fn default(&self, _registry: &TypeRegistry, _schema: &Schema) -> Value {
        Self::empty_state()
    }

    fn apply(
        &self,
        _registry: &TypeRegistry,
        _schema: &Schema,
        state: &Value,
        update: &Value,
    ) -> Value {
        let mut vertices: IndexMap<crate::value::Key, Value> = state
            .get_field("vertices")
            .and_then(|v| v.as_map())
            .cloned()
            .unwrap_or_default();
        let mut edges: Vec<Value> = match state.get_field("edges") {
            Some(Value::List(l)) => l.clone(),
            _ => Vec::new(),
        };

        if let Value::Map(upd) = update {
            if let Some(Value::Map(adds)) = upd.get("_add_vertex") {
                for (k, v) in adds {
                    vertices.insert(k.clone(), v.clone());
                }
            }
            if let Some(Value::List(rms)) = upd.get("_remove_vertex") {
                for id in rms {
                    if let Some(s) = id.as_str() {
                        vertices.shift_remove(s);
                    }
                }
                // Also drop edges referencing removed vertices.
                let removed: std::collections::HashSet<&str> = rms
                    .iter()
                    .filter_map(|v| v.as_str())
                    .collect();
                edges.retain(|e| {
                    if let Value::List(pair) = e {
                        if pair.len() == 2 {
                            let a = pair[0].as_str();
                            let b = pair[1].as_str();
                            return !(a.map_or(false, |s| removed.contains(s))
                                || b.map_or(false, |s| removed.contains(s)));
                        }
                    }
                    true
                });
            }
            if let Some(Value::List(adds)) = upd.get("_add_edge") {
                edges.extend(adds.iter().cloned());
            }
            if let Some(Value::List(rms)) = upd.get("_remove_edge") {
                edges.retain(|e| !rms.iter().any(|r| edges_equal(e, r)));
            }
        }
        Value::tree([
            ("vertices", Value::Map(vertices)),
            ("edges", Value::List(edges)),
        ])
    }

    fn divide(
        &self,
        _registry: &TypeRegistry,
        _schema: &Schema,
        state: &Value,
        ctx: &DivideContext,
    ) -> Vec<Value> {
        let n = ctx.n_daughters.max(2);
        let vertices = state
            .get_field("vertices")
            .and_then(|v| v.as_map())
            .cloned()
            .unwrap_or_default();
        let edges: Vec<Value> = match state.get_field("edges") {
            Some(Value::List(l)) => l.clone(),
            _ => Vec::new(),
        };
        let mut daughter_vertices: Vec<IndexMap<crate::value::Key, Value>> =
            vec![IndexMap::new(); n];
        // Assignment lookup: ctx.partition maps vertex_id → daughter index.
        // Vertices not in the partition get assigned round-robin.
        let mut fallback_idx = 0usize;
        let mut vertex_to_daughter: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for (k, v) in vertices.iter() {
            let key_str = k.to_string();
            let target = ctx
                .partition
                .get(&key_str)
                .copied()
                .unwrap_or_else(|| {
                    let t = fallback_idx % n;
                    fallback_idx += 1;
                    t
                });
            let target = target.min(n - 1);
            daughter_vertices[target].insert(k.clone(), v.clone());
            vertex_to_daughter.insert(key_str, target);
        }
        // Edges: route to whichever daughter contains both endpoints.
        // Cross-daughter edges are dropped (the cleavage cuts them).
        let mut daughter_edges: Vec<Vec<Value>> = vec![Vec::new(); n];
        for edge in &edges {
            if let Value::List(pair) = edge {
                if pair.len() == 2 {
                    if let (Some(a), Some(b)) =
                        (pair[0].as_str(), pair[1].as_str())
                    {
                        if let (Some(da), Some(db)) = (
                            vertex_to_daughter.get(a),
                            vertex_to_daughter.get(b),
                        ) {
                            if da == db {
                                daughter_edges[*da].push(edge.clone());
                            }
                            // else: cross-daughter edge, drop (cleaved).
                        }
                    }
                }
            }
        }
        daughter_vertices
            .into_iter()
            .zip(daughter_edges.into_iter())
            .map(|(verts, edges)| {
                Value::tree([
                    ("vertices", Value::Map(verts)),
                    ("edges", Value::List(edges)),
                ])
            })
            .collect()
    }

    fn serialize(&self, _registry: &TypeRegistry, _schema: &Schema, state: &Value) -> Value {
        state.clone()
    }

    fn realize(&self, _registry: &TypeRegistry, _schema: &Schema, encoded: &Value) -> Value {
        if encoded.get_field("vertices").is_some() {
            encoded.clone()
        } else {
            Self::empty_state()
        }
    }
}

fn edges_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::List(la), Value::List(lb)) if la.len() == 2 && lb.len() == 2 => {
            (la[0] == lb[0] && la[1] == lb[1]) || (la[0] == lb[1] && la[1] == lb[0])
        }
        _ => false,
    }
}

// ════════════════════════════════════════════════════════════════════
// SchemaTypeMethods — RT.3: schemas are first-class values.
// ════════════════════════════════════════════════════════════════════
//
// Registered under name `"schema"`. The `Foreign` carrier holds an
// `Arc<Schema>` instance.
//
// Semantics:
//   - default: returns `Foreign(Schema::Any)`
//   - apply:   schemas are declarative — update replaces state
//   - divide:  schemas are intensive — copy to every daughter
//   - serialize: serde_json::to_value on the inner Schema, then converted
//                to a `Value::Map`
//   - realize:   Value::Map → serde_json::Value → Schema → Foreign

struct SchemaTypeMethods;

impl TypeMethods for SchemaTypeMethods {
    fn default(&self, _registry: &TypeRegistry, _schema: &Schema) -> Value {
        Value::Foreign(Foreign::new("schema", Schema::Any))
    }

    fn apply(
        &self,
        _registry: &TypeRegistry,
        _schema: &Schema,
        _state: &Value,
        update: &Value,
    ) -> Value {
        // Schemas are declarative — apply means "replace with the new
        // schema." If the update is already a Foreign, use it directly;
        // otherwise try realizing the update from JSON-shape.
        if matches!(update, Value::Foreign(f) if f.type_name == "schema") {
            update.clone()
        } else {
            self.realize(_registry, _schema, update)
        }
    }

    fn divide(
        &self,
        _registry: &TypeRegistry,
        _schema: &Schema,
        state: &Value,
        ctx: &DivideContext,
    ) -> Vec<Value> {
        // Intensive — every daughter gets the same schema.
        let n = ctx.n_daughters.max(2);
        vec![state.clone(); n]
    }

    fn serialize(
        &self,
        _registry: &TypeRegistry,
        _schema: &Schema,
        state: &Value,
    ) -> Value {
        // Pull the Schema out of the Foreign, serialize via serde_json,
        // convert to a `Value::Map`.
        let schema_inner = state
            .as_foreign()
            .and_then(|f| f.downcast_ref::<Schema>())
            .cloned()
            .unwrap_or(Schema::Any);
        let json = serde_json::to_value(&schema_inner)
            .unwrap_or(serde_json::Value::Null);
        json_to_value(&json)
    }

    fn realize(
        &self,
        _registry: &TypeRegistry,
        _schema: &Schema,
        encoded: &Value,
    ) -> Value {
        let json = value_to_json(encoded);
        let inner: Schema = serde_json::from_value(json).unwrap_or(Schema::Any);
        Value::Foreign(Foreign::new("schema", inner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Foreign;
    use std::any::Any;

    #[test]
    fn test_builtins() {
        let reg = TypeRegistry::new();
        assert!(reg.get("float").is_some());
        assert!(reg.get("integer").is_some());
        assert!(reg.get("string").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_custom_type() {
        let mut reg = TypeRegistry::new();
        reg.register(
            "concentration",
            Schema::float_default(0.0),
            Some(Value::float(0.0)),
        );
        assert!(reg.get("concentration").is_some());
        assert_eq!(reg.default_value("concentration"), Some(Value::float(0.0)));
    }

    // ── Rich-type method dispatch tests (RT.2) ────────────────────

    /// A toy `Counter` type: holds a `u64` count, saturating add for
    /// apply, halves for divide, serializes as Int.
    #[derive(Debug, Clone, Copy)]
    struct Counter {
        value: u64,
    }

    struct CounterMethods;

    impl TypeMethods for CounterMethods {
        fn default(&self, _registry: &TypeRegistry, _schema: &Schema) -> Value {
            Value::Foreign(Foreign::new("counter", Counter { value: 0 }))
        }

        fn apply(
            &self,
            _registry: &TypeRegistry,
            _schema: &Schema,
            state: &Value,
            update: &Value,
        ) -> Value {
            let cur = state
                .as_foreign()
                .and_then(|f| f.downcast_ref::<Counter>())
                .copied()
                .unwrap_or(Counter { value: 0 });
            let delta = update.as_i64().unwrap_or(0).max(0) as u64;
            Value::Foreign(Foreign::new(
                "counter",
                Counter {
                    value: cur.value.saturating_add(delta),
                },
            ))
        }

        fn divide(
            &self,
            _registry: &TypeRegistry,
            _schema: &Schema,
            state: &Value,
            ctx: &DivideContext,
        ) -> Vec<Value> {
            let cur = state
                .as_foreign()
                .and_then(|f| f.downcast_ref::<Counter>())
                .copied()
                .unwrap_or(Counter { value: 0 });
            let n = ctx.n_daughters.max(2) as u64;
            let per = cur.value / n;
            let rem = cur.value % n;
            (0..n)
                .map(|i| {
                    let v = per + if i < rem { 1 } else { 0 };
                    Value::Foreign(Foreign::new("counter", Counter { value: v }))
                })
                .collect()
        }

        fn serialize(&self, _registry: &TypeRegistry, _schema: &Schema, state: &Value) -> Value {
            let cur = state
                .as_foreign()
                .and_then(|f| f.downcast_ref::<Counter>())
                .copied()
                .unwrap_or(Counter { value: 0 });
            Value::Int(cur.value as i64)
        }

        fn realize(&self, _registry: &TypeRegistry, _schema: &Schema, encoded: &Value) -> Value {
            let v = encoded.as_i64().unwrap_or(0).max(0) as u64;
            Value::Foreign(Foreign::new("counter", Counter { value: v }))
        }
    }

    fn register_counter(reg: &mut TypeRegistry) {
        reg.register_full(
            "counter",
            Schema::Any,
            None,
            Some(Arc::new(CounterMethods) as Arc<dyn TypeMethods>),
            Vec::new(),
        );
    }

    #[test]
    fn rich_type_default_apply_serialize() {
        let mut reg = TypeRegistry::new();
        register_counter(&mut reg);

        let v0 = reg.type_default("counter");
        assert_eq!(
            v0.as_foreign().unwrap().downcast_ref::<Counter>().unwrap().value,
            0
        );

        let v1 = reg.type_apply("counter", &v0, &Value::Int(3));
        let v2 = reg.type_apply("counter", &v1, &Value::Int(2));
        assert_eq!(
            v2.as_foreign().unwrap().downcast_ref::<Counter>().unwrap().value,
            5
        );

        let serialized = reg.type_serialize("counter", &v2);
        assert_eq!(serialized, Value::Int(5));

        let realized = reg.type_realize("counter", &serialized);
        assert_eq!(
            realized.as_foreign().unwrap().downcast_ref::<Counter>().unwrap().value,
            5
        );
    }

    #[test]
    fn schema_driven_divide_halves_extensive_shares_intensive() {
        // The container layout: a Tree whose fields carry their divide
        // semantics in their SCHEMA — `mass` is extensive (Delta → halves),
        // `rate` is intensive (Float → shares), `body` is a sub-engine spec
        // (CompositeLink → shared; daughters re-realize). `_type` has no
        // schema entry → shared. No registered methods, no `_type` dispatch:
        // divide is read entirely from the schema.
        let reg = TypeRegistry::new();
        let schema = Schema::Tree {
            branches: IndexMap::from([
                (Key::from("mass"), Schema::Delta { default: None }),
                (Key::from("rate"), Schema::Float { default: None }),
                (
                    Key::from("body"),
                    Schema::CompositeLink {
                        inputs: IndexMap::new(),
                        outputs: IndexMap::new(),
                        interval: 1.0,
                        inner_schema: Box::new(Schema::Any),
                    },
                ),
            ]),
        };
        let body = Value::tree([
            ("address", Value::String("local:Composite".to_string())),
            ("config", Value::map()),
        ]);
        let mother = Value::tree([
            ("_type", Value::String("Cell".to_string())),
            ("mass", Value::float(2.0)),
            ("rate", Value::float(0.6)),
            ("body", body.clone()),
        ]);

        let daughters = divide_by_schema(&schema, &mother, &DivideContext::binary(), &reg);
        assert_eq!(daughters.len(), 2);
        for d in &daughters {
            assert_eq!(
                d.get_field("mass").and_then(|v| v.as_f64()),
                Some(1.0),
                "mass is extensive (Delta) → halves"
            );
            assert_eq!(
                d.get_field("rate").and_then(|v| v.as_f64()),
                Some(0.6),
                "rate is intensive (Float) → shares"
            );
            assert_eq!(
                d.get_field("_type").and_then(|v| v.as_str()),
                Some("Cell"),
                "untyped key shared"
            );
            assert_eq!(
                d.get_field("body"),
                Some(&body),
                "composite spec shared (daughters re-realize)"
            );
        }
        let total: f64 = daughters
            .iter()
            .map(|d| d.get_field("mass").and_then(|v| v.as_f64()).unwrap())
            .sum();
        assert_eq!(total, 2.0, "extensive mass conserved across daughters");
    }

    #[test]
    fn divide_sentinel_splits_a_cell_in_a_map() {
        // The engine-side trigger: a `_divide` sentinel in a Map update is
        // consumed by the schema-aware apply, which runs the schema-driven
        // divide on the named child and installs the daughters. This is how
        // a reaction's `?c.divide()` becomes an actual split — the reaction
        // marks intent, the schema-holding apply performs it.
        let reg = TypeRegistry::new();
        let cell = Schema::Tree {
            branches: IndexMap::from([
                (Key::from("mass"), Schema::Delta { default: None }),
                (
                    Key::from("body"),
                    Schema::CompositeLink {
                        inputs: IndexMap::new(),
                        outputs: IndexMap::new(),
                        interval: 1.0,
                        inner_schema: Box::new(Schema::Any),
                    },
                ),
            ]),
        };
        let cells_schema = Schema::Map {
            value: Box::new(cell),
        };
        let body = Value::tree([
            ("address", Value::String("local:Composite".to_string())),
            ("config", Value::map()),
        ]);
        let cells = Value::tree([(
            "0",
            Value::tree([
                ("_type", Value::String("Cell".to_string())),
                ("mass", Value::float(2.0)),
                ("body", body.clone()),
            ]),
        )]);
        let update = Value::tree([(
            "_divide",
            Value::tree([
                ("mother", Value::String("0".to_string())),
                (
                    "daughters",
                    Value::tree([("0_0", Value::map()), ("0_1", Value::map())]),
                ),
            ]),
        )]);

        let result = cells_schema.apply_update_with(Some(&reg), &cells, &update);
        let m = result.as_map().expect("result is a map");
        assert!(m.get("0").is_none(), "mother removed");
        assert_eq!(m.len(), 2, "two daughters installed");
        for k in ["0_0", "0_1"] {
            let d = m.get(k).unwrap_or_else(|| panic!("daughter {k} present"));
            assert_eq!(
                d.get_field("mass").and_then(|v| v.as_f64()),
                Some(1.0),
                "{k}: mass halved (Delta)"
            );
            assert_eq!(
                d.get_field("_type").and_then(|v| v.as_str()),
                Some("Cell"),
                "{k}: _type shared"
            );
            assert_eq!(
                d.get_field("body"),
                Some(&body),
                "{k}: composite body shared (re-realizes)"
            );
        }
    }

    #[test]
    fn divide_composite_link_node_halves_face_shares_spec() {
        // A cell is an ADDRESSED CompositeLink node `{address, config, mass}` —
        // not a container. Its `CompositeLink` schema exposes `mass: Delta` as its
        // outer FACE (an output port bridged onto its own node via `%.mass`).
        // `divide_by_schema(CompositeLink, node)` halves the on-node face and
        // SHARES the rest of the spec — crucially `address`, so daughters
        // re-realize on the SAME protocol (this is what makes division
        // cross-protocol "for free": the parent that holds the node holds the
        // address). The shared `config.state.mass` is stale, but the face is
        // authoritative — the `%`-self reseed corrects each daughter on tick 1 —
        // so face-only division conserves mass without dividing `inner_schema`.
        let reg = TypeRegistry::new();
        let schema = Schema::CompositeLink {
            inputs: IndexMap::from([(Key::from("mass"), Schema::Delta { default: None })]),
            outputs: IndexMap::from([(Key::from("mass"), Schema::Delta { default: None })]),
            interval: 1.0,
            inner_schema: Box::new(Schema::Tree {
                branches: IndexMap::from([(Key::from("mass"), Schema::Delta { default: None })]),
            }),
        };
        // The node carries its spec + the current exported face (mass=2.0, grown
        // past its t=0 seed of 1.2).
        let config = Value::tree([
            ("state", Value::tree([("mass", Value::float(1.2))])),
            ("bridge", Value::map()),
        ]);
        let node = Value::tree([
            ("address", Value::String("local:Composite".into())),
            ("config", config.clone()),
            ("mass", Value::float(2.0)),
        ]);

        let daughters = divide_by_schema(&schema, &node, &DivideContext::binary(), &reg);
        assert_eq!(daughters.len(), 2);
        for d in &daughters {
            assert_eq!(
                d.get_field("mass").and_then(|v| v.as_f64()),
                Some(1.0),
                "the on-node face (mass:Delta) halves"
            );
            assert_eq!(
                d.get_field("address").and_then(|v| v.as_str()),
                Some("local:Composite"),
                "address SHARED → daughters re-realize on the SAME protocol (cross-protocol division)"
            );
            assert_eq!(
                d.get_field("config"),
                Some(&config),
                "config (the re-realization seed) shared; the face is authoritative via the %-self reseed"
            );
        }
        let total: f64 = daughters
            .iter()
            .map(|d| d.get_field("mass").and_then(|v| v.as_f64()).unwrap())
            .sum();
        assert_eq!(total, 2.0, "extensive face conserved across daughters — no mass from nothing");
    }

    #[test]
    fn rich_type_divide_partitions_extensive() {
        let mut reg = TypeRegistry::new();
        register_counter(&mut reg);

        let parent = Value::Foreign(Foreign::new("counter", Counter { value: 10 }));
        let daughters = reg.type_divide("counter", &parent, &DivideContext::binary());
        assert_eq!(daughters.len(), 2);
        let total: u64 = daughters
            .iter()
            .map(|d| d.as_foreign().unwrap().downcast_ref::<Counter>().unwrap().value)
            .sum();
        assert_eq!(total, 10, "extensive partition must preserve total");
    }

    #[test]
    fn inheritance_chain_walks_to_parent_methods() {
        let mut reg = TypeRegistry::new();
        register_counter(&mut reg);
        // Register a child that inherits methods from "counter".
        reg.register_full(
            "fancy_counter",
            Schema::Any,
            None,
            None, // No own methods — falls back to parent's.
            vec!["counter".to_string()],
        );
        // type_default on fancy_counter dispatches to counter's methods.
        let v0 = reg.type_default("fancy_counter");
        assert_eq!(
            v0.as_foreign().unwrap().downcast_ref::<Counter>().unwrap().value,
            0
        );
        let v1 = reg.type_apply("fancy_counter", &v0, &Value::Int(7));
        assert_eq!(
            v1.as_foreign().unwrap().downcast_ref::<Counter>().unwrap().value,
            7
        );
    }

    // ── Schema-as-state tests (RT.3) ──────────────────────────────

    #[test]
    fn schema_type_default_is_any() {
        let reg = TypeRegistry::new();
        let v = reg.type_default("schema");
        let inner = v.as_foreign().and_then(|f| f.downcast_ref::<Schema>());
        assert!(matches!(inner, Some(Schema::Any)));
    }

    #[test]
    fn schema_round_trips_through_serialize_realize() {
        let reg = TypeRegistry::new();
        // Build a non-trivial schema and put it in a Foreign.
        let original = Schema::Float {
            default: Some(1.5),
        };
        let state = Value::Foreign(Foreign::new("schema", original.clone()));
        // Serialize → Value (JSON-shaped).
        let serialized = reg.type_serialize("schema", &state);
        // Serialized should be a Map (because Schema serializes as a
        // serde-tagged enum → JSON object).
        assert!(matches!(serialized, Value::Map(_)));
        // Realize → Foreign(Schema).
        let realized = reg.type_realize("schema", &serialized);
        let inner = realized
            .as_foreign()
            .and_then(|f| f.downcast_ref::<Schema>())
            .cloned()
            .unwrap();
        assert_eq!(inner, original);
    }

    #[test]
    fn schema_divide_is_intensive() {
        let reg = TypeRegistry::new();
        let state = Value::Foreign(Foreign::new(
            "schema",
            Schema::Integer { default: Some(7) },
        ));
        let daughters = reg.type_divide("schema", &state, &DivideContext::binary());
        assert_eq!(daughters.len(), 2);
        for d in &daughters {
            let inner = d
                .as_foreign()
                .and_then(|f| f.downcast_ref::<Schema>())
                .cloned()
                .unwrap();
            assert_eq!(inner, Schema::Integer { default: Some(7) });
        }
    }

    #[test]
    fn schema_apply_replaces() {
        let reg = TypeRegistry::new();
        let initial = Value::Foreign(Foreign::new("schema", Schema::Any));
        let new_schema = Schema::Float { default: Some(3.14) };
        let update = Value::Foreign(Foreign::new("schema", new_schema.clone()));
        let result = reg.type_apply("schema", &initial, &update);
        let inner = result
            .as_foreign()
            .and_then(|f| f.downcast_ref::<Schema>())
            .cloned()
            .unwrap();
        assert_eq!(inner, new_schema);
    }

    // ── Generic catalog tests (RT.4) ──────────────────────────────

    #[test]
    fn chain_supports_add_remove_set() {
        let reg = TypeRegistry::new();
        let mut state = reg.type_default("chain");
        // _add: append 3 items
        state = reg.type_apply(
            "chain",
            &state,
            &Value::tree([(
                "_add",
                Value::List(vec![Value::Int(10), Value::Int(20), Value::Int(30)]),
            )]),
        );
        assert_eq!(state, Value::List(vec![Value::Int(10), Value::Int(20), Value::Int(30)]));
        // _set: replace index 1 with 99
        state = reg.type_apply(
            "chain",
            &state,
            &Value::tree([(
                "_set",
                Value::List(vec![Value::List(vec![Value::Int(1), Value::Int(99)])]),
            )]),
        );
        assert_eq!(state, Value::List(vec![Value::Int(10), Value::Int(99), Value::Int(30)]));
        // _remove: drop index 0
        state = reg.type_apply(
            "chain",
            &state,
            &Value::tree([("_remove", Value::List(vec![Value::Int(0)]))]),
        );
        assert_eq!(state, Value::List(vec![Value::Int(99), Value::Int(30)]));
    }

    #[test]
    fn chain_divide_round_robin_partition() {
        let reg = TypeRegistry::new();
        let state = Value::List((0..10).map(Value::Int).collect());
        let daughters = reg.type_divide("chain", &state, &DivideContext::binary());
        assert_eq!(daughters.len(), 2);
        // Round-robin: daughter 0 gets even indices, daughter 1 gets odd
        let d0 = daughters[0].clone();
        let d1 = daughters[1].clone();
        assert_eq!(d0, Value::List(vec![Value::Int(0), Value::Int(2), Value::Int(4), Value::Int(6), Value::Int(8)]));
        assert_eq!(d1, Value::List(vec![Value::Int(1), Value::Int(3), Value::Int(5), Value::Int(7), Value::Int(9)]));
    }

    #[test]
    fn graph_add_vertices_and_edges() {
        let reg = TypeRegistry::new();
        let mut state = reg.type_default("graph");
        // Add 3 vertices
        let add = Value::tree([(
            "_add_vertex",
            Value::tree([
                ("v0", Value::Float(0.0.into())),
                ("v1", Value::Float(1.0.into())),
                ("v2", Value::Float(2.0.into())),
            ]),
        )]);
        state = reg.type_apply("graph", &state, &add);
        assert_eq!(state.get_field("vertices").and_then(|v| v.as_map()).unwrap().len(), 3);
        // Add 2 edges
        let add_edges = Value::tree([(
            "_add_edge",
            Value::List(vec![
                Value::List(vec![Value::String("v0".into()), Value::String("v1".into())]),
                Value::List(vec![Value::String("v1".into()), Value::String("v2".into())]),
            ]),
        )]);
        state = reg.type_apply("graph", &state, &add_edges);
        if let Some(Value::List(edges)) = state.get_field("edges") {
            assert_eq!(edges.len(), 2);
        } else {
            panic!("expected edges list");
        }
    }

    #[test]
    fn graph_divide_cleaves_cross_daughter_edges() {
        let reg = TypeRegistry::new();
        let state = Value::tree([
            (
                "vertices",
                Value::tree([
                    ("v0", Value::Int(0)),
                    ("v1", Value::Int(1)),
                    ("v2", Value::Int(2)),
                    ("v3", Value::Int(3)),
                ]),
            ),
            (
                "edges",
                Value::List(vec![
                    Value::List(vec![Value::String("v0".into()), Value::String("v1".into())]),
                    Value::List(vec![Value::String("v1".into()), Value::String("v2".into())]), // CROSS
                    Value::List(vec![Value::String("v2".into()), Value::String("v3".into())]),
                ]),
            ),
        ]);
        // Daughter A gets v0+v1; daughter B gets v2+v3. v1-v2 is the cleavage.
        let mut ctx = DivideContext::binary();
        ctx.partition.insert("v0".into(), 0);
        ctx.partition.insert("v1".into(), 0);
        ctx.partition.insert("v2".into(), 1);
        ctx.partition.insert("v3".into(), 1);

        let daughters = reg.type_divide("graph", &state, &ctx);
        let d0_edges = daughters[0].get_field("edges").and_then(|e| match e {
            Value::List(l) => Some(l.clone()),
            _ => None,
        }).unwrap();
        let d1_edges = daughters[1].get_field("edges").and_then(|e| match e {
            Value::List(l) => Some(l.clone()),
            _ => None,
        }).unwrap();
        assert_eq!(d0_edges.len(), 1, "daughter 0 keeps v0-v1");
        assert_eq!(d1_edges.len(), 1, "daughter 1 keeps v2-v3");
        // Cleaved edge v1-v2 is in neither.
    }

    #[test]
    fn alias_inherits_methods() {
        let mut reg = TypeRegistry::new();
        reg.register_alias("polymer_chain", "chain");
        // polymer_chain should support _add semantics via inheritance.
        let state = reg.type_default("polymer_chain");
        assert_eq!(state, Value::List(Vec::new()));
        let updated = reg.type_apply(
            "polymer_chain",
            &state,
            &Value::tree([("_add", Value::List(vec![Value::Int(7)]))]),
        );
        assert_eq!(updated, Value::List(vec![Value::Int(7)]));
    }

    #[test]
    fn inheritance_cycle_doesnt_loop() {
        let mut reg = TypeRegistry::new();
        // Create a fake cycle a → b → a. methods() must not infinite-loop.
        reg.register_full("a", Schema::Any, None, None, vec!["b".to_string()]);
        reg.register_full("b", Schema::Any, None, None, vec!["a".to_string()]);
        // Should return None gracefully.
        assert!(reg.methods("a").is_none());
        assert!(reg.methods("b").is_none());
    }

    // Silence dead-code warning since Counter is used through Any-downcast.
    #[allow(dead_code)]
    fn _counter_any_witness(c: Counter) -> &'static str {
        let _: Box<dyn Any> = Box::new(c);
        "ok"
    }
}
