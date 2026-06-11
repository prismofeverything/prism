//! `chrysalis coord` — multi-agent COORDINATION on the mesh (the #62 dogfood: the
//! mesh coordinating the very agents building the mesh).
//!
//! A live board hosted as ONE `map[any]` **mesh** link (the per-source CRDT,
//! `mesh_safety`-blessed): peers `push` their own key and `pull` the converged
//! board over a socket — communication ACROSS THE LINK, no shared file to collide
//! on (the file form is `coord/<peer>.ys`; this is the live form). Pure assembly of
//! [`RestProcessServer`] + [`MeshReplica`] — no new mechanism.
//!
//! ```text
//! chrysalis coord serve [--port 8799]      # the daemon hosts the board (one terminal)
//! chrysalis coord push <peer> '<json>'     # merge your key (per-source, atomic)
//! chrysalis coord pull                     # print the converged board
//! ```

use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use prism_bigraph::factory::ProcessRegistry;
use prism_bigraph::process::{Process, ProcessNode};
use prism_bigraph::protocols::mesh::{CONTRIBUTION_PORT, LINK_PORT};
use prism_bigraph::protocols::{MeshReplica, RestProcessServer, RestProtocol};
use prism_bigraph::{Core, Protocol};
use prism_schema::schema::{json_to_value, value_to_json};
use prism_schema::{Schema, TypeRegistry, Value};

pub const DEFAULT_PORT: u16 = 8799;
const BOARD: &str = "Board";

/// The board is a per-source pool: `map[any]` keyed by peer (mesh-safe — the merge
/// is a key-union, idempotent + commutative, so concurrent pushes never collide).
fn board_schema() -> Schema {
    Schema::map(Schema::Any)
}

/// Run the coordination daemon: host the board as one shared `MeshReplica` at
/// `127.0.0.1:port`. Every connection shares the SAME board slot, so it persists
/// across pushes/pulls. Parks until killed.
pub fn serve(port: u16) -> std::io::Result<()> {
    let slot = Arc::new(Mutex::new(Value::Map(IndexMap::new())));
    let types = Arc::new(TypeRegistry::new());
    let mut processes = ProcessRegistry::new();
    processes.register(BOARD, move |_| {
        ProcessNode::Process(Box::new(
            MeshReplica::shared(board_schema(), Arc::clone(&slot), Arc::clone(&types))
                .expect("board schema is mesh-safe"),
        ))
    });
    let server = RestProcessServer::start_on(Core::from(Arc::new(processes)), ("127.0.0.1", port))?;
    println!(
        "chrysalis coord: the board is LIVE as a map[any] mesh link on {}\n  \
         push:  chrysalis coord push <peer> '<json>'   pull:  chrysalis coord pull\n  \
         (Ctrl-C to stop)",
        server.base_url()
    );
    loop {
        std::thread::park();
    }
}

/// PUSH a peer's value (a JSON object) to the board — merges `{<peer>: <value>}`
/// via the CRDT key-union (per-source: your key only). Prints the converged board.
pub fn push(port: u16, peer: &str, json: &str) -> Result<(), String> {
    let parsed: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("bad JSON for `{peer}`: {e}"))?;
    let contribution = Value::tree([(
        CONTRIBUTION_PORT,
        Value::tree([(peer, json_to_value(&parsed))]),
    )]);
    print_board(client(port)?.update(&contribution, 0.0).into_value());
    Ok(())
}

/// PULL the converged board — merge nothing, read the current link. Prints it.
pub fn pull(port: u16) -> Result<(), String> {
    let empty = Value::Map(IndexMap::new());
    print_board(client(port)?.update(&empty, 0.0).into_value());
    Ok(())
}

/// SET fields on a peer's heartbeat file (`coord/<peer>.ys`) FROM DATA — the
/// durable-file analogue of [`push`]. Read → parse → set the named fields on the
/// `def <peer> = {…}` record → unparse through the canonical serializer →
/// **round-trip gate** → write. Because each value is DATA the serializer escapes
/// (never hand-written `.ys` source), a `{` (interpolation) or a lone `'` can NEVER
/// reach the board; and the gate guarantees the file only ever transitions
/// good → good — if the re-serialized text does not re-parse, the write is ABORTED
/// and the file left untouched. `tick` auto-increments unless set explicitly.
///
/// ```text
/// chrysalis coord set simplify status='all green' note='shipped the guard'
/// ```
pub fn set(peer: &str, assignments: &[(String, String)]) -> Result<(), String> {
    let path = format!("coord/{peer}.ys");
    // CREATE-ON-BOOT: a missing heartbeat is synthesized from the canonical skeleton
    // (DATA → gated by `set_in_source` below), so an agent's FIRST `coord set` makes it
    // live on the board — "boot into the heartbeat" is one command, never a hand-made file.
    let (src, created) = match std::fs::read_to_string(&path) {
        Ok(s) => (s, false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (skeleton(peer), true),
        Err(e) => return Err(format!("read {path}: {e}")),
    };
    let (rendered, bumped) = set_in_source(&src, peer, assignments)?;
    std::fs::write(&path, &rendered).map_err(|e| format!("write {path}: {e}"))?;
    // Wire the peer into the aggregated board (idempotent — a no-op if already joined).
    let joined = join_board(peer)?;
    println!(
        "coord set: {path} {} ({} field(s){}{})",
        if created {
            "CREATED — you are live on the board"
        } else {
            "updated"
        },
        assignments.len(),
        if bumped { ", tick bumped" } else { "" },
        if joined { ", joined coord/board.ys" } else { "" },
    );
    Ok(())
}

/// The canonical skeleton heartbeat for a freshly-booting peer — a `def <peer> = { … }`
/// record of the standard fields at their defaults, plus a `#` banner. Passed through
/// `set_in_source` (which gates it), so a malformed skeleton would abort, not corrupt.
fn skeleton(peer: &str) -> String {
    format!(
        "# coord/{peer}.ys — the `{peer}` peer's LIVE heartbeat (created by `chrysalis coord set`;\n\
         # rebuilt from coord/{peer}.next on boot). Edit ONLY via `chrysalis coord set {peer} …`,\n\
         # NEVER by hand: the serializer escapes data, so a stray brace/quote cannot wedge the\n\
         # board. coord/board.ys MERGES every peer through the mesh link.\n\
         def {peer} = {{ task: 'booting — see coord/{peer}.next', touching: [], status: '', note: '', build: {{ state: 'idle', layer: '', green_tick: 0, cmd: '', note: '' }}, tick: 0 }}\n"
    )
}

/// Ensure `coord/board.ys` imports + merges `peer` (idempotent — a no-op if already
/// present, so every `set` self-heals the board). The board aggregates every peer
/// through its `mesh` link, so a freshly created heartbeat must be wired in to appear
/// in the rendered board. The edit is a programmatic text splice GUARDED by the parse
/// gate (re-parse or ABORT — the board only ever goes good → good), so it is board-safe
/// even though it splices text. No board file (the live-socket-only form) ⇒ a no-op.
fn join_board(peer: &str) -> Result<bool, String> {
    let path = "coord/board.ys";
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("read {path}: {e}")),
    };
    match join_board_in_source(&src, peer)? {
        Some(rendered) => {
            std::fs::write(path, &rendered).map_err(|e| format!("write {path}: {e}"))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// The pure core of [`join_board`] — no file I/O, so it is unit-testable. Returns
/// `Some(new_source)` if `peer` was wired in (the import + the mesh-link merge entry),
/// or `None` if it was already present (idempotent). The **parse gate** lives here: a
/// returned `Some` is GUARANTEED to re-parse, so the caller can never write a broken
/// board. The splice is textual but gate-protected — board-safe (good → good).
pub fn join_board_in_source(src: &str, peer: &str) -> Result<Option<String>, String> {
    let import = format!("from .{peer} import {peer}");
    let merge_entry = format!("{peer}: {peer}");
    let has_import = src.lines().any(|l| l.trim() == import);
    let has_merge = src.contains(&merge_entry);
    if has_import && has_merge {
        return Ok(None);
    }

    let trailing_nl = src.ends_with('\n');
    let mut lines: Vec<String> = src.lines().map(String::from).collect();
    if !has_import {
        let pos = lines
            .iter()
            .rposition(|l| l.trim_start().starts_with("from ."))
            .map(|i| i + 1)
            .unwrap_or(0);
        lines.insert(pos, import);
    }
    let mut rendered = lines.join("\n");
    if trailing_nl {
        rendered.push('\n');
    }
    if !has_merge {
        // Splice `<peer>: <peer>,` just inside the mesh link Record's opening brace.
        let anchor = "mesh = {";
        let at = rendered
            .find(anchor)
            .ok_or_else(|| format!("could not find the `{anchor}` link Record to join"))?
            + anchor.len();
        rendered.insert_str(at, &format!(" {merge_entry},"));
    }
    // GATE — the board only ever goes good → good; a bad splice aborts (nothing written).
    crate::parse::parse_program(&rendered).map_err(|e| {
        format!("join_board ABORTED (nothing written): board.ys would not re-parse — {e}")
    })?;
    Ok(Some(rendered))
}

/// Parse a `coord set` value string into an `Expr` DATA literal. A JSON array/object
/// (`[…]` / `{…}`) becomes structured data (lists, nested records); a bare integer
/// becomes `Int`; anything else is a `String` literal (the unparser escapes it). If a
/// `[`/`{` value is NOT valid JSON it falls back to a string (so a `status='{busy}'`
/// stays a safe escaped string, never an error). The round-trip gate in
/// [`set_in_source`] guarantees the result is safely encodable regardless.
fn value_to_expr(raw: &str) -> Result<crate::ast::Expr, String> {
    use crate::ast::{Expr, StringLit};
    let trimmed = raw.trim_start();
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
            if json.is_array() || json.is_object() {
                return data_value_to_expr(&json_to_value(&json))
                    .map_err(|e| format!("value `{raw}` is not representable as `.ys` data: {e}"));
            }
        }
        // Looked structured but is not valid JSON array/object — treat as a plain
        // string (the unparser escapes the braces, so the board stays safe).
    }
    Ok(match raw.parse::<i64>() {
        Ok(n) => Expr::Int(n),
        Err(_) => Expr::Str(StringLit::plain(raw)),
    })
}

/// Lift a DATA [`Value`] into an `Expr` LITERAL (structural). This is NOT
/// `Expr::from_value` — that reifies a *quoted AST* (a Map-encoded `Expr`), whereas
/// this turns ordinary data (a JSON list / record) into the literal that denotes it.
/// JSON yields only None/Bool/Int/Float/String/List/Map, so those are total here.
fn data_value_to_expr(v: &Value) -> Result<crate::ast::Expr, String> {
    use crate::ast::{Expr, StringLit};
    Ok(match v {
        Value::Bool(b) => Expr::Bool(*b),
        Value::Int(n) => Expr::Int(*n),
        Value::Float(f) => Expr::Float(f.0),
        Value::String(s) => Expr::Str(StringLit::plain(s)),
        Value::List(items) => Expr::List(
            items
                .iter()
                .map(data_value_to_expr)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Map(entries) => Expr::Map(
            entries
                .iter()
                .map(|(k, val)| Ok((StringLit::plain(k.as_str()), data_value_to_expr(val)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
        other => return Err(format!("cannot represent {other:?} as `.ys` data")),
    })
}

/// Set `value` at a DOTTED PATH in a record, creating intermediate Records as needed.
/// `["build","state"]` descends into `build`'s Record (making it if absent / not a
/// record) and sets `state`. A single-segment path is a plain top-level insert. Shared
/// by the board heartbeat edit ([`set_in_source`]) and the manifest edit
/// ([`crate::manifest::add_dependency`]) — the one correct-by-construction `.ys`-record
/// setter (the homoiconic round-trip: parse → set here → unparse → re-parse gate).
pub(crate) fn set_path(
    record: &mut IndexMap<String, crate::ast::Expr>,
    path: &[&str],
    value: crate::ast::Expr,
) {
    use crate::ast::Expr;
    let (head, rest) = path.split_first().expect("coord set: empty field path");
    if rest.is_empty() {
        record.insert((*head).to_string(), value);
        return;
    }
    let child = record
        .entry((*head).to_string())
        .or_insert_with(|| Expr::Record(IndexMap::new()));
    if !matches!(child, Expr::Record(_)) {
        *child = Expr::Record(IndexMap::new());
    }
    if let Expr::Record(inner) = child {
        set_path(inner, rest, value);
    }
}

/// The pure core of [`set`] — no file I/O, so it is unit-testable. Returns the new
/// source text + whether `tick` was auto-bumped, or an error (nothing written). The
/// **round-trip gate** lives here: a returned `Ok` is GUARANTEED to re-parse, so a
/// caller can never write a broken heartbeat to the board.
pub fn set_in_source(
    src: &str,
    peer: &str,
    assignments: &[(String, String)],
) -> Result<(String, bool), String> {
    use crate::ast::{Def, Expr};

    let mut program =
        crate::parse::parse_program(src).map_err(|e| format!("parse heartbeat: {e}"))?;

    // The heartbeat is `def <peer> = { field: value, … }` — a Binding whose value
    // is a Record. Grab its field map (mutably).
    let record = program
        .defs
        .iter_mut()
        .find_map(|d| match d {
            Def::Binding { name, value: Expr::Record(fields), .. } if name.as_str() == peer => {
                Some(fields)
            }
            _ => None,
        })
        .ok_or_else(|| format!("no `def {peer} = {{ … }}` record binding in the heartbeat"))?;

    // Apply each field=value FROM DATA. `field` may be a DOTTED PATH (`build.state`)
    // — descend / create nested Records. `value` parses as a JSON array/object
    // (`touching=["a","b"]`) → structured data; else a bare integer (`tick=16`) → Int;
    // else a String literal — the unparser escapes every value, so the author never
    // hand-writes `.ys` syntax and a `{` / `'` can never reach the board (the point).
    let mut set_tick = false;
    for (field, raw) in assignments {
        let segments: Vec<&str> = field.split('.').collect();
        if segments.as_slice() == ["tick"] {
            set_tick = true;
        }
        set_path(record, &segments, value_to_expr(raw)?);
    }

    // Auto-bump the monotone `tick` counter unless it was set explicitly.
    let mut bumped = false;
    if !set_tick {
        if let Some(Expr::Int(t)) = record.get("tick") {
            let next = *t + 1;
            record.insert("tick".to_string(), Expr::Int(next));
            bumped = true;
        }
    }

    // Serialize through the canonical unparser, preserving a leading comment/banner
    // block verbatim — the AST does not carry comments (comment-preserving unparse is
    // #10), so re-prepend the heartbeat's `#` header (lines before the first code line)
    // rather than silently dropping it.
    let header: String = src
        .lines()
        .take_while(|l| {
            let t = l.trim_start();
            t.is_empty() || t.starts_with('#')
        })
        .map(|l| format!("{l}\n"))
        .collect();
    let rendered = format!("{header}{}", crate::unparse::unparse(&program));

    // ROUND-TRIP GATE — the board only ever goes good → good. If the re-serialized
    // text does not re-parse, ABORT (the value carried something the serializer
    // could not safely encode, e.g. a `{` that would interpolate). The literal
    // `{{` below is an escaped brace in the message, not interpolation.
    crate::parse::parse_program(&rendered).map_err(|e| {
        format!(
            "coord set ABORTED (nothing written): the update would not re-parse — {e}. \
             A value likely carried a `{{` (interpolation) or other unencodable text — rephrase it."
        )
    })?;

    Ok((rendered, bumped))
}

/// A rest client onto the daemon's shared board replica.
fn client(port: u16) -> Result<Box<dyn Process>, String> {
    let types = Arc::new(TypeRegistry::new());
    let mut processes = ProcessRegistry::new();
    processes.register(BOARD, move |_| {
        ProcessNode::Process(Box::new(
            MeshReplica::shared(
                board_schema(),
                Arc::new(Mutex::new(Value::Map(IndexMap::new()))),
                Arc::clone(&types),
            )
            .expect("mesh-safe"),
        ))
    });
    let core = Core::from(Arc::new(processes));
    let addr = Value::Map(IndexMap::from_iter([
        ("process".into(), Value::String(BOARD.into())),
        ("host".into(), Value::String("127.0.0.1".into())),
        ("port".into(), Value::Int(port as i64)),
    ]));
    match RestProtocol
        .instantiate(&addr, Value::None, &core)
        .map_err(|e| format!("connect to coord daemon on :{port} failed: {e} (is it serving?)"))?
    {
        ProcessNode::Process(p) => Ok(p),
        _ => Err("expected a Process".into()),
    }
}

fn print_board(update: Option<Value>) {
    let board = update
        .and_then(|v| v.get_field(LINK_PORT).cloned())
        .unwrap_or(Value::Map(IndexMap::new()));
    println!(
        "{}",
        serde_json::to_string_pretty(&value_to_json(&board)).unwrap_or_else(|_| "{}".into())
    );
}
