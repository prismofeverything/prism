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
