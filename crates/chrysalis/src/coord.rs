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
    let src = std::fs::read_to_string(&path).map_err(|e| format!("read {path}: {e}"))?;
    let (rendered, bumped) = set_in_source(&src, peer, assignments)?;
    std::fs::write(&path, &rendered).map_err(|e| format!("write {path}: {e}"))?;
    println!(
        "coord set: {path} updated ({} field(s){})",
        assignments.len(),
        if bumped { ", tick bumped" } else { "" }
    );
    Ok(())
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
    use crate::ast::{Def, Expr, StringLit};

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

    // Apply each field=value. A bare integer becomes an `Int` (so `tick=16` stays
    // numeric); everything else is a `String` literal — the unparser escapes it, so
    // the author never hand-writes `.ys` syntax (the whole point).
    let mut set_tick = false;
    for (field, raw) in assignments {
        if field == "tick" {
            set_tick = true;
        }
        let expr = match raw.parse::<i64>() {
            Ok(n) => Expr::Int(n),
            Err(_) => Expr::Str(StringLit::plain(raw.as_str())),
        };
        record.insert(field.clone(), expr);
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
