//! Schema definitions for typed hierarchical state.
//!
//! Schemas describe the structure and types of the state tree.
//! They mirror bigraph-schema's type system but leverage Rust's
//! type system for compile-time safety where possible, with
//! runtime flexibility for dynamic composition.

use std::fmt;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::value::{Key, StateMap, Value};

fn default_interval() -> f64 {
    1.0
}

/// A schema describing the type of a value in the state tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "_type")]
pub enum Schema {
    /// No type constraint
    Any,

    /// Atomic types
    Bool {
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<bool>,
    },
    Integer {
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<i64>,
    },
    Float {
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<f64>,
    },
    String {
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },

    /// A list with typed elements
    List {
        element: Box<Schema>,
    },

    /// A map with string keys and typed values
    Map {
        value: Box<Schema>,
    },

    /// A tree with named, individually-typed branches
    Tree {
        branches: IndexMap<Key, Schema>,
    },

    /// Optional value
    Maybe {
        inner: Box<Schema>,
    },

    /// Enum with allowed values
    Enum {
        values: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },

    /// Delta type — updates are additive rather than replacing.
    /// (Note: Float and Integer are ALSO additive by default.
    ///  Delta is kept for backward compatibility.)
    Delta {
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<f64>,
    },

    /// Overwrite wrapper — updates REPLACE the current value.
    /// Use this for types where replacement semantics are needed
    /// (e.g., positions, absolute state). Wraps any inner schema.
    Overwrite {
        inner: Box<Schema>,
    },

    /// Const wrapper — immutable. apply / merge / reconcile preserve
    /// the current value; updates are silently ignored. Mirrors
    /// upstream `bigraph_schema.schema.Const`.
    Const {
        inner: Box<Schema>,
    },

    /// Quote wrapper — opaque, passes through apply/realize untouched.
    /// Used for values that should be carried as-is (process instances,
    /// binary blobs, etc.). Mirrors upstream `bigraph_schema.schema.Quote`.
    Quote {
        inner: Box<Schema>,
    },

    /// **Place-graph hole.** A site is an open inner-face position in
    /// Milner's place graph (Def. 2.1). A schema with `Site` markers
    /// describes a *context* into which another bigraph can be plugged
    /// during composition. Sites carry no state on their own; once
    /// filled, no site remains.
    ///
    /// `sort` is an optional place-sort label (Milner Ch. 6); empty
    /// string means unsorted.
    Site {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        sort: String,
    },

    /// **Inner link-graph name.** An open link endpoint facing inward
    /// (Milner Def. 2.2). Inner names form the domain of the link map;
    /// during composition `G ∘ F` each outer name of `F` is connected
    /// to the inner name of `G` of the same name.
    ///
    /// `sort` is an optional link-sort label.
    InnerName {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        sort: String,
    },

    /// **Outer link-graph name.** An open link endpoint facing outward
    /// (Milner Def. 2.2). Outer names escape the bigraph and can be
    /// joined to another bigraph's inner name of the same name.
    ///
    /// `sort` is an optional link-sort label.
    OuterName {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        sort: String,
    },

    /// **Bigraphical interface** `I = ⟨m, X⟩` (Milner Def. 2.3).
    /// Pairs a place-graph face (`places`, ordered) with a link-graph
    /// face (`names`, name → sort). The trivial interface
    /// `ε = ⟨0, ∅⟩` is `Interface { places: [], names: {} }`.
    ///
    /// The same node is used for both inner and outer faces; which
    /// side it represents is determined by attachment to a composite
    /// schema.
    Interface {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        places: Vec<Schema>,
        #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
        names: IndexMap<String, String>,
    },

    /// Multidimensional array — element-wise additive apply.
    /// Like Python bigraph-schema's `array` type. Updates are added
    /// element-wise when shapes match.
    Array {
        /// Shape dimensions (e.g., [10, 10] for a 10x10 grid)
        shape: Vec<usize>,
        /// Element type
        element: Box<Schema>,
    },

    /// Tuple — fixed-length, element-wise typed apply.
    /// Like Python bigraph-schema's `tuple` type. Each element
    /// is applied using its own type's semantics.
    Tuple {
        elements: Vec<Schema>,
    },

    /// Recursive tree — a nested dict where every leaf matches the
    /// leaf schema. Like Python bigraph-schema's `tree[float]`.
    /// Values can be either the leaf type or another nested map.
    RecursiveTree {
        leaf: Box<Schema>,
    },

    /// A link (edge) in the bigraph — represents a process or step.
    /// This is the schema-level declaration that a node is computational,
    /// not just data. The engine uses this to identify and instantiate
    /// processes without scanning state for "address" fields.
    ///
    /// The generic `Link` variant is the parent type that doesn't
    /// commit to temporal vs reactive semantics. For concrete process
    /// or step typing, use [`Schema::ProcessLink`] / [`Schema::StepLink`].
    /// `CompositeLink` extends `ProcessLink` with inner-state schema +
    /// bridge wiring.
    ///
    /// Corresponds to Python bigraph-schema's `Link` type and
    /// process-bigraph's `ProcessLink`/`StepLink`/`CompositeLink`.
    Link {
        /// Schema for input ports: port_name → type
        inputs: IndexMap<Key, Schema>,
        /// Schema for output ports: port_name → type
        outputs: IndexMap<Key, Schema>,
        /// Whether this link has a temporal interval (process) or not (step).
        /// None means unspecified (inferred at instantiation time).
        temporal: Option<bool>,
    },

    /// A reactive step — fires on input change, no time interval.
    /// Mirrors `process_bigraph.types.process.StepLink`. Carries an
    /// optional priority for when multiple steps trigger simultaneously
    /// (higher priority runs first).
    StepLink {
        inputs: IndexMap<Key, Schema>,
        outputs: IndexMap<Key, Schema>,
        #[serde(default)]
        priority: f64,
    },

    /// A temporal process — runs every `interval` time units.
    /// Mirrors `process_bigraph.types.process.ProcessLink`.
    ProcessLink {
        inputs: IndexMap<Key, Schema>,
        outputs: IndexMap<Key, Schema>,
        #[serde(default = "default_interval")]
        interval: f64,
    },

    /// A composite link — a process whose body is itself a state subtree
    /// with embedded sub-processes. The composite's *interface* is
    /// `inputs`/`outputs`; its *internal schema* describes the shape of
    /// its inner state. Mirrors
    /// `process_bigraph.types.process.CompositeLink`.
    ///
    /// At instantiation time, the engine constructs a sub-engine running
    /// the inner schema and bridges its observable slots to the outer
    /// interface via the spec's `inputs`/`outputs` wires.
    CompositeLink {
        inputs: IndexMap<Key, Schema>,
        outputs: IndexMap<Key, Schema>,
        #[serde(default = "default_interval")]
        interval: f64,
        /// Schema for the composite's internal state.
        inner_schema: Box<Schema>,
    },

    /// `Bridge` — a wiring map from port names to internal state paths.
    /// First-class so wiring can be inspected / mutated like any other
    /// schema-typed value. Mirrors
    /// `process_bigraph.types.process.Bridge`.
    Bridge {
        /// External port → internal state path
        #[serde(default)]
        inputs: IndexMap<Key, Vec<Key>>,
        #[serde(default)]
        outputs: IndexMap<Key, Vec<Key>>,
    },

    /// Reference to a registered named type in the `TypeRegistry`. The
    /// engine dispatches `apply`/`divide`/`serialize`/etc. through the
    /// registry's `TypeMethods` for the named type. Mirrors
    /// bigraph-schema's `{'_type': 'X', ...}` dict references.
    ///
    /// **Parameters** carry per-instance configuration (e.g.,
    /// `{element: float}` for `list[float]`). They're stored as nested
    /// schemas and may be read by the type's methods.
    Custom {
        /// Registered name. Must exist in the `TypeRegistry` at dispatch
        /// time; otherwise apply/serialize/etc. fall back to defaults.
        name: String,
        /// Optional type parameters. Empty for parameter-free types
        /// (e.g., `sacculus`); non-empty for parameterized ones
        /// (e.g., `list { element: float }`).
        #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
        parameters: IndexMap<Key, Schema>,
    },
}

impl Schema {
    // ── Convenience constructors ──

    pub fn float() -> Self {
        Self::Float { default: None }
    }

    pub fn float_default(v: f64) -> Self {
        Self::Float { default: Some(v) }
    }

    pub fn integer() -> Self {
        Self::Integer { default: None }
    }

    pub fn bool() -> Self {
        Self::Bool { default: None }
    }

    pub fn string() -> Self {
        Self::String { default: None }
    }

    pub fn delta() -> Self {
        Self::Delta { default: Some(0.0) }
    }

    pub fn list(element: Schema) -> Self {
        Self::List {
            element: Box::new(element),
        }
    }

    pub fn map(value: Schema) -> Self {
        Self::Map {
            value: Box::new(value),
        }
    }

    pub fn overwrite(inner: Schema) -> Self {
        Self::Overwrite {
            inner: Box::new(inner),
        }
    }

    pub fn const_of(inner: Schema) -> Self {
        Self::Const {
            inner: Box::new(inner),
        }
    }

    pub fn quote_of(inner: Schema) -> Self {
        Self::Quote {
            inner: Box::new(inner),
        }
    }

    pub fn site() -> Self {
        Self::Site {
            sort: String::new(),
        }
    }

    pub fn site_sorted(sort: impl Into<String>) -> Self {
        Self::Site { sort: sort.into() }
    }

    pub fn inner_name(sort: impl Into<String>) -> Self {
        Self::InnerName { sort: sort.into() }
    }

    pub fn outer_name(sort: impl Into<String>) -> Self {
        Self::OuterName { sort: sort.into() }
    }

    pub fn interface() -> Self {
        Self::Interface {
            places: vec![],
            names: IndexMap::new(),
        }
    }

    pub fn step_link(
        inputs: IndexMap<Key, Schema>,
        outputs: IndexMap<Key, Schema>,
    ) -> Self {
        Self::StepLink {
            inputs,
            outputs,
            priority: 0.0,
        }
    }

    pub fn step_link_with_priority(
        inputs: IndexMap<Key, Schema>,
        outputs: IndexMap<Key, Schema>,
        priority: f64,
    ) -> Self {
        Self::StepLink {
            inputs,
            outputs,
            priority,
        }
    }

    pub fn process_link(
        inputs: IndexMap<Key, Schema>,
        outputs: IndexMap<Key, Schema>,
    ) -> Self {
        Self::ProcessLink {
            inputs,
            outputs,
            interval: 1.0,
        }
    }

    pub fn process_link_with_interval(
        inputs: IndexMap<Key, Schema>,
        outputs: IndexMap<Key, Schema>,
        interval: f64,
    ) -> Self {
        Self::ProcessLink {
            inputs,
            outputs,
            interval,
        }
    }

    pub fn composite_link(
        inputs: IndexMap<Key, Schema>,
        outputs: IndexMap<Key, Schema>,
        inner_schema: Schema,
    ) -> Self {
        Self::CompositeLink {
            inputs,
            outputs,
            interval: 1.0,
            inner_schema: Box::new(inner_schema),
        }
    }

    pub fn bridge() -> Self {
        Self::Bridge {
            inputs: IndexMap::new(),
            outputs: IndexMap::new(),
        }
    }

    /// Return the inputs/outputs port schemas for any link-typed
    /// schema (Link, StepLink, ProcessLink, CompositeLink). Useful for
    /// uniform engine code that doesn't care which kind of link it is.
    pub fn link_ports(&self) -> Option<(&IndexMap<Key, Schema>, &IndexMap<Key, Schema>)> {
        match self {
            Self::Link { inputs, outputs, .. }
            | Self::StepLink { inputs, outputs, .. }
            | Self::ProcessLink { inputs, outputs, .. }
            | Self::CompositeLink { inputs, outputs, .. } => Some((inputs, outputs)),
            _ => None,
        }
    }

    /// True if this schema is any kind of link/process/composite.
    pub fn is_link_kind(&self) -> bool {
        matches!(
            self,
            Self::Link { .. }
                | Self::StepLink { .. }
                | Self::ProcessLink { .. }
                | Self::CompositeLink { .. }
        )
    }

    /// Overwrite float — for positions, absolute values.
    pub fn set_float() -> Self {
        Self::overwrite(Self::float())
    }

    /// Array type — element-wise additive.
    pub fn array(shape: Vec<usize>, element: Schema) -> Self {
        Self::Array { shape, element: Box::new(element) }
    }

    /// Tuple type — element-wise typed apply.
    pub fn tuple(elements: Vec<Schema>) -> Self {
        Self::Tuple { elements }
    }

    /// Create a link schema (process or step).
    pub fn link(inputs: IndexMap<Key, Schema>, outputs: IndexMap<Key, Schema>) -> Self {
        Self::Link { inputs, outputs, temporal: None }
    }

    /// Create a process link (temporal — has interval).
    pub fn process(inputs: IndexMap<Key, Schema>, outputs: IndexMap<Key, Schema>) -> Self {
        Self::Link { inputs, outputs, temporal: Some(true) }
    }

    /// Create a step link (non-temporal — fires on state change).
    pub fn step(inputs: IndexMap<Key, Schema>, outputs: IndexMap<Key, Schema>) -> Self {
        Self::Link { inputs, outputs, temporal: Some(false) }
    }

    pub fn recursive_tree(leaf: Schema) -> Self {
        Self::RecursiveTree { leaf: Box::new(leaf) }
    }

    pub fn maybe(inner: Schema) -> Self {
        Self::Maybe {
            inner: Box::new(inner),
        }
    }

    pub fn tree(
        branches: impl IntoIterator<Item = (impl Into<Key>, Schema)>,
    ) -> Self {
        Self::Tree {
            branches: branches
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
        }
    }

    /// Compile a Tree schema into a StructLayout for O(1) field access.
    /// Returns None for non-Tree schemas.
    pub fn compile_layout(&self) -> Option<std::sync::Arc<crate::value::StructLayout>> {
        match self {
            Self::Tree { branches } => {
                let fields: Vec<Key> = branches.keys().cloned().collect();
                Some(crate::value::StructLayout::new(fields))
            }
            _ => None,
        }
    }

    /// Recursively compile a value tree into Struct values wherever
    /// the schema declares a Tree with known branches AND no child
    /// uses Map/dynamic semantics. Container-level trees (with particles,
    /// fields, etc.) stay as Maps since processes iterate their keys.
    pub fn compile_value(&self, value: &Value) -> Value {
        match (self, value) {
            (Self::Tree { branches }, Value::Map(map)) => {
                // Only compile if no branch has Map/RecursiveTree schema
                // (those need dynamic key iteration which Struct doesn't support)
                let has_dynamic = branches.values().any(|s| matches!(s,
                    Schema::Map { .. } | Schema::RecursiveTree { .. }
                ));
                if has_dynamic {
                    // Keep as Map but recursively compile children
                    let compiled: StateMap = map.iter()
                        .map(|(k, v)| {
                            let child_schema = branches.get(k).unwrap_or(&Schema::Any);
                            (k.clone(), child_schema.compile_value(v))
                        })
                        .collect();
                    Value::Map(compiled)
                } else {
                    // Safe to compile to Struct — all children are fixed-structure
                    let layout = self.compile_layout().unwrap();
                    let values: Vec<Value> = layout.fields.iter()
                        .map(|k| {
                            let child_schema = branches.get(k).unwrap_or(&Schema::Any);
                            let child_val = map.get(k).unwrap_or(&Value::None);
                            child_schema.compile_value(child_val)
                        })
                        .collect();
                    Value::Struct { layout, values }
                }
            }
            _ => value.clone(),
        }
    }

    /// Generate a default value for this schema.
    pub fn default_value(&self) -> Value {
        match self {
            Self::Any => Value::None,
            Self::Bool { default } => Value::Bool(default.unwrap_or(false)),
            Self::Integer { default } => Value::Int(default.unwrap_or(0)),
            Self::Float { default } => Value::float(default.unwrap_or(0.0)),
            Self::String { default } => {
                Value::String(default.clone().unwrap_or_default())
            }
            Self::List { .. } => Value::List(vec![]),
            Self::Map { .. } => Value::map(),
            Self::Tree { branches } => Value::tree(
                branches
                    .iter()
                    .map(|(k, s)| (k.clone(), s.default_value())),
            ),
            Self::Maybe { .. } => Value::None,
            Self::Enum { default, values } => {
                Value::String(default.clone().unwrap_or_else(|| {
                    values.first().cloned().unwrap_or_default()
                }))
            }
            Self::Delta { default } => Value::float(default.unwrap_or(0.0)),
            Self::Overwrite { inner } => inner.default_value(),
            Self::Array { shape, element } => {
                // Build nested list matching shape dimensions
                fn build_array(dims: &[usize], elem: &Schema) -> Value {
                    if dims.is_empty() {
                        return elem.default_value();
                    }
                    let inner = if dims.len() == 1 {
                        (0..dims[0]).map(|_| elem.default_value()).collect()
                    } else {
                        (0..dims[0]).map(|_| build_array(&dims[1..], elem)).collect()
                    };
                    Value::List(inner)
                }
                build_array(shape, element)
            }
            Self::Tuple { elements } => {
                Value::List(elements.iter().map(|s| s.default_value()).collect())
            }
            Self::RecursiveTree { .. } => Value::map(),
            Self::Link { inputs, .. } => {
                // Default link state: address, default wiring (port→[port])
                let mut state = IndexMap::new();
                state.insert(Key::from("address"), Value::String("local:edge".into()));
                let default_inputs: IndexMap<Key, Value> = inputs.keys()
                    .map(|k| (k.clone(), Value::List(vec![Value::String(k.to_string())])))
                    .collect();
                state.insert(Key::from("inputs"), Value::Map(default_inputs.clone()));
                state.insert(Key::from("outputs"), Value::Map(default_inputs));
                Value::Map(state)
            }
            // Custom types' real defaults come from `TypeMethods::default`
            // dispatched through the `TypeRegistry`. With no registry
            // available at the Schema level, fall back to None.
            Self::Custom { .. } => Value::None,
            // Wrap-style: delegate to inner (Const/Quote)
            Self::Const { inner } | Self::Quote { inner } => inner.default_value(),
            // Empty types — no value
            Self::Site { .. }
            | Self::InnerName { .. }
            | Self::OuterName { .. }
            | Self::Interface { .. } => Value::None,
            // Typed link variants — default to a process spec shape
            // matching what `Schema::Link` returns.
            Self::StepLink { inputs, .. }
            | Self::ProcessLink { inputs, .. }
            | Self::CompositeLink { inputs, .. } => {
                let mut map: IndexMap<Key, Value> = IndexMap::new();
                let mut input_defaults: IndexMap<Key, Value> = IndexMap::new();
                for (port, schema) in inputs {
                    input_defaults.insert(port.clone(), schema.default_value());
                }
                map.insert(Key::from("address"), Value::String("local:Unknown".into()));
                map.insert(Key::from("config"), Value::map());
                map.insert(Key::from("inputs"), Value::Map(input_defaults));
                Value::Map(map)
            }
            // Bridge: empty wiring map
            Self::Bridge { .. } => Value::map(),
        }
    }

    /// Check if a value conforms to this schema.
    pub fn check(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Any, _) => true,
            (Self::Bool { .. }, Value::Bool(_)) => true,
            (Self::Integer { .. }, Value::Int(_)) => true,
            (Self::Float { .. }, Value::Float(_)) => true,
            (Self::Float { .. }, Value::Int(_)) => true, // int is valid as float
            (Self::Delta { .. }, Value::Float(_)) => true,
            (Self::Delta { .. }, Value::Int(_)) => true,
            (Self::Overwrite { inner }, v) => inner.check(v),
            (Self::String { .. }, Value::String(_)) => true,
            (Self::Enum { values, .. }, Value::String(s)) => values.contains(s),
            (Self::Maybe { .. }, Value::None) => true,
            (Self::Maybe { inner }, v) => inner.check(v),
            (Self::List { element }, Value::List(items)) => {
                items.iter().all(|item| element.check(item))
            }
            (Self::Map { value: val_schema }, Value::Map(map)) => {
                map.values().all(|v| val_schema.check(v))
            }
            (Self::Tree { branches }, Value::Map(map)) => branches
                .iter()
                .all(|(k, s)| map.get(k).is_some_and(|v| s.check(v))),
            (Self::Array { element, .. }, Value::List(items)) => {
                items.iter().all(|item| match item {
                    Value::List(row) => row.iter().all(|v| element.check(v)),
                    _ => element.check(item),
                })
            }
            (Self::Tuple { elements }, Value::List(items)) => {
                elements.len() == items.len()
                    && elements.iter().zip(items.iter()).all(|(s, v)| s.check(v))
            }
            (
                Self::Link { .. }
                | Self::StepLink { .. }
                | Self::ProcessLink { .. }
                | Self::CompositeLink { .. },
                Value::Map(map),
            ) => {
                // A realized link must have an "instance" key (after instantiation)
                // An unrealized link has "address" + "inputs" + "outputs"
                map.contains_key("instance") || map.contains_key("address")
            }
            (Self::RecursiveTree { leaf }, v) => {
                // A recursive tree value is either a leaf or a map of recursive trees
                if leaf.check(v) {
                    true
                } else if let Value::Map(map) = v {
                    map.values().all(|child| Self::RecursiveTree { leaf: leaf.clone() }.check(child))
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Walk the schema tree to find the sub-schema at a given path.
    /// For example, path ["fields", "glucose"] in Tree{fields: Map(Array(Float))}
    /// returns Array(Float).
    pub fn schema_at_path(&self, path: &[Key]) -> &Schema {
        if path.is_empty() {
            return self;
        }
        match self {
            Self::Tree { branches } => {
                if let Some(child) = branches.get(&path[0]) {
                    child.schema_at_path(&path[1..])
                } else {
                    &Schema::Any
                }
            }
            Self::Map { value } => value.schema_at_path(&path[1..]),
            Self::Array { element, .. } => element.schema_at_path(&path[1..]),
            Self::RecursiveTree { .. } => self,
            Self::Link { inputs, outputs, .. } => {
                // Navigate into link's port schemas
                match path[0].as_str() {
                    "inputs" => {
                        if path.len() > 1 {
                            inputs.get(&path[1]).unwrap_or(&Schema::Any)
                                .schema_at_path(&path[2..])
                        } else {
                            &Schema::Any
                        }
                    }
                    "outputs" => {
                        if path.len() > 1 {
                            outputs.get(&path[1]).unwrap_or(&Schema::Any)
                                .schema_at_path(&path[2..])
                        } else {
                            &Schema::Any
                        }
                    }
                    _ => &Schema::Any,
                }
            }
            _ => self, // Leaf schema applies to everything below
        }
    }

    /// Infer a schema from a value, including `_type` annotations.
    ///
    /// This is the Rust equivalent of Python bigraph-schema's `infer` +
    /// `realize` for state with embedded type declarations. When a map
    /// contains `_type`, it's used to determine the schema for that node.
    pub fn infer(value: &Value) -> Schema {
        match value {
            Value::Float(_) => Schema::float(),
            Value::Int(_) => Schema::integer(),
            Value::Bool(_) => Schema::bool(),
            Value::String(s) => {
                // Try to parse as a number → infer float
                if s.parse::<f64>().is_ok() {
                    Schema::float()
                } else {
                    Schema::string()
                }
            }
            Value::List(_) => Schema::List { element: Box::new(Schema::Any) },
            Value::Map(map) => {
                // Check for _type annotation
                if let Some(Value::String(type_str)) = map.get("_type") {
                    let base = crate::type_parser::parse_type_expression(type_str);
                    // For Link types without port info, infer ports from
                    // _inputs/_outputs in the state (if present)
                    if matches!(base, Schema::Link { .. }) {
                        if let Schema::Link { inputs, outputs, temporal } = &base {
                            if inputs.is_empty() && outputs.is_empty() {
                                let inferred_inputs = map.get("_inputs")
                                    .and_then(|v| v.as_map())
                                    .map(|m| m.iter()
                                        .map(|(k, v)| (k.clone(), crate::type_parser::parse_type_expression(
                                            v.as_str().unwrap_or("any"))))
                                        .collect())
                                    .unwrap_or_default();
                                let inferred_outputs = map.get("_outputs")
                                    .and_then(|v| v.as_map())
                                    .map(|m| m.iter()
                                        .map(|(k, v)| (k.clone(), crate::type_parser::parse_type_expression(
                                            v.as_str().unwrap_or("any"))))
                                        .collect())
                                    .unwrap_or_default();
                                return Schema::Link {
                                    inputs: inferred_inputs,
                                    outputs: inferred_outputs,
                                    temporal: *temporal,
                                };
                            }
                        }
                    }
                    return base;
                }
                // Infer as Tree with branches
                let branches: IndexMap<Key, Schema> = map.iter()
                    .filter(|(k, _)| !k.starts_with('_'))
                    .map(|(k, v)| (k.clone(), Schema::infer(v)))
                    .collect();
                if branches.is_empty() {
                    Schema::Any
                } else {
                    Schema::Tree { branches }
                }
            }
            Value::None => Schema::Any,
            _ => Schema::Any,
        }
    }

    /// Apply an update to a current value using type-dispatched semantics.
    ///
    /// Default behavior by type:
    /// - `Float`, `Integer`, `Delta` → **additive** (current + update). Commutative.
    /// - `Overwrite` → **replacement** (returns update directly).
    /// - `Bool`, `String`, `Enum` → **replacement** (non-numeric, can't add).
    /// - `List` → **replacement** (lists replace wholesale).
    /// - `Map` → **recursive merge** (each key applies independently).
    /// - `Tree` → **recursive merge** with per-branch schemas.
    /// - `Any` → **inferred**: additive for numbers, merge for maps, replace otherwise.
    /// The apply op's core (dispatch on sort). **Module-private**: external
    /// crates call [`crate::algebra::apply`] — the single public door — so the
    /// apply surface stays inside the algebra.
    /// The DATA face of a process/composite **node** — its scalar output ports
    /// (`Float`/`Delta`/`Integer`), keyed by port name. These are the fields a
    /// node carries *on itself* when it exports a face onto its own node (a cell's
    /// `mass` via `%.mass`): extensive `Delta`/`Integer` split on divide + apply
    /// additively; intensive `Float` shares; the spec (`address`/`config`/wiring)
    /// and any non-scalar keys pass through. This is **uniform across node kinds**
    /// — `Link`/`ProcessLink`/`StepLink`/`CompositeLink` — so `apply`/`divide`/
    /// `reconcile` treat a node's concurrent field-writes identically regardless
    /// of kind (a composite was special-cased; a pure process that self-exports a
    /// face was silently mishandled). A node with no exported scalar face → empty
    /// → the value passes through unchanged (the old replace/share/last-wins
    /// behaviour for pure specs). Empty for non-node schemas.
    pub fn node_data_branches(&self) -> IndexMap<Key, Schema> {
        let outputs = match self {
            Schema::Link { outputs, .. }
            | Schema::ProcessLink { outputs, .. }
            | Schema::StepLink { outputs, .. }
            | Schema::CompositeLink { outputs, .. } => outputs,
            _ => return IndexMap::new(),
        };
        outputs
            .iter()
            .filter(|(_, s)| {
                matches!(
                    s,
                    Schema::Float { .. } | Schema::Delta { .. } | Schema::Integer { .. }
                )
            })
            .map(|(k, s)| (k.clone(), s.clone()))
            .collect()
    }

    pub(crate) fn apply_update(&self, current: &Value, update: &Value) -> Value {
        match self {
            // Const: immutable — apply is a no-op, current value preserved.
            // Mirrors upstream `bigraph_schema.methods.apply` on Const.
            Self::Const { .. } => current.clone(),

            // Quote: opaque passthrough — last update wins as-is.
            // Mirrors upstream `bigraph_schema.methods.apply` on Quote.
            Self::Quote { .. } => update.clone(),

            // Empty bigraph types carry no state — current preserved.
            Self::Site { .. }
            | Self::InnerName { .. }
            | Self::OuterName { .. }
            | Self::Interface { .. } => current.clone(),

            // Numeric types: additive (delta) by default
            Self::Float { .. } | Self::Delta { .. } => {
                let base = current.as_f64().unwrap_or(0.0);
                let delta = update.as_f64().unwrap_or(0.0);
                Value::float(base + delta)
            }
            Self::Integer { .. } => {
                let base = current.as_i64().unwrap_or(0);
                let delta = update.as_i64().unwrap_or(0);
                Value::Int(base + delta)
            }

            // Overwrite: always replace
            Self::Overwrite { .. } => update.clone(),

            // Non-numeric atoms: replace
            Self::Bool { .. } | Self::String { .. } | Self::Enum { .. } => {
                update.clone()
            }

            // Tree: recursive merge with per-branch schemas.
            // Handles both Map and Struct current values.
            Self::Tree { branches } => {
                match (current, update) {
                    // Struct current + Map update (common: process delta applied to compiled state)
                    (Value::Struct { layout, values }, Value::Map(upd)) => {
                        let mut new_values = values.clone();
                        // Note: _add/_remove not supported on Struct (fixed fields)
                        for (k, v) in upd {
                            if k == "_add" || k == "_remove" { continue; }
                            if let Some(idx) = layout.index_of(k) {
                                let schema = branches.get(k).unwrap_or(&Schema::Any);
                                let existing = &values[idx];
                                new_values[idx] = schema.apply_update(existing, v);
                            }
                        }
                        Value::Struct { layout: layout.clone(), values: new_values }
                    }
                    // Map current + Map update (original path)
                    (Value::Map(cur), Value::Map(upd)) => {
                        let mut result = cur.clone();
                        apply_add_remove(&mut result, upd);
                        for (k, v) in upd {
                            if k == "_add" || k == "_remove" { continue; }
                            let schema = branches.get(k).unwrap_or(&Schema::Any);
                            // post-`_add` base (see Map arm): a re-added key composes
                            // with a concurrent update instead of reverting.
                            let existing = result.get(k).cloned().unwrap_or(Value::None);
                            result.insert(k.clone(), schema.apply_update(&existing, v));
                        }
                        Value::Map(result)
                    }
                    _ => update.clone(),
                }
            }

            // Map: recursive merge with _add/_remove support
            Self::Map { value: val_schema } => {
                if let (Value::Map(cur), Value::Map(upd)) = (current, update) {
                    let mut result = cur.clone();
                    apply_add_remove(&mut result, upd);
                    for (k, v) in upd {
                        if k == "_add" || k == "_remove" {
                            continue;
                        }
                        // Base on `result` (post `_add`/`_remove`), not `cur`: a
                        // key that was just re-added (`_add`) must compose with a
                        // concurrent same-key update, not be reverted to the old
                        // value.
                        let existing = result.get(k).cloned().unwrap_or(Value::None);
                        result.insert(k.clone(), val_schema.apply_update(&existing, v));
                    }
                    Value::Map(result)
                } else {
                    update.clone()
                }
            }

            // Lists: a structural `{_add, _remove}` update appends / removes
            // (faithful to upstream `apply(List)`); `_remove` is a list of
            // indices (or "all"); `_add` is appended. A plain-list update
            // replaces. This is what lets a collection delta — e.g. a graph
            // type's `add_node` returning `{nodes: {_add: [x]}}` — land and
            // compose like `_add` on a map.
            Self::List { .. } => match update {
                Value::Map(upd) if upd.contains_key("_add") || upd.contains_key("_remove") => {
                    let mut result: Vec<Value> =
                        current.as_list().map(<[Value]>::to_vec).unwrap_or_default();
                    // `_remove` removes **by value** (a list treated as a set:
                    // drop every element equal to one listed), or `"all"`. This
                    // is what lets a Custom type's `remove_node(x)` /
                    // `remove_edge(a,b)` be a composable `_remove` delta.
                    match upd.get("_remove") {
                        Some(Value::String(s)) if s == "all" => result.clear(),
                        Some(Value::List(vals)) => result.retain(|v| !vals.contains(v)),
                        _ => {}
                    }
                    if let Some(Value::List(adds)) = upd.get("_add") {
                        result.extend(adds.iter().cloned());
                    }
                    Value::List(result)
                }
                _ => update.clone(),
            },

            // Maybe: delegate to inner when both non-None, otherwise replace
            Self::Maybe { inner } => {
                match (current, update) {
                    (Value::None, _) => update.clone(),
                    (_, Value::None) => Value::None,
                    _ => inner.apply_update(current, update),
                }
            }

            // Array: element-wise additive apply over the numeric leaves —
            // **representation-agnostic**. The shape is grid metadata; the
            // value may be stored nested (`[[…],[…]]`) OR flat row-major
            // (`[…]`, the spatio-flux convention). At each position: if both
            // sides are sub-lists, recurse as a sub-array; otherwise the leaf
            // is numeric and the element schema (additive `Float`/`Delta`)
            // applies. This makes a flat field with an `array[ny,nx]` schema
            // sum element-wise (mass-conserving) instead of replacing.
            Self::Array { shape, element } => {
                match (current, update) {
                    (Value::List(cur), Value::List(upd)) if cur.len() == upd.len() => {
                        let sub_array = Schema::Array {
                            shape: if shape.len() > 1 { shape[1..].to_vec() } else { shape.clone() },
                            element: element.clone(),
                        };
                        Value::List(
                            cur.iter().zip(upd.iter())
                                .map(|(c, u)| match (c, u) {
                                    // Nested → recurse one dimension deeper.
                                    (Value::List(_), Value::List(_)) => sub_array.apply_update(c, u),
                                    // Flat numeric leaf → additive element apply.
                                    _ => element.apply_update(c, u),
                                })
                                .collect()
                        )
                    }
                    _ => update.clone(),
                }
            }

            // Tuple: element-wise typed apply.
            Self::Tuple { elements } => {
                match (current, update) {
                    (Value::List(cur), Value::List(upd)) if cur.len() == upd.len() => {
                        Value::List(
                            cur.iter().zip(upd.iter()).enumerate()
                                .map(|(i, (c, u))| {
                                    let schema = elements.get(i).unwrap_or(&Schema::Any);
                                    schema.apply_update(c, u)
                                })
                                .collect()
                        )
                    }
                    _ => update.clone(),
                }
            }

            // Any Link-kind NODE (Link / StepLink / ProcessLink / CompositeLink):
            // a node may carry a data face on itself (a cell's exported `mass` via
            // `%.mass` — scalar output ports become node-local fields). Apply
            // STRUCTURALLY over the data face (extensive `Delta` additive, etc.) so
            // a bridged delta (`{mass:+Δ}`) composes; the spec (`address`/`config`/
            // wiring) + non-face keys pass through (via `Any` on unbranched keys).
            // For nodes WITHOUT a self-exported face, `node_data_branches` is empty
            // → the Tree apply collapses to the same per-key Any merge that a pure
            // spec value would receive (spec keys preserved, update overlays).
            Self::Link { .. }
            | Self::StepLink { .. }
            | Self::ProcessLink { .. }
            | Self::CompositeLink { .. } => Schema::Tree {
                branches: self.node_data_branches(),
            }
            .apply_update(current, update),

            // Bridge: wiring data, replaced wholesale on update.
            Self::Bridge { .. } => update.clone(),

            // RecursiveTree: merge like Map with leaf-type apply
            Self::RecursiveTree { leaf } => {
                match (current, update) {
                    (Value::Map(cur), Value::Map(upd)) => {
                        let mut result = cur.clone();
                        apply_add_remove(&mut result, upd);
                        for (k, v) in upd {
                            if k == "_add" || k == "_remove" { continue; }
                            // post-`_add` base (see Map arm).
                            let existing = result.get(k).cloned().unwrap_or(Value::None);
                            match (&existing, v) {
                                // Both maps: recurse as tree
                                (Value::Map(_), Value::Map(_)) => {
                                    result.insert(k.clone(), self.apply_update(&existing, v));
                                }
                                // Both leaves: apply leaf semantics
                                (_, _) if existing.as_map().is_none() && v.as_map().is_none() => {
                                    result.insert(k.clone(), leaf.apply_update(&existing, v));
                                }
                                // Type mismatch (map vs leaf): update replaces
                                _ => {
                                    result.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        Value::Map(result)
                    }
                    _ => leaf.apply_update(current, update),
                }
            }

            // Custom: dispatched through TypeRegistry — at the Schema
            // level we don't have the registry, so fall back to update
            // (replace). Engine integration uses `apply_update_with`
            // to consult the registry instead.
            Self::Custom { .. } => update.clone(),

            // Any: infer behavior from the value types
            Self::Any => {
                match (current, update) {
                    // Both numeric → additive
                    (Value::Float(_) | Value::Int(_), Value::Float(_) | Value::Int(_)) => {
                        let base = current.as_f64().unwrap_or(0.0);
                        let delta = update.as_f64().unwrap_or(0.0);
                        Value::float(base + delta)
                    }
                    // Both maps → recursive merge with _add/_remove
                    (Value::Map(cur), Value::Map(upd)) => {
                        let mut result = cur.clone();
                        apply_add_remove(&mut result, upd);
                        for (k, v) in upd {
                            if k == "_add" || k == "_remove" {
                                continue;
                            }
                            // post-`_add` base (see Map arm).
                            let existing = result.get(k).cloned().unwrap_or(Value::None);
                            result.insert(k.clone(), Schema::Any.apply_update(&existing, v));
                        }
                        Value::Map(result)
                    }
                    // Struct current + Map update → update fields in place
                    (Value::Struct { layout, values }, Value::Map(upd)) => {
                        let mut new_values = values.clone();
                        for (k, v) in upd {
                            if k == "_add" || k == "_remove" { continue; }
                            if let Some(idx) = layout.index_of(k) {
                                let existing = &values[idx];
                                new_values[idx] = Schema::Any.apply_update(existing, v);
                            }
                        }
                        Value::Struct { layout: layout.clone(), values: new_values }
                    }
                    // Both lists → replace (use Array schema for element-wise additive)
                    (Value::List(_), Value::List(_)) => update.clone(),
                    // Otherwise → replace
                    _ => update.clone(),
                }
            }
        }
    }

    /// Apply an update consulting an optional `TypeRegistry` for
    /// `Schema::Custom` dispatch. When `registry` is `Some` and the
    /// schema is `Custom`, the registered `TypeMethods::apply` is
    /// invoked; otherwise this delegates to [`Self::apply_update`].
    ///
    /// This is the **registry-aware** entry point the engine uses;
    /// `apply_update` remains the standalone (no-registry) version.
    /// Registry-aware apply (Custom dispatch + `_divide` sentinel).
    /// **Module-private**: external crates call [`crate::algebra::apply_with`].
    pub(crate) fn apply_update_with(
        &self,
        registry: Option<&crate::registry::TypeRegistry>,
        current: &Value,
        update: &Value,
    ) -> Value {
        if let Self::Custom { name, .. } = self {
            if let Some(reg) = registry {
                return reg.type_apply(name, current, update);
            }
        }
        // `_divide` sentinel on a Map: split the named child by the value
        // schema, drop the mother, install the daughters. Faithful port of
        // bigraph-schema `_handle_divide_sentinel`; needs the registry to
        // run the schema-driven divide.
        if let (Self::Map { value }, Some(reg)) = (self, registry) {
            if let Some(um) = update.as_map() {
                if um.contains_key("_divide") {
                    // Apply the regular (non-`_divide`) part FIRST — sibling
                    // entries' deltas AND the mother's own delta from the same
                    // tick — THEN enact the divide on the result. So a tick that
                    // both grows and divides conserves mass (the mother is split
                    // at its post-growth value, and no co-located delta is
                    // dropped). Returning early on `_divide` alone silently
                    // discarded those deltas — a conservation leak.
                    let mut rest = um.clone();
                    rest.shift_remove("_divide");
                    let base = if rest.is_empty() {
                        current.clone()
                    } else {
                        self.apply_update_with(registry, current, &Value::Map(rest))
                    };
                    return apply_divide_sentinel(value, reg, &base, um);
                }
            }
        }
        self.apply_update(current, update)
    }

    /// Serialize a typed value to a JSON-compatible representation.
    ///
    /// This is the inverse of `realize`. Numbers, strings, and bools
    /// pass through. Maps and trees recurse. Links encode their
    /// address and port schemas.
    pub fn encode(&self, value: &Value) -> Value {
        match (self, value) {
            // Atoms pass through
            (Self::Float { .. } | Self::Delta { .. }, _) => value.clone(),
            (Self::Integer { .. }, _) => value.clone(),
            (Self::Bool { .. }, _) => value.clone(),
            (Self::String { .. } | Self::Enum { .. }, _) => value.clone(),
            (Self::Any, _) => value.clone(),

            // Overwrite/Maybe: delegate to inner
            (Self::Overwrite { inner }, _) => inner.encode(value),
            (Self::Maybe { .. }, Value::None) => Value::None,
            (Self::Maybe { inner }, _) => inner.encode(value),

            // Const: encode via inner — the value originally satisfies
            // the inner schema; immutability doesn't affect encoding.
            (Self::Const { inner }, _) => inner.encode(value),

            // Quote: opaque — pass through verbatim.
            (Self::Quote { .. }, _) => value.clone(),

            // Empty bigraph types — no state to encode.
            (Self::Site { .. }, _) => Value::None,
            (Self::InnerName { .. }, _) => Value::None,
            (Self::OuterName { .. }, _) => Value::None,
            (Self::Interface { .. }, _) => Value::None,

            // List: serialize each element
            (Self::List { element }, Value::List(items)) => {
                Value::List(items.iter().map(|v| element.encode(v)).collect())
            }

            // Map: serialize each value
            (Self::Map { value: val_schema }, Value::Map(map)) => {
                Value::Map(map.iter()
                    .map(|(k, v)| (k.clone(), val_schema.encode(v)))
                    .collect())
            }

            // Tree: serialize each branch with its schema
            (Self::Tree { branches }, Value::Map(map)) => {
                Value::Map(map.iter()
                    .map(|(k, v)| {
                        let s = branches.get(k).unwrap_or(&Schema::Any);
                        (k.clone(), s.encode(v))
                    })
                    .collect())
            }

            // Tuple: element-wise serialize
            (Self::Tuple { elements }, Value::List(items)) => {
                Value::List(items.iter().enumerate()
                    .map(|(i, v)| {
                        elements.get(i).unwrap_or(&Schema::Any).encode(v)
                    })
                    .collect())
            }

            // Array: pass through (already numeric lists)
            (Self::Array { .. }, _) => value.clone(),

            // RecursiveTree: serialize leaves, recurse maps
            (Self::RecursiveTree { .. }, Value::Map(map)) => {
                Value::Map(map.iter()
                    .map(|(k, v)| (k.clone(), self.encode(v)))
                    .collect())
            }
            (Self::RecursiveTree { leaf }, _) => leaf.encode(value),

            // Link-kind: per-key encode over `node_data_branches` (the self-
            // exported data face) with spec keys flowing through `Any` (pass-
            // through). For `Schema::Link` we additionally inject `_inputs`/
            // `_outputs` schema-string metadata — the upstream wire convention
            // consumed by `Schema::infer` to recover port types from a
            // schemaless value. `realize` strips them so the codec round-trip
            // (`realize(encode(v)) ≡ v`) holds.
            (
                Self::Link { .. }
                | Self::StepLink { .. }
                | Self::ProcessLink { .. }
                | Self::CompositeLink { .. },
                Value::Map(map),
            ) => {
                let branches = self.node_data_branches();
                let mut encoded: IndexMap<Key, Value> = map
                    .iter()
                    .map(|(k, v)| {
                        let s = branches.get(k).unwrap_or(&Schema::Any);
                        (k.clone(), s.encode(v))
                    })
                    .collect();
                if let Self::Link { inputs, outputs, .. } = self {
                    encoded.insert(Key::from("_inputs"), Value::String(render_port_schema(inputs)));
                    encoded.insert(Key::from("_outputs"), Value::String(render_port_schema(outputs)));
                }
                Value::Map(encoded)
            }

            _ => value.clone(),
        }
    }

    /// Realize (decode) an encoded value into a typed representation.
    ///
    /// This is the inverse of `serialize`. Converts string-encoded
    /// numbers, parses JSON strings into structured values, etc.
    pub fn realize(&self, encoded: &Value) -> Value {
        match (self, encoded) {
            // Float: accept string encoding
            (Self::Float { .. } | Self::Delta { .. }, Value::String(s)) => {
                s.parse::<f64>().map(Value::float).unwrap_or(encoded.clone())
            }
            (Self::Float { .. } | Self::Delta { .. }, Value::Int(i)) => {
                Value::float(*i as f64)
            }
            (Self::Float { .. } | Self::Delta { .. }, _) => encoded.clone(),

            // Integer: accept string encoding
            (Self::Integer { .. }, Value::String(s)) => {
                s.parse::<i64>().map(Value::Int).unwrap_or(encoded.clone())
            }
            (Self::Integer { .. }, _) => encoded.clone(),

            // Bool: accept string encoding
            (Self::Bool { .. }, Value::String(s)) => {
                match s.to_lowercase().as_str() {
                    "true" | "1" => Value::Bool(true),
                    "false" | "0" => Value::Bool(false),
                    _ => encoded.clone(),
                }
            }
            (Self::Bool { .. }, _) => encoded.clone(),

            // String/Enum: pass through
            (Self::String { .. } | Self::Enum { .. }, _) => encoded.clone(),

            // Overwrite/Maybe: delegate
            (Self::Overwrite { inner }, _) => inner.realize(encoded),
            (Self::Maybe { .. }, Value::None) => Value::None,
            (Self::Maybe { inner }, _) => inner.realize(encoded),

            // Const: realize through inner — the immutability constraint
            // is enforced at apply, not at realize.
            (Self::Const { inner }, _) => inner.realize(encoded),

            // Quote: opaque pass-through, never walk the encoded value.
            // Mirrors upstream `realize(Quote)`.
            (Self::Quote { .. }, _) => encoded.clone(),

            // Empty bigraph types — no state to materialize.
            (Self::Site { .. }, _) => Value::None,
            (Self::InnerName { .. }, _) => Value::None,
            (Self::OuterName { .. }, _) => Value::None,
            (Self::Interface { .. }, _) => Value::None,

            // List
            (Self::List { element }, Value::List(items)) => {
                Value::List(items.iter().map(|v| element.realize(v)).collect())
            }

            // Map
            (Self::Map { value: val_schema }, Value::Map(map)) => {
                Value::Map(map.iter()
                    .map(|(k, v)| (k.clone(), val_schema.realize(v)))
                    .collect())
            }

            // Tree: realize each branch, fill defaults for missing
            (Self::Tree { branches }, Value::Map(map)) => {
                let mut result: IndexMap<Key, Value> = branches.iter()
                    .map(|(k, s)| (k.clone(), s.default_value()))
                    .collect();
                for (k, v) in map {
                    let s = branches.get(k).unwrap_or(&Schema::Any);
                    result.insert(k.clone(), s.realize(v));
                }
                Value::Map(result)
            }

            // Tuple
            (Self::Tuple { elements }, Value::List(items)) => {
                Value::List(items.iter().enumerate()
                    .map(|(i, v)| {
                        elements.get(i).unwrap_or(&Schema::Any).realize(v)
                    })
                    .collect())
            }
            // Tuple from JSON string
            (Self::Tuple { elements }, Value::String(s)) => {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
                    if let Some(arr) = parsed.as_array() {
                        return Value::List(arr.iter().enumerate()
                            .map(|(i, v)| {
                                let schema = elements.get(i).unwrap_or(&Schema::Any);
                                schema.realize(&json_to_value(v))
                            })
                            .collect());
                    }
                }
                encoded.clone()
            }

            // Map from JSON string
            (Self::Map { value: val_schema }, Value::String(s)) => {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s) {
                    if let Some(obj) = parsed.as_object() {
                        return Value::Map(obj.iter()
                            .map(|(k, v)| (Key::from(k.as_str()), val_schema.realize(&json_to_value(v))))
                            .collect());
                    }
                }
                encoded.clone()
            }

            // Array/RecursiveTree
            (Self::Array { .. }, _) => encoded.clone(),
            (Self::RecursiveTree { .. }, Value::Map(map)) => {
                Value::Map(map.iter()
                    .map(|(k, v)| (k.clone(), self.realize(v)))
                    .collect())
            }
            (Self::RecursiveTree { leaf }, _) => leaf.realize(encoded),

            // Link-kind: per-key realize over `node_data_branches` (the self-
            // exported data face) with spec keys flowing through `Any` (pass-
            // through). The `_inputs`/`_outputs` schema-string metadata
            // (injected by `encode` for upstream compat) is STRIPPED — it
            // encodes schema, not value, and is recovered by `Schema::infer`
            // from the encoded form directly. Dropping it here is what makes
            // `realize(encode(v)) ≡ v` hold.
            (
                Self::Link { .. }
                | Self::StepLink { .. }
                | Self::ProcessLink { .. }
                | Self::CompositeLink { .. },
                Value::Map(map),
            ) => {
                let branches = self.node_data_branches();
                Value::Map(
                    map.iter()
                        .filter(|(k, _)| !matches!(k.as_str(), "_inputs" | "_outputs"))
                        .map(|(k, v)| {
                            let s = branches.get(k).unwrap_or(&Schema::Any);
                            (k.clone(), s.realize(v))
                        })
                        .collect(),
                )
            }

            // Any/fallback
            (Self::Any, _) => encoded.clone(),
            _ => encoded.clone(),
        }
    }
}

/// Render port schema as a type expression string.
fn render_port_schema(ports: &IndexMap<Key, Schema>) -> String {
    ports.iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join("|")
}

/// Convert a `serde_json::Value` to our `Value` type. Foreign values
/// cannot be reconstructed from JSON alone — they require a TypeMethods
/// dispatch via the registry to be realized.
pub fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::None,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() { Value::Int(i) }
            else { Value::float(n.as_f64().unwrap_or(0.0)) }
        }
        serde_json::Value::String(s) => Value::String(s.clone()),
        serde_json::Value::Array(arr) => Value::List(arr.iter().map(json_to_value).collect()),
        serde_json::Value::Object(obj) => {
            Value::Map(obj.iter().map(|(k, v)| (Key::from(k.as_str()), json_to_value(v))).collect())
        }
    }
}

/// Convert a `Value` to `serde_json::Value`. Foreign values are encoded
/// as `null` here — opaque to plain JSON; round-tripping a Foreign
/// requires its TypeMethods dispatch (`serialize` → JSON, `realize`
/// → Foreign).
pub fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::None => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => serde_json::Number::from_f64(f.0)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::List(l) => {
            serde_json::Value::Array(l.iter().map(value_to_json).collect())
        }
        Value::Map(m) => {
            let mut obj = serde_json::Map::with_capacity(m.len());
            for (k, v) in m.iter() {
                obj.insert(k.to_string(), value_to_json(v));
            }
            serde_json::Value::Object(obj)
        }
        Value::Struct { layout, values } => {
            let mut obj = serde_json::Map::with_capacity(values.len());
            for (field, val) in layout.fields.iter().zip(values.iter()) {
                obj.insert(field.to_string(), value_to_json(val));
            }
            serde_json::Value::Object(obj)
        }
        Value::Foreign(_) => serde_json::Value::Null,
        Value::Bytes(b) => serde_json::Value::Array(
            b.iter()
                .map(|x| serde_json::Value::Number((*x as u64).into()))
                .collect(),
        ),
    }
}

impl fmt::Display for Schema {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => write!(f, "any"),
            Self::Bool { .. } => write!(f, "bool"),
            Self::Integer { .. } => write!(f, "integer"),
            Self::Float { .. } => write!(f, "float"),
            Self::String { .. } => write!(f, "string"),
            Self::Delta { .. } => write!(f, "delta"),
            Self::Const { inner } => write!(f, "const[{inner}]"),
            Self::Quote { inner } => write!(f, "quote[{inner}]"),
            Self::Site { sort } if sort.is_empty() => write!(f, "site"),
            Self::Site { sort } => write!(f, "site[{sort}]"),
            Self::InnerName { sort } if sort.is_empty() => write!(f, "inner_name"),
            Self::InnerName { sort } => write!(f, "inner_name[{sort}]"),
            Self::OuterName { sort } if sort.is_empty() => write!(f, "outer_name"),
            Self::OuterName { sort } => write!(f, "outer_name[{sort}]"),
            Self::Interface { places, names } => {
                write!(f, "interface[{};{}]", places.len(), names.len())
            }
            Self::StepLink { .. } => write!(f, "step"),
            Self::ProcessLink { interval, .. } => write!(f, "process[{interval}]"),
            Self::CompositeLink { interval, inner_schema, .. } => {
                write!(f, "composite[{interval},{inner_schema}]")
            }
            Self::Bridge { inputs, outputs } => {
                write!(f, "bridge[{}→{}]", inputs.len(), outputs.len())
            }
            Self::Overwrite { inner } => write!(f, "overwrite[{inner}]"),
            Self::List { element } => write!(f, "list[{element}]"),
            Self::Map { value } => write!(f, "map[{value}]"),
            Self::Maybe { inner } => write!(f, "maybe[{inner}]"),
            Self::Enum { values, .. } => write!(f, "enum[{}]", values.join(",")),
            Self::Tree { branches } => {
                write!(f, "tree{{")?;
                for (i, (k, v)) in branches.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, "}}")
            }
            Self::Array { shape, element } => {
                let dims: Vec<String> = shape.iter().map(|d| d.to_string()).collect();
                write!(f, "array[{}|{}]", dims.join("|"), element)
            }
            Self::Tuple { elements } => {
                write!(f, "tuple[")?;
                for (i, e) in elements.iter().enumerate() {
                    if i > 0 { write!(f, ",")?; }
                    write!(f, "{e}")?;
                }
                write!(f, "]")
            }
            Self::RecursiveTree { leaf } => write!(f, "tree[{leaf}]"),
            Self::Link { inputs, outputs, temporal } => {
                let prefix = match temporal {
                    Some(true) => "process",
                    Some(false) => "step",
                    None => "link",
                };
                write!(f, "{prefix}[")?;
                for (i, (k, v)) in inputs.iter().enumerate() {
                    if i > 0 { write!(f, "|")?; }
                    write!(f, "{k}:{v}")?;
                }
                write!(f, ",")?;
                for (i, (k, v)) in outputs.iter().enumerate() {
                    if i > 0 { write!(f, "|")?; }
                    write!(f, "{k}:{v}")?;
                }
                write!(f, "]")
            }
            Self::Custom { name, parameters } => {
                if parameters.is_empty() {
                    write!(f, "{name}")
                } else {
                    write!(f, "{name}[")?;
                    for (i, (k, v)) in parameters.iter().enumerate() {
                        if i > 0 { write!(f, ",")?; }
                        write!(f, "{k}:{v}")?;
                    }
                    write!(f, "]")
                }
            }
        }
    }
}

/// Parse a simple schema string into a Schema.
/// Supports: "float", "integer", "bool", "string", "delta", "any"
/// For complex schemas, use the Schema constructors directly.
/// Process `_remove` and `_add` special keys in a map update.
///
/// - `_remove`: a list of keys to delete from the map.
/// - `_add`: a map of new entries to insert (absolute values, not deltas).
///
/// These are core process-bigraph operations for structural state changes
/// like particle division, boundary spawning, and process composition.
/// Process a `_divide` sentinel from a `Map` update — the faithful port of
/// bigraph-schema's `methods/apply.py::_handle_divide_sentinel`.
///
/// Shape: `{ _divide: { mother: <key>, daughters: { <k1>: <override>, … } } }`
/// (or `daughters: [<k1>, <k2>]` for pure type-driven splits). Two-phase:
/// (1) `divide_by_schema(value_schema, mother)` produces baseline daughters
/// (extensive fields split, intensive copied, sub-process specs shared so
/// they re-realize); (2) each caller override is deep-merged on top (ids,
/// fresh declarations). The mother key is removed and the daughters installed.
fn apply_divide_sentinel(
    value_schema: &Schema,
    registry: &crate::registry::TypeRegistry,
    current: &Value,
    update_map: &crate::value::StateMap,
) -> Value {
    use crate::registry::{divide_by_schema, DivideContext};

    let Some(current_map) = current.as_map() else {
        return current.clone();
    };
    let Some(spec) = update_map.get("_divide").and_then(|v| v.as_map()) else {
        return current.clone();
    };
    let Some(mother) = spec.get("mother").and_then(|v| v.as_str()) else {
        return current.clone();
    };
    // Normalize daughters into (key, optional override) pairs.
    let daughter_items: Vec<(crate::value::Key, Option<Value>)> = match spec.get("daughters") {
        Some(Value::Map(d)) => d.iter().map(|(k, v)| (k.clone(), Some(v.clone()))).collect(),
        Some(Value::List(l)) => l
            .iter()
            .filter_map(|v| v.as_str().map(|s| (crate::value::Key::from(s), None)))
            .collect(),
        _ => return current.clone(),
    };
    if daughter_items.is_empty() {
        return current.clone();
    }
    let Some(mother_state) = current_map.get(mother) else {
        return current.clone();
    };

    let mut ctx = DivideContext::binary();
    ctx.n_daughters = daughter_items.len();
    let baselines = divide_by_schema(value_schema, mother_state, &ctx, registry);

    let mut result = current_map.clone();
    result.shift_remove(mother);
    for (i, (key, override_val)) in daughter_items.into_iter().enumerate() {
        let baseline = baselines.get(i).cloned().unwrap_or(Value::None);
        let daughter = match override_val {
            Some(ov) => merge_replace(&baseline, &ov),
            None => baseline,
        };
        result.insert(key, daughter);
    }
    Value::Map(result)
}

/// Deep-merge `over` onto `base`, with `over` winning (replace semantics, not
/// the additive merge of `apply_update`). Used for daughter overrides.
fn merge_replace(base: &Value, over: &Value) -> Value {
    match (base, over) {
        (Value::Map(b), Value::Map(o)) => {
            let mut m = b.clone();
            for (k, v) in o {
                let merged = match m.get(k) {
                    Some(existing) => merge_replace(existing, v),
                    None => v.clone(),
                };
                m.insert(k.clone(), merged);
            }
            Value::Map(m)
        }
        _ => over.clone(),
    }
}

/// Apply the `_remove`/`_add` structural sentinels of a map update in place.
/// **Module-private to the algebra**: only `apply` calls this; consumers go
/// through `algebra::apply` (so inline `_add`/`_remove` munging can't reappear
/// at a call site — the closure invariant).
pub(crate) fn apply_add_remove(result: &mut crate::value::StateMap, update: &crate::value::StateMap) {
    // _remove: delete listed keys
    if let Some(Value::List(keys)) = update.get("_remove") {
        for key in keys {
            if let Some(k) = key.as_str() {
                result.swap_remove(k);
            }
        }
    }

    // _add: insert new entries (absolute values, not deltas)
    if let Some(Value::Map(adds)) = update.get("_add") {
        for (k, v) in adds {
            result.insert(k.clone(), v.clone());
        }
    }
}

pub fn parse_schema(s: &str) -> Schema {
    match s.trim() {
        "any" => Schema::Any,
        "bool" | "boolean" => Schema::bool(),
        "int" | "integer" => Schema::integer(),
        "float" => Schema::float(),
        "string" => Schema::string(),
        "delta" => Schema::delta(),
        "set_float" => Schema::set_float(),
        _ => Schema::Any,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_value() {
        let schema = Schema::tree([
            ("mass", Schema::float_default(1.0)),
            ("alive", Schema::bool()),
        ]);

        let val = schema.default_value();
        let map = val.as_map().unwrap();
        assert_eq!(map["mass"].as_f64(), Some(1.0));
        assert_eq!(map["alive"].as_bool(), Some(false));
    }

    #[test]
    fn test_delta_apply() {
        let schema = Schema::delta();
        let current = Value::float(10.0);
        let update = Value::float(3.0);
        let result = schema.apply_update(&current, &update);
        assert_eq!(result.as_f64(), Some(13.0));
    }

    #[test]
    fn test_check() {
        let schema = Schema::tree([
            ("x", Schema::float()),
            ("name", Schema::string()),
        ]);
        let good = Value::tree([
            ("x", Value::float(1.0)),
            ("name", Value::from("test")),
        ]);
        assert!(schema.check(&good));

        let bad = Value::tree([
            ("x", Value::from("not a float")),
            ("name", Value::from("test")),
        ]);
        assert!(!schema.check(&bad));
    }

    #[test]
    fn const_apply_preserves_current() {
        let schema = Schema::const_of(Schema::float());
        let current = Value::float(5.0);
        let update = Value::float(99.0);
        let result = schema.apply_update(&current, &update);
        assert_eq!(result.as_f64(), Some(5.0), "const value must not change");
    }

    #[test]
    fn quote_apply_replaces() {
        let schema = Schema::quote_of(Schema::float());
        let result = schema.apply_update(&Value::float(1.0), &Value::String("opaque".into()));
        assert_eq!(result, Value::String("opaque".into()));
    }

    #[test]
    fn empty_bigraph_types_carry_no_state() {
        for s in [
            Schema::site(),
            Schema::site_sorted("organelle"),
            Schema::inner_name(""),
            Schema::outer_name(""),
            Schema::interface(),
        ] {
            assert!(matches!(s.default_value(), Value::None));
            // Apply ignores updates — current preserved.
            let r = s.apply_update(&Value::None, &Value::float(99.0));
            assert!(matches!(r, Value::None));
        }
    }

    #[test]
    fn typed_link_variants_classify_correctly() {
        let step = Schema::step_link(IndexMap::new(), IndexMap::new());
        let process =
            Schema::process_link_with_interval(IndexMap::new(), IndexMap::new(), 0.5);
        let composite = Schema::composite_link(
            IndexMap::new(),
            IndexMap::new(),
            Schema::tree([("mass", Schema::float())]),
        );

        assert!(step.is_link_kind());
        assert!(process.is_link_kind());
        assert!(composite.is_link_kind());
        assert!(!Schema::float().is_link_kind());
    }

    #[test]
    fn link_ports_uniform_access() {
        let inputs = IndexMap::from_iter([("mass".into(), Schema::float())]);
        let outputs = IndexMap::from_iter([("mass".into(), Schema::float())]);
        let process = Schema::process_link(inputs.clone(), outputs.clone());
        let (i, o) = process.link_ports().unwrap();
        assert_eq!(i.len(), 1);
        assert_eq!(o.len(), 1);
        assert_eq!(Schema::float().link_ports(), None);
    }
}
