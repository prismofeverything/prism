//! Generate the MAPK HTML report by running the **step-network workflow
//! composite** (`prism_mapk::workflow`) — the report as a DAG of steps the
//! engine fires in dependency order, not a hand-ordered `main`.
//!
//! Usage: `report_workflow [--time 80] [--interval 1.0] [--seed 42]
//!         [--out out] [--name brs_mapk_workflow]`

use std::path::PathBuf;

use prism_mapk::workflow::{run_report_workflow, ReportConfig};

fn main() {
    let mut cfg = ReportConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--time" => cfg.duration = args.next().and_then(|s| s.parse().ok()).unwrap_or(cfg.duration),
            "--interval" => cfg.interval = args.next().and_then(|s| s.parse().ok()).unwrap_or(cfg.interval),
            "--seed" => cfg.seed = args.next().and_then(|s| s.parse().ok()).unwrap_or(cfg.seed),
            "--out" => cfg.out_dir = args.next().map(PathBuf::from).unwrap_or(cfg.out_dir),
            "--name" => cfg.name = args.next().unwrap_or(cfg.name),
            "-h" | "--help" => {
                println!(
                    "Usage: report_workflow [--time 80] [--interval 1.0] [--seed 42] \
                            [--out out] [--name brs_mapk_workflow]"
                );
                return;
            }
            _ => {}
        }
    }

    println!(
        "⏱  MAPK report workflow: duration={} interval={} seed={}",
        cfg.duration, cfg.interval, cfg.seed
    );
    match run_report_workflow(&cfg) {
        Ok(report) => {
            println!("📰 {}", report.display());
            println!("   (RunBrs → {{PlotTrajectories, RenderRules, RenderAnimation}} → WriteReport, fired as a DAG)");
        }
        Err(e) => eprintln!("✗ workflow failed: {e}"),
    }
}
