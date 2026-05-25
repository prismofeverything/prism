# Protocols as types — addresses are first-class typed values

The design for making a process's **address** a first-class schema value, restoring
the typed-protocol model upstream `../process-bigraph` had (each protocol a
registered type with its own fields) that prism flattened to an untyped
`{protocol, data: Value}`.

## The split: address (data) vs transport (runtime)

"Protocol" conflates two things:

- **The address** — *data* describing where/how a process runs: `Rest{host, port,
  process}`, `Stream{path}`, `Local{process}`. A **value with a type**. It is wired,
  checked, serialized, defaulted, divided — through the *closed algebra*, like every
  other value.
- **The transport** — the *behavior* that realizes an address + config into a **live
  process** (opens a socket, spawns a child, submits to a pool). The result is a
  **runtime instance**, never a `Value` — the same reason a running engine can't sit
  in state (cells-and-division.md: "the running instance is a runtime concern").

So **the address is "just a type people wire to"; the live transport is its runtime
realization** — the identical data-vs-instance split prism already makes one level
up: a `ProcessLink` is *data* (address + config); the live `Process` is the instance
the engine builds from it. A protocol address is that pattern one level down.

This is why a Rust `Protocol::data_schema()` method is the wrong home: it *describes*
a type in Rust instead of the type *being* in the schema. The fix: **register each
protocol's address as a `Custom` type** (exactly how `graph`/`CRN`/`Path` are rich
types — representation in the schema, behavior in Rust). After registration the
algebra handles addresses for free.

## Representation — `_type`-tagged Custom values

A Custom value is a `Value::Map` carrying `_type`; `Schema::infer` recovers
`Custom(name)` from it. So an address is:

```text
Local    {_type:"local",    process:"Cell"}
Parallel {_type:"parallel", process:"Double"}
Rest     {_type:"rest",     process:"Composite", host:"127.0.0.1", port:"8080"}
Stream   {_type:"stream",   path:"cell.ys"}            # local spawn (today)
                                                       # {host,port,path} for remote (later)
```

Each protocol registers its representation type in the `TypeRegistry`
(`Protocol::address_type() -> (name, Schema)`, registered into the `Core`):

| protocol | representation |
|---|---|
| `local` | `{ process: String }` |
| `parallel` | `{ process: String }` |
| `rest` | `{ process: String, host: String, port: String }` |
| `stream` | `{ path: String }` |

Matches upstream exactly (`RayProtocol.data: String`, `RestProtocol.data:
RestData{process,host,port}`) — single-field protocols are a one-field record.

**No sum type needed.** prism has no tagged-union sort, but it doesn't need one: a
node's `address` field is typed *per instance* by its value's `_type` (infer →
`Custom("rest")`), and the `TypeRegistry` drives `check`/`serialize`/`realize` from
the registered representation. (The legacy `"proto:data"` string and `{protocol,
data}` map forms stay accepted as sugar — `ParsedAddress::parse` normalizes all
three; no forced migration.)

## Transport — keyed by the type name

The Rust `Protocol` trait keeps `instantiate(address, config, registry) ->
ProcessNode` (the IO that can't be pure schema — sockets/pipes/threads), now reading
its **typed fields** off the address value. The protocol's `name()` == its address
type name, so the `_type` tag selects both the schema type (for the algebra) and the
transport (for instantiation).

## The surface

Constructing an address is constructing a typed value — a protocol control with its
fields: `Rest{host:"…", port:"…", process:Cell}`, `Stream{path:"cell.ys"}`. A
composite/process node is then placed *at* an address. Exact `.ys` syntax for "run
this composite over this protocol" is the surface step (the env.ys goal); the typed
address value is what it lowers to.

## Plan (incremental, test-guarded)

1. ✅ **Register protocol address types** as `Custom` types in the `Core`'s
   `TypeRegistry` (`Protocol::address_type()` → `(name, repr)`; `Core` registers each
   on `with_protocols`/`with_types`). Addresses are intensive (`String` fields), so a
   divide *shares* them (daughters inherit) for free. Proven:
   `prism-bigraph/tests/protocol_types.rs::core_registers_protocol_address_types`.
2. ✅ **`ParsedAddress::parse` normalizes the `_type` form.** A single-field record
   unwraps to its value, so `{_type:"local",process:"Cell"}` normalizes to the SAME
   `data` (a `String`) the legacy `"local:Cell"`/`{protocol,data}` forms produced —
   `instantiate` is unchanged; `rest`'s `{process,host,port}` stays a record. Proven:
   `typed_address_parses_to_legacy_data` + `engine_discovers_and_runs_a_typed_address_node`.
3. ⏳ **Surface**: a protocol control lowering to a typed address; `env.ys` places cells
   over `stream:`/`parallel:`; the whole environment runs via `chrysalis run env.ys`.
   (Needs the `map[Cell]→Map{CompositeLink}` threading + the `%` self-node surface wire
   too — task #20.)
