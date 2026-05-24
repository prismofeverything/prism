use std::sync::Arc;

fn main() {
    let fixture = include_str!("../../fixtures/spatioflux_reference_demo.json");
    let registry = Arc::new(spatio_flux::build_registry());
    let (mut engine, _) = spatio_flux::vivarium_loader::load_vivarium(fixture, registry).unwrap();

    // Before running, check state structure
    let state = engine.state();
    eprintln!("State type: {}", state.type_name());
    eprintln!("State is_map_like: {}", state.is_map_like());
    eprintln!(
        "Fields via get_field: {:?}",
        state.get_field("fields").map(|v| v.type_name())
    );
    eprintln!("Fields via as_map: {:?}", state.as_map().map(|m| m.len()));

    // Check glucose specifically
    let glc = state
        .get_field("fields")
        .and_then(|f| f.get_field("glucose"));
    eprintln!("glucose field: {:?}", glc.map(|v| v.type_name()));

    let glc_val = state.get_path(&["fields".into(), "glucose".into(), "5".into(), "5".into()]);
    eprintln!("glucose[5][5] via get_path: {:?}", glc_val);

    // Check particles
    let particles = state.get_field("particles");
    eprintln!("particles: {:?}", particles.map(|v| v.type_name()));

    // Run 1 step
    engine.run(1.0);

    let state = engine.state();
    let glc_after = state.get_path(&["fields".into(), "glucose".into(), "5".into(), "5".into()]);
    eprintln!("After 1s: glucose[5][5] = {:?}", glc_after);

    let mass = state
        .get_field("particles")
        .and_then(|p| p.iter_fields())
        .and_then(|mut iter| iter.next())
        .and_then(|(_, p)| p.get_field("mass"))
        .and_then(|v| v.as_f64());
    eprintln!("After 1s: first particle mass = {:?}", mass);
}
