//! Surface for protocols-as-types (docs/protocols-as-types.md, step 3): a
//! `protocol Name = stream<Cell, path: '…'>` alias, used as `Name[config]`, lowers
//! the wrapped composite to a node whose `address` is the TYPED protocol address
//! value `{_type: stream, path: '…'}` — the eval half of the env.ys surface.

use chrysalis::compile::compile;
use chrysalis::parse::parse_program;

const SRC: &str = "\
composite Cell[mass :: Float = 1.0]
  ~{mass :: Float @ mass}
  ->{mass :: Float @ mass}
(
  mass: mass
)

protocol StreamingCell = stream<Cell, path: 'cell.ys'>

StreamingCell[mass: 2.0]
";

#[test]
fn protocol_bound_control_lowers_to_a_typed_stream_address() {
    let prog = parse_program(SRC).expect("parse");
    let result = compile(&prog).expect("compile");
    // The trailing `StreamingCell[mass: 2.0]` is the root value.
    let node = result.initial_state;
    let m = node.as_map().expect("the cell node is a map");

    // The address is the TYPED protocol address value, not `local:Composite`.
    let addr = m
        .get("address")
        .and_then(|v| v.as_map())
        .expect("address is a typed (map) value");
    assert_eq!(
        addr.get("_type").and_then(|v| v.as_str()),
        Some("stream"),
        "the `_type` tag is the protocol"
    );
    assert_eq!(
        addr.get("path").and_then(|v| v.as_str()),
        Some("cell.ys"),
        "the protocol's typed `path` field"
    );

    // The wrapped Cell's config/bridge/wiring rode along unchanged — only the
    // address changed.
    assert!(m.get("config").is_some(), "wrapped Cell config present");
    assert!(m.get("inputs").is_some(), "wrapped Cell input wiring present");
    assert!(m.get("outputs").is_some(), "wrapped Cell output wiring present");
}

#[test]
fn protocol_def_round_trips_through_unparse() {
    let prog = parse_program(SRC).expect("parse");
    let text = chrysalis::unparse::unparse(&prog);
    assert!(
        text.contains("protocol StreamingCell = stream<Cell, path: 'cell.ys'>"),
        "protocol def unparses to its surface form; got:\n{text}"
    );
}
