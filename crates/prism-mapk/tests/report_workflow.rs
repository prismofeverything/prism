//! The MAPK report produced by a **step-network workflow composite** (#6):
//! `RunBrs → {PlotTrajectories, RenderRules, RenderAnimation} → WriteReport`,
//! wired by shared state paths and fired in dependency order by the engine.
//! Proves the report (trajectory, reaction diagrams, animation, collated HTML)
//! falls out of the composite — no hand-ordered pipeline.

use prism_mapk::workflow::{run_report_workflow, ReportConfig};

#[test]
fn mapk_report_is_produced_by_a_step_network() {
    let dir = std::env::temp_dir().join(format!("prism_mapk_wf_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cfg = ReportConfig {
        duration: 5.0, // short — just enough snapshots to render
        interval: 1.0,
        seed: 42,
        out_dir: dir.clone(),
        name: "wf_test".into(),
    };

    let report = run_report_workflow(&cfg).expect("workflow runs to completion");

    // The join step wrote the HTML…
    assert!(report.exists(), "report HTML at {report:?}");
    // …and each independent analysis branch wrote its asset.
    assert!(dir.join("wf_test_trajectories.svg").exists(), "trajectory svg");
    assert!(dir.join("wf_test_animation.svg").exists(), "animation svg");
    // RenderRules emitted a redex/reactum pair per rule (phosphorylate is one).
    assert!(
        dir.join("wf_test_rule_phosphorylate_redex.svg").exists(),
        "a rule diagram svg"
    );

    // The collated report links every panel the DAG produced.
    let html = std::fs::read_to_string(&report).unwrap();
    for needle in [
        "MAPK report",
        "Populations over time",
        "wf_test_trajectories.svg",
        "Reaction rules",
        "Cell animation",
        "wf_test_animation.svg",
    ] {
        assert!(html.contains(needle), "report HTML missing {needle:?}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}
