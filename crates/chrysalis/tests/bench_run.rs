//! The **default harness**: a bare `process`/`step` `.ys` file runs standalone —
//! `chrysalis run grow.ys --mass 1 --glucose 5 --time 5` realizes the process in a
//! synthesized place-graph (a state slot per port, self-wired), seeds the inputs
//! from `--port` flags, iterates, and reports the evolved slots. "A heart beating
//! on a bench." This is what makes every `.ys` definer runnable, not just
//! self-contained composites.

use std::collections::BTreeMap;
use std::process::Command;

/// Run a `.ys` file via the cargo-built `chrysalis` and parse its JSON output
/// record into `{field: f64}` (bools become 1.0/0.0).
fn run(file: &str, flags: &[(&str, &str)], time: &str) -> BTreeMap<String, f64> {
    let path = format!("{}/ys/{file}", env!("CARGO_MANIFEST_DIR"));
    let bin = env!("CARGO_BIN_EXE_chrysalis");
    let mut args = vec!["run".to_string(), path, "--time".to_string(), time.to_string()];
    for (k, v) in flags {
        args.push(format!("--{k}"));
        args.push(v.to_string());
    }
    let out = Command::new(bin).args(&args).output().expect("spawn chrysalis");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "run {file} {flags:?} failed: {}\n{stdout}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Minimal JSON-number/bool scrape: the output is a flat `{ "k": v, … }` record.
    let mut map = BTreeMap::new();
    for line in stdout.lines() {
        let line = line.trim().trim_end_matches(',');
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim().trim_matches('"').to_string();
            let v = v.trim();
            let num = if v == "true" {
                Some(1.0)
            } else if v == "false" {
                Some(0.0)
            } else {
                v.parse::<f64>().ok()
            };
            if let Some(n) = num {
                map.insert(key, n);
            }
        }
    }
    map
}

#[test]
fn grow_on_a_bench_grows_and_conserves() {
    // Grow alone, seeded with a finite glucose pool: it converts glucose to biomass
    // + acetate, conserving the total (1 + 5 = 6). The process runs in a synthesized
    // harness — no environment, no parent.
    let s = run("grow.ys", &[("mass", "1.0"), ("glucose", "5.0")], "5");
    let (mass, glucose, acetate) = (s["mass"], s["glucose"], s["acetate"]);
    assert!(mass > 1.0, "biomass grew off the glucose: {mass}");
    assert!(glucose < 5.0 && glucose >= -1e-9, "glucose consumed: {glucose}");
    assert!(acetate > 0.0, "acetate excreted: {acetate}");
    assert!(
        (mass + glucose + acetate - 6.0).abs() < 1e-6,
        "mass conserved on the bench: {mass}+{glucose}+{acetate} = {}",
        mass + glucose + acetate
    );
}

#[test]
fn grow_with_no_fuel_is_inert_but_runs() {
    // No `--glucose` ⇒ the slot defaults to 0 ⇒ no growth, but the file still runs
    // cleanly (every definer is runnable).
    let s = run("grow.ys", &[("mass", "1.0")], "5");
    assert_eq!(s["mass"], 1.0, "no fuel, no growth");
    assert_eq!(s["acetate"], 0.0);
}

#[test]
fn divide_on_a_bench_flips_when_over_threshold() {
    // The propose step alone: above threshold ⇒ divide flips true; below ⇒ false.
    assert_eq!(run("divide.ys", &[("mass", "3.0")], "1")["divide"], 1.0);
    assert_eq!(run("divide.ys", &[("mass", "1.0")], "1")["divide"], 0.0);
}
