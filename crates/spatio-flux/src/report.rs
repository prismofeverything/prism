//! HTML report generator for spatio-flux simulations.
//!
//! Generates a report page with:
//! - Bigraph composite visualization (PNG via graphviz)
//! - Timeseries SVG plots for scalar fields
//! - 2D field heatmap snapshots for spatial data
//! - Links to JSON documents

use std::fmt::Write as FmtWrite;
use std::path::Path;
use std::time::Instant;

use indexmap::IndexMap;

use std::sync::Arc;

use prism_bigraph::{Document, ProcessRegistry, Value, VivariumDocument};
use prism_viz::{render_dot, DotOptions};

use crate::processes::fields::flatten_field;
use crate::vivarium_loader::instantiate_vivarium;

/// A single simulation run result for the report.
struct SimResult {
    name: String,
    status: SimStatus,
    n_processes: usize,
    process_types: Vec<String>,
    runtime_ms: u128,
}

enum SimStatus {
    Ok {
        times: Vec<f64>,
        has_scalar_fields: bool,
        has_probes: bool,
        spatial_field_names: Vec<String>,
        has_particles: bool,
    },
    Skipped(String),
}

/// Report scale: controls simulation duration and detail level.
#[derive(Clone, Copy, Debug)]
pub enum ReportScale {
    /// Minimal: 3 timesteps, fastest possible. For iteration.
    Debug,
    /// Standard: matches Python spatio-flux durations.
    Standard,
    /// Maximal: 10x longer durations for stress testing.
    Max,
}

impl ReportScale {
    fn duration_multiplier(self) -> f64 {
        match self {
            Self::Debug => 0.05,    // 3 steps for a 60s sim
            Self::Standard => 1.0,
            Self::Max => 10.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Standard => "standard",
            Self::Max => "max",
        }
    }
}

/// Canonical simulation order matching Python spatio-flux test suite.
pub const CANONICAL_ORDER: &[&str] = &[
    "monod_kinetics",
    "ecoli_core_dfba",
    "ecoli_dfba",
    "yeast_dfba",
    "community_dfba",
    "dfba_kinetics_community",
    "spatial_many_dfba",
    "spatial_dfba_process",
    "diffusion_process",
    "brownian_particles",
    "br_particles_kinetics",
    "br_particles_dfba",
    "comets_diffusion",
    "comets_br_particles_kinetics",
    "comets_br_particles_dfba",
    "newtonian_particles",
    "comets_nt_particles_dfba",
    "spatioflux_reference_demo",
];

/// Run a single simulation, save plots and metadata to output_dir.
/// Returns runtime in ms.
pub fn run_single_sim(
    name: &str,
    fixture_dir: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    registry: &ProcessRegistry,
) -> Result<u128, String> {
    let fixture_dir = fixture_dir.as_ref();
    let output_dir = output_dir.as_ref();
    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;

    let path = fixture_dir.join(format!("{name}.json"));
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("{name}: {e}"))?;

    let vdoc = VivariumDocument::from_json(&json)
        .map_err(|e| format!("{name}: parse error: {e}"))?;

    let process_types: Vec<String> = vdoc.processes.values()
        .map(|p| p.class_name.clone()).collect();
    let n_processes = vdoc.processes.len();

    // Copy source JSON
    let _ = std::fs::copy(&path, output_dir.join(format!("{name}.json")));

    // Generate bigraph viz
    let topology = vdoc.to_topology();
    let doc = Document::from_topology(&topology);
    let dot = render_dot(&doc, &DotOptions::default());
    let _ = std::fs::write(output_dir.join(format!("{name}.dot")), &dot);
    let _ = prism_viz::render_to_file(&dot, output_dir.join(format!("{name}_viz.svg")), "svg");

    let start = Instant::now();
    let duration = determine_duration(name, ReportScale::Standard);

    // Build a fresh registry for the engine (needs Arc for dynamic discovery)
    let engine_registry = std::sync::Arc::new(crate::from_config::build_registry());
    match instantiate_vivarium(&vdoc, engine_registry) {
        Ok((mut engine, _topology)) => {
            let emit_interval = 1.0;
            let n_steps = (duration / emit_interval).ceil() as usize;
            let mut times = vec![0.0];
            let mut states = vec![engine.state().clone()];

            for _ in 0..n_steps {
                let result = std::panic::catch_unwind(
                    std::panic::AssertUnwindSafe(|| engine.run(emit_interval))
                );
                if result.is_err() {
                    eprintln!("  {name}: panicked at t={:.0}", engine.time());
                    break;
                }
                times.push(engine.time());
                states.push(engine.state().clone());
            }

            let runtime_ms = start.elapsed().as_millis();
            println!("  {name}: {n_processes} procs, {duration:.0}s, {runtime_ms}ms");

            // Generate plots
            let (has_scalar, has_probes, spatial_names, has_particles) =
                generate_plots(name, &times, &states, output_dir);

            // Save metadata
            let mut unique_types = process_types.clone();
            unique_types.sort();
            unique_types.dedup();
            let meta = serde_json::json!({
                "name": name,
                "n_processes": n_processes,
                "process_types": unique_types,
                "runtime_ms": runtime_ms,
                "duration": *times.last().unwrap_or(&0.0),
                "n_steps": times.len() - 1,
                "has_scalar_fields": has_scalar,
                "has_probes": has_probes,
                "spatial_field_names": spatial_names,
                "has_particles": has_particles,
                "status": "ok",
            });
            let _ = std::fs::write(
                output_dir.join(format!("{name}_meta.json")),
                serde_json::to_string_pretty(&meta).unwrap_or_default(),
            );

            Ok(runtime_ms)
        }
        Err(e) => {
            // Save skipped metadata
            let meta = serde_json::json!({
                "name": name,
                "n_processes": n_processes,
                "process_types": process_types,
                "runtime_ms": 0,
                "status": "skipped",
                "reason": e,
            });
            let _ = std::fs::write(
                output_dir.join(format!("{name}_meta.json")),
                serde_json::to_string_pretty(&meta).unwrap_or_default(),
            );
            Err(e)
        }
    }
}

/// Run all simulations.
pub fn run_all_sims(
    fixture_dir: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    registry: &ProcessRegistry,
) -> Result<(), String> {
    let fixture_dir = fixture_dir.as_ref();
    for name in CANONICAL_ORDER {
        if fixture_dir.join(format!("{name}.json")).exists() {
            let _ = run_single_sim(name, fixture_dir, output_dir.as_ref(), registry);
        }
    }
    Ok(())
}

/// Assemble HTML report from cached results (metadata + plots in output_dir).
pub fn assemble_report(
    _fixture_dir: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> Result<(), String> {
    let output_dir = output_dir.as_ref();

    let mut results: Vec<SimResult> = Vec::new();

    for name in CANONICAL_ORDER {
        let meta_path = output_dir.join(format!("{name}_meta.json"));
        if !meta_path.exists() {
            continue;
        }
        let meta_str = std::fs::read_to_string(&meta_path).map_err(|e| e.to_string())?;
        let meta: serde_json::Value = serde_json::from_str(&meta_str).map_err(|e| e.to_string())?;

        let n_processes = meta["n_processes"].as_u64().unwrap_or(0) as usize;
        let process_types: Vec<String> = meta["process_types"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();
        let runtime_ms = meta["runtime_ms"].as_u64().unwrap_or(0) as u128;

        let status = match meta["status"].as_str().unwrap_or("unknown") {
            "ok" => {
                let duration = meta["duration"].as_f64().unwrap_or(0.0);
                let n_steps = meta["n_steps"].as_u64().unwrap_or(0) as usize;
                SimStatus::Ok {
                    times: (0..=n_steps).map(|i| i as f64).collect(),
                    has_scalar_fields: meta["has_scalar_fields"].as_bool().unwrap_or(false),
                    has_probes: meta["has_probes"].as_bool().unwrap_or(false),
                    spatial_field_names: meta["spatial_field_names"]
                        .as_array()
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                    has_particles: meta["has_particles"].as_bool().unwrap_or(false),
                }
            }
            _ => SimStatus::Skipped(
                meta["reason"].as_str().unwrap_or("unknown").to_string()
            ),
        };

        results.push(SimResult {
            name: name.to_string(),
            status,
            n_processes,
            process_types,
            runtime_ms,
        });
    }

    let html = build_html(&results, output_dir, ReportScale::Standard);
    std::fs::write(output_dir.join("index.html"), html).map_err(|e| e.to_string())?;

    let ran = results.iter().filter(|r| matches!(r.status, SimStatus::Ok { .. })).count();
    let total_ms: u128 = results.iter().map(|r| r.runtime_ms).sum();
    println!("Report: {ran}/{} sims, {total_ms}ms total → {}",
        results.len(), output_dir.join("index.html").display());

    Ok(())
}

/// Run all fixtures and generate an HTML report.
pub fn generate_report(
    fixture_dir: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    registry: Arc<ProcessRegistry>,
) -> Result<(), String> {
    generate_report_scaled(fixture_dir, output_dir, registry, ReportScale::Standard)
}

/// Run all fixtures at the given scale and generate an HTML report.
pub fn generate_report_scaled(
    fixture_dir: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    registry: Arc<ProcessRegistry>,
    scale: ReportScale,
) -> Result<(), String> {
    generate_report_focused(fixture_dir, output_dir, registry, scale, &[])
}

/// Run all fixtures, with focus simulations at standard scale and the rest at `scale`.
pub fn generate_report_focused(
    fixture_dir: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    registry: Arc<ProcessRegistry>,
    scale: ReportScale,
    focus: &[String],
) -> Result<(), String> {
    let fixture_dir = fixture_dir.as_ref();
    let output_dir = output_dir.as_ref();
    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;

    // Ordered to match the Python spatio-flux test suite report
    let canonical_order = [
        "monod_kinetics",
        "ecoli_core_dfba",
        "ecoli_dfba",
        "yeast_dfba",
        "community_dfba",
        "dfba_kinetics_community",
        "spatial_many_dfba",
        "spatial_dfba_process",
        "diffusion_process",
        "brownian_particles",
        "br_particles_kinetics",
        "br_particles_dfba",
        "comets_diffusion",
        "comets_br_particles_kinetics",
        "comets_br_particles_dfba",
        "newtonian_particles",
        "comets_nt_particles_dfba",
        "spatioflux_reference_demo",
    ];

    // Use canonical order, then append any extras found on disk
    let mut fixture_names: Vec<String> = Vec::new();
    for name in &canonical_order {
        if fixture_dir.join(format!("{name}.json")).exists() {
            fixture_names.push(name.to_string());
        }
    }
    // Add any fixtures not in the canonical list
    if let Ok(entries) = std::fs::read_dir(fixture_dir) {
        for entry in entries.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.ends_with(".json") {
                let name = fname.trim_end_matches(".json").to_string();
                if !fixture_names.contains(&name) {
                    fixture_names.push(name);
                }
            }
        }
    }

    let mut results: Vec<SimResult> = Vec::new();

    for name in &fixture_names {
        let path = fixture_dir.join(format!("{name}.json"));
        let json = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;

        let vdoc = VivariumDocument::from_json(&json)
            .map_err(|e| format!("{name}: parse error: {e}"))?;

        let process_types: Vec<String> = vdoc
            .processes
            .values()
            .map(|p| p.class_name.clone())
            .collect();
        let n_processes = vdoc.processes.len();

        // Copy the source JSON to output
        std::fs::copy(&path, output_dir.join(format!("{name}.json")))
            .map_err(|e| e.to_string())?;

        // Generate bigraph viz
        let topology = vdoc.to_topology();
        let doc = Document::from_topology(&topology);
        let dot = render_dot(&doc, &DotOptions::default());
        let dot_path = output_dir.join(format!("{name}.dot"));
        std::fs::write(&dot_path, &dot).map_err(|e| e.to_string())?;
        let _ = prism_viz::render_to_file(
            &dot,
            output_dir.join(format!("{name}_viz.svg")),
            "svg",
        );

        let start = Instant::now();

        // In debug mode, skip simulation for non-focused sims (just viz)
        let is_focused = focus.iter().any(|f| f == name);
        let skip_sim = matches!(scale, ReportScale::Debug) && !focus.is_empty() && !is_focused;

        if skip_sim {
            results.push(SimResult {
                name: name.clone(),
                status: SimStatus::Skipped("debug: not focused".into()),
                n_processes,
                process_types,
                runtime_ms: 0,
            });
            continue;
        }

        match instantiate_vivarium(&vdoc, Arc::clone(&registry)) {
            Ok((mut engine, _)) => {
                let sim_scale = if is_focused {
                    ReportScale::Standard
                } else {
                    scale
                };
                let duration = determine_duration(name, sim_scale);
                let emit_interval = 1.0;
                let n_steps = (duration / emit_interval).ceil() as usize;

                let mut times = vec![0.0];
                let mut states = vec![engine.state().clone()];
                let mut sim_failed = false;

                for _ in 0..n_steps {
                    let result = std::panic::catch_unwind(
                        std::panic::AssertUnwindSafe(|| engine.run(emit_interval))
                    );
                    if result.is_err() {
                        eprintln!("  {name}: simulation panicked at t={:.0}, stopping", engine.time());
                        sim_failed = true;
                        break;
                    }
                    times.push(engine.time());
                    states.push(engine.state().clone());
                }

                let runtime_ms = start.elapsed().as_millis();
                println!("  {name}: {n_processes} processes, {duration:.0}s, {runtime_ms}ms{}",
                    if sim_failed { " (FAILED)" } else { "" });

                // Generate plots and determine what we have
                let (has_scalar, has_probes, spatial_names, has_particles) =
                    generate_plots(name, &times, &states, output_dir);

                results.push(SimResult {
                    name: name.clone(),
                    status: SimStatus::Ok {
                        times,
                        has_scalar_fields: has_scalar,
                        has_probes,
                        spatial_field_names: spatial_names,
                        has_particles,
                    },
                    n_processes,
                    process_types,
                    runtime_ms,
                });
            }
            Err(e) => {
                results.push(SimResult {
                    name: name.clone(),
                    status: SimStatus::Skipped(e),
                    n_processes,
                    process_types,
                    runtime_ms: start.elapsed().as_millis(),
                });
            }
        }
    }

    // Generate HTML
    let html = build_html(&results, output_dir, scale);
    std::fs::write(output_dir.join("index.html"), html).map_err(|e| e.to_string())?;

    // Print summary
    let ran = results.iter().filter(|r| matches!(r.status, SimStatus::Ok { .. })).count();
    let skipped = results.len() - ran;
    let total_ms: u128 = results.iter().map(|r| r.runtime_ms).sum();
    println!(
        "Report: {ran} ran, {skipped} skipped, {total_ms}ms total — {}",
        output_dir.join("index.html").display()
    );

    Ok(())
}

fn determine_duration(name: &str, scale: ReportScale) -> f64 {
    // Match Python spatio-flux DEFAULT_RUNTIME values
    let base = match name {
        // DEFAULT_RUNTIME_LONGER = 200
        "brownian_particles" | "br_particles_kinetics" | "comets_diffusion" => 200.0,
        // Custom duration from Python test config
        "spatioflux_reference_demo" => 120.0,
        // DEFAULT_RUNTIME_LONG = 60 (everything else)
        _ => 60.0,
    };
    (base * scale.duration_multiplier()).max(3.0)
}

/// Per-simulation plot options.
struct PlotOptions {
    /// Fields to exclude from timeseries plot.
    exclude_fields: Vec<&'static str>,
    /// Force log scale on timeseries.
    force_log: bool,
    /// For spatial fields: extract timeseries at specific (row,col) points.
    /// Each entry: (label, row, col).
    spatial_probes: Vec<(&'static str, usize, usize)>,
    /// Which spatial field names to probe.
    probe_fields: Vec<&'static str>,
}

fn plot_options(name: &str) -> PlotOptions {
    match name {
        "community_dfba" => PlotOptions {
            exclude_fields: vec!["glucose", "acetate"],
            force_log: true,
            spatial_probes: vec![],
            probe_fields: vec![],
        },
        "spatial_many_dfba" => PlotOptions {
            exclude_fields: vec![],
            force_log: false,
            // 2x4 grid, probe at corners + middle
            spatial_probes: vec![("(0,0)", 0, 0), ("(1,1)", 1, 1), ("(1,3)", 1, 3)],
            probe_fields: vec!["glucose", "acetate", "dissolved biomass"],
        },
        "spatial_dfba_process" => PlotOptions {
            exclude_fields: vec![],
            force_log: false,
            // 5x6 grid (5 cols, 6 rows), max spread: corners + center
            spatial_probes: vec![("(0,0)", 0, 0), ("(5,4)", 5, 4), ("(2,2)", 2, 2)],
            probe_fields: vec!["glucose", "acetate", "dissolved biomass"],
        },
        _ => PlotOptions {
            exclude_fields: vec![],
            force_log: false,
            spatial_probes: vec![],
            probe_fields: vec![],
        },
    }
}

/// Generate timeseries and heatmap plots for a simulation.
/// Returns (has_scalar_fields, has_probes, spatial_field_names, has_particles).
fn generate_plots(
    name: &str,
    times: &[f64],
    states: &[Value],
    output_dir: &Path,
) -> (bool, bool, Vec<String>, bool) {
    let first = &states[0];
    let mut has_scalar = false;
    let mut spatial_names: Vec<String> = Vec::new();
    let opts = plot_options(name);

    // Detect particles early — needed for Y-axis convention.
    // Python's reference uses origin='lower' for particle sims and snapshot grids,
    // but origin='upper' for plain GIF animations (a bug in plot_species_distributions_to_gif).
    // We flip Y for sims whose Python report primarily uses origin='lower' plots:
    // particle sims (fields_and_agents_to_gif) and COMETS sims (plot_snapshots_grid).
    let has_particles = first
        .as_map()
        .and_then(|m| m.get("particles"))
        .and_then(|v| v.as_map())
        .is_some_and(|p| !p.is_empty());
    let uses_snapshot_grid = name.starts_with("comets_");
    let flip_y = has_particles || uses_snapshot_grid;

    // Timeseries for scalar fields
    if let Some(fields) = first.as_map().and_then(|m| m.get("fields")).and_then(|v| v.as_map()) {
        let mut scalar_series: IndexMap<String, Vec<f64>> = IndexMap::new();
        let mut is_spatial: IndexMap<String, bool> = IndexMap::new();

        for (mol_id, val) in fields {
            let flat = flatten_field(val);
            if flat.len() == 1 {
                is_spatial.insert(mol_id.clone(), false);
                if !opts.exclude_fields.contains(&mol_id.as_str()) {
                    scalar_series.insert(mol_id.clone(), Vec::new());
                }
            } else {
                is_spatial.insert(mol_id.clone(), true);
            }
        }

        // Collect scalar timeseries
        for state in states {
            if let Some(fields) = state.as_map().and_then(|m| m.get("fields")).and_then(|v| v.as_map()) {
                for (mol_id, series) in &mut scalar_series {
                    let val = fields
                        .get(mol_id)
                        .map(|v| flatten_field(v))
                        .and_then(|v| v.first().copied())
                        .unwrap_or(0.0);
                    series.push(val);
                }
            }
        }

        // Render scalar timeseries SVG
        if !scalar_series.is_empty() {
            let svg = render_timeseries_svg(name, times, &scalar_series, opts.force_log);
            let _ = std::fs::write(output_dir.join(format!("{name}_timeseries.svg")), svg);
        }

        // Render spatial probe timeseries (sample specific grid points over time)
        if !opts.spatial_probes.is_empty() && !opts.probe_fields.is_empty() {
            let mut probe_series: IndexMap<String, Vec<f64>> = IndexMap::new();

            // Create a series for each (field, probe_point) combination
            for field_name in &opts.probe_fields {
                for (label, _, _) in &opts.spatial_probes {
                    let key = format!("{field_name} {label}");
                    probe_series.insert(key, Vec::new());
                }
            }

            // Infer grid width from first frame's 2D structure
            let probe_nx = states[0]
                .as_map()
                .and_then(|m| m.get("fields"))
                .and_then(|v| v.as_map())
                .and_then(|m| {
                    let (_, first_field) = m.iter().next()?;
                    let rows = first_field.as_list()?;
                    let first_row = rows.first()?.as_list()?;
                    Some(first_row.len())
                })
                .unwrap_or(1);

            // Collect values at each timestep
            for state in states.iter() {
                if let Some(fields_map) = state.as_map()
                    .and_then(|m| m.get("fields"))
                    .and_then(|v| v.as_map())
                {
                    for field_name in &opts.probe_fields {
                        if let Some(field_val) = fields_map.get(*field_name) {
                            for (label, row, col) in &opts.spatial_probes {
                                let key = format!("{field_name} {label}");
                                // Try 2D access first, then flat
                                let val = field_val
                                    .as_list()
                                    .and_then(|rows| {
                                        // 2D: rows[row] is a List
                                        if let Some(r) = rows.get(*row) {
                                            if let Some(cols) = r.as_list() {
                                                return cols.get(*col).and_then(|v| v.as_f64());
                                            }
                                        }
                                        // Flat: index = row * nx + col
                                        let flat_idx = row * probe_nx + col;
                                        rows.get(flat_idx).and_then(|v| v.as_f64())
                                    })
                                    .unwrap_or(0.0);
                                if let Some(series) = probe_series.get_mut(&key) {
                                    series.push(val);
                                }
                            }
                        }
                    }
                }
            }

            if !probe_series.is_empty() {
                has_scalar = true;
                let svg = render_timeseries_svg(
                    &format!("{name} (spatial probes)"), times, &probe_series, false);
                let _ = std::fs::write(
                    output_dir.join(format!("{name}_probes.svg")), svg);
            }
        }

        // Render spatial field animation frames
        for (mol_id, spatial) in &is_spatial {
            if !spatial {
                continue;
            }

            // Use every frame for animation
            let n_frames = states.len();
            let mut frames_json = Vec::new();
            let mut snapshot_svgs = Vec::new();
            let n_snapshots = 8;

            // Infer grid dims from the first frame (which has the original 2D structure).
            // Later frames may be flat (after engine processing), so we lock dims here.
            let first_field_val = states[0]
                .as_map()
                .and_then(|m| m.get("fields"))
                .and_then(|v| v.as_map())
                .and_then(|m| m.get(mol_id));
            let first_flat = first_field_val.map(flatten_field).unwrap_or_default();
            let (grid_nx, grid_ny) = infer_grid_dims(first_field_val, first_flat.len());

            // Compute global max across ALL frames for consistent colormap
            let global_max = states.iter()
                .filter_map(|s| {
                    s.as_map()
                        .and_then(|m| m.get("fields"))
                        .and_then(|v| v.as_map())
                        .and_then(|m| m.get(mol_id))
                        .map(flatten_field)
                })
                .flat_map(|f| f.into_iter())
                .fold(0.0_f64, f64::max);

            for frame_i in 0..n_frames {
                let field_val = states[frame_i]
                    .as_map()
                    .and_then(|m| m.get("fields"))
                    .and_then(|v| v.as_map())
                    .and_then(|m| m.get(mol_id));
                let field = field_val.map(flatten_field).unwrap_or_default();
                let (nx, ny) = (grid_nx, grid_ny);

                if !field.is_empty() {
                    let mut svg = render_heatmap_svg(&field, nx, ny, Some(global_max), flip_y);
                    // Overlay particle positions on the field
                    overlay_particles(&mut svg, &states[frame_i], nx, ny, flip_y);
                    let t = times.get(frame_i).unwrap_or(&0.0);
                    frames_json.push(format!(
                        "{{\"t\":{t:.1},\"svg\":\"{}\"}}",
                        svg.replace('"', "\\\"").replace('\n', "")
                    ));

                    let snap_step = ((n_frames - 1) / n_snapshots.max(1)).max(1);
                    if frame_i % snap_step == 0 || frame_i == n_frames - 1 {
                        snapshot_svgs.push((format!("t={t:.0}"), svg));
                    }
                }
            }

            // Write animation data as JSON
            if !frames_json.is_empty() {
                let anim_json = format!("[{}]", frames_json.join(","));
                let _ = std::fs::write(
                    output_dir.join(format!("{name}_{mol_id}_frames.json")),
                    &anim_json,
                );
            }

            // Write static snapshot grid
            if !snapshot_svgs.is_empty() {
                let combined = render_snapshot_grid(mol_id, &snapshot_svgs);
                let _ = std::fs::write(
                    output_dir.join(format!("{name}_{mol_id}_spatial.svg")),
                    combined,
                );
            }
        }

        has_scalar = !scalar_series.is_empty();
        spatial_names = is_spatial
            .iter()
            .filter(|(_, s)| **s)
            .map(|(k, _)| k.clone())
            .collect();
    }

    // Particle traces
    if has_particles {
        let svg = render_particle_traces(name, times, states);
        let _ = std::fs::write(output_dir.join(format!("{name}_traces.svg")), svg);
        let svg = render_particle_mass(name, times, states);
        let _ = std::fs::write(output_dir.join(format!("{name}_mass.svg")), svg);

        // Particle animation frames — sample ~60 frames max to avoid huge inline JSON
        let bounds = detect_domain_bounds(&states[0]);
        let mut particle_frames = Vec::new();
        let mut particle_snapshots = Vec::new();
        let n_snap = 6;
        let max_anim_frames = 60;
        let anim_step = (states.len() / max_anim_frames).max(1);
        let snap_step = ((states.len() - 1) / n_snap.max(1)).max(1);

        for (frame_i, state) in states.iter().enumerate() {
            if frame_i % anim_step == 0 || frame_i == states.len() - 1 {
                let svg = render_particle_frame(state, bounds, frame_i);
                let t = times.get(frame_i).unwrap_or(&0.0);
                particle_frames.push(format!(
                    "{{\"t\":{t:.1},\"svg\":\"{}\"}}",
                    svg.replace('"', "\\\"").replace('\n', "")
                ));
            }
            if frame_i % snap_step == 0 || frame_i == states.len() - 1 {
                let svg = render_particle_frame(state, bounds, frame_i);
                let t = times.get(frame_i).unwrap_or(&0.0);
                particle_snapshots.push((format!("t={t:.0}"), svg));
            }
        }

        let frames_json = format!("[{}]", particle_frames.join(","));
        let _ = std::fs::write(
            output_dir.join(format!("{name}_particles_frames.json")),
            &frames_json,
        );
        let snap_svg = render_snapshot_grid("particles", &particle_snapshots);
        let _ = std::fs::write(
            output_dir.join(format!("{name}_particles_spatial.svg")),
            snap_svg,
        );
    }

    let has_probes = !opts.spatial_probes.is_empty() && !opts.probe_fields.is_empty();
    (has_scalar, has_probes, spatial_names, has_particles)
}

/// Render a timeseries plot as SVG using plotters.
fn render_timeseries_svg(
    title: &str,
    times: &[f64],
    series: &IndexMap<String, Vec<f64>>,
    force_log: bool,
) -> String {
    use plotters::prelude::*;

    let mut buf = String::new();
    {
        let root = SVGBackend::with_string(&mut buf, (800, 450)).into_drawing_area();
        root.fill(&WHITE).unwrap();

        let t_max = *times.last().unwrap_or(&1.0);
        let y_min = series
            .values()
            .flat_map(|s| s.iter())
            .copied()
            .filter(|v| *v > 0.0)
            .fold(f64::MAX, f64::min);
        let y_max = series
            .values()
            .flat_map(|s| s.iter())
            .copied()
            .fold(0.0_f64, f64::max)
            .max(0.01);

        // Use log scale only when the max value across series differs
        // by > 1000x from the median of series maxima (truly multi-scale data)
        let mut series_maxes: Vec<f64> = series
            .values()
            .map(|s| s.iter().copied().fold(0.0_f64, f64::max))
            .filter(|&m| m > 0.0)
            .collect();
        series_maxes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let use_log = force_log || if series_maxes.len() >= 2 {
            let median = series_maxes[series_maxes.len() / 2];
            let max = *series_maxes.last().unwrap();
            max / median > 1000.0
        } else {
            false
        };

        // For log scale we transform values manually (plotters log coord
        // requires feature flags we don't have). For linear, use normal range.
        let (plot_ymin, plot_ymax) = if use_log {
            (y_min.log10().floor(), (y_max * 1.2).log10().ceil())
        } else {
            (0.0, y_max * 1.15)
        };

        let mut chart = ChartBuilder::on(&root)
            .caption(
                if use_log { format!("{title} (log scale)") } else { title.to_string() },
                ("sans-serif", 18).into_font(),
            )
            .margin(10)
            .x_label_area_size(35)
            .y_label_area_size(55)
            .build_cartesian_2d(0.0..t_max, plot_ymin..plot_ymax)
            .unwrap();

        chart
            .configure_mesh()
            .x_desc("time")
            .y_desc(if use_log { "log10(concentration)" } else { "concentration" })
            .draw()
            .unwrap();

        for (i, (mol_id, values)) in series.iter().enumerate() {
            // Matching STANDARD_FIELD_COLORS from Python spatio-flux.
            // Use starts_with to handle probe labels like "glucose (0,0)".
            let id = mol_id.as_str();
            let color = if id.starts_with("glucose") {
                RGBColor(0x1f, 0x77, 0xb4)             // #1f77b4 blue
            } else if id.starts_with("acetate") {
                RGBColor(0xff, 0x7f, 0x0e)             // #ff7f0e orange
            } else if id.starts_with("dissolved biomass") {
                RGBColor(0x17, 0xbe, 0xcf)             // #17becf teal
            } else if id.starts_with("biomass") || id.starts_with("dfba_biomass")
                || id.starts_with("ecoli core biomass") || id.starts_with("ecoli_core")
                || id.starts_with("monod_biomass") || id.starts_with("mass") {
                RGBColor(0x2c, 0xa0, 0x2c)             // #2ca02c green
            } else if id.starts_with("formate") {
                RGBColor(0x94, 0x67, 0xbd)             // #9467bd purple
            } else if id.starts_with("ammonium") {
                RGBColor(0xbc, 0xbd, 0x22)             // #bcbd22 yellow-green
            } else if id.starts_with("kinetic_biomass") {
                RGBColor(0x1b, 0x9e, 0x77)             // #1b9e77 dark teal
            } else {
                [RGBColor(0xd6, 0x27, 0x28),            // red
                 RGBColor(0x94, 0x67, 0xbd),            // purple
                 RGBColor(0x17, 0xbe, 0xcf),            // teal
                 RGBColor(0x8c, 0x56, 0x4b)]            // brown
                [i % 4]
            };
            let data: Vec<(f64, f64)> = times
                .iter()
                .zip(values.iter())
                .map(|(&t, &v)| {
                    let y = if use_log {
                        if v > 0.0 { v.log10() } else { plot_ymin }
                    } else {
                        v
                    };
                    (t, y)
                })
                .collect();

            chart
                .draw_series(LineSeries::new(data, color.stroke_width(2)))
                .unwrap()
                .label(mol_id)
                .legend(move |(x, y)| {
                    PathElement::new(vec![(x, y), (x + 15, y)], color.stroke_width(2))
                });
        }

        chart
            .configure_series_labels()
            .background_style(WHITE.mix(0.8))
            .border_style(BLACK)
            .draw()
            .unwrap();

        root.present().unwrap();
    }
    buf
}

/// Render a 2D heatmap as inline SVG.
/// `fixed_max`: if Some, use this as the colormap max (for consistent scaling across frames).
fn render_heatmap_svg(data: &[f64], nx: usize, ny: usize, fixed_max: Option<f64>, flip_y: bool) -> String {
    let cell_size = if nx.max(ny) <= 5 { 24 } else if nx.max(ny) <= 10 { 16 } else { 10 };
    let width = nx * cell_size;
    let height = ny * cell_size;

    let max_val = fixed_max
        .unwrap_or_else(|| data.iter().copied().fold(0.0_f64, f64::max))
        .max(1e-10);

    let mut svg = format!(
        r#"<svg width="{width}" height="{height}" viewBox="0 0 {width} {height}" xmlns="http://www.w3.org/2000/svg">"#,
    );

    for y in 0..ny {
        for x in 0..nx {
            let idx = y * nx + x;
            let val = data.get(idx).copied().unwrap_or(0.0);
            // Blue → Green → Yellow colormap (like viridis)
            let t = (val / max_val).clamp(0.0, 1.0);
            let (r, g, b) = if t < 0.5 {
                // Blue (68,1,84) → Green (33,145,140)
                let s = t * 2.0;
                (
                    (68.0 + s * (33.0 - 68.0)) as u8,
                    (1.0 + s * (145.0 - 1.0)) as u8,
                    (84.0 + s * (140.0 - 84.0)) as u8,
                )
            } else {
                // Green (33,145,140) → Yellow (253,231,37)
                let s = (t - 0.5) * 2.0;
                (
                    (33.0 + s * (253.0 - 33.0)) as u8,
                    (145.0 + s * (231.0 - 145.0)) as u8,
                    (140.0 + s * (37.0 - 140.0)) as u8,
                )
            };
            let _ = write!(
                svg,
                r#"<rect x="{}" y="{}" width="{cell_size}" height="{cell_size}" fill="rgb({r},{g},{b})"/>"#,
                x * cell_size,
                if flip_y { (ny - 1 - y) * cell_size } else { y * cell_size },
            );
        }
    }

    svg.push_str("</svg>");
    svg
}

/// Combine multiple heatmap snapshots into a horizontal strip.
fn render_snapshot_grid(_mol_id: &str, snapshots: &[(String, String)]) -> String {
    // Extract the width of the first snapshot SVG to compute spacing
    let snap_width = snapshots
        .first()
        .and_then(|(_, svg)| {
            svg.split("width=\"").nth(1)
                .and_then(|s| s.split('"').next())
                .and_then(|s| s.parse::<usize>().ok())
        })
        .unwrap_or(80);
    let snap_height = snapshots
        .first()
        .and_then(|(_, svg)| {
            svg.split("height=\"").nth(1)
                .and_then(|s| s.split('"').next())
                .and_then(|s| s.parse::<usize>().ok())
        })
        .unwrap_or(80);

    let gap = 4;
    let label_h = 12;
    let total_w = snapshots.len() * (snap_width + gap);
    let total_h = snap_height + label_h + 4;

    let mut html = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{total_w}" height="{total_h}">"#,
    );

    for (i, (label, svg_inner)) in snapshots.iter().enumerate() {
        let x = i * (snap_width + gap);
        let _ = write!(
            html,
            "<text x=\"{x}\" y=\"{label_h}\" font-size=\"9\" font-family=\"sans-serif\" fill=\"gray\">{label}</text>\
             <g transform=\"translate({x}, {lh})\">{svg_inner}</g>",
            lh = label_h + 2,
        );
    }

    html.push_str("</svg>");
    html
}

/// Overlay particle positions as circles on a heatmap SVG.
/// Inserts circles before the closing </svg> tag.
fn overlay_particles(svg: &mut String, state: &Value, nx: usize, ny: usize, flip_y: bool) {
    let particles = match state.as_map()
        .and_then(|m| m.get("particles"))
        .and_then(|v| v.as_map())
    {
        Some(p) if !p.is_empty() => p,
        _ => return,
    };

    // Detect cell size from SVG width/height
    let cell_size = if nx.max(ny) <= 5 { 24 } else if nx.max(ny) <= 10 { 16 } else { 10 };
    let svg_w = nx * cell_size;
    let svg_h = ny * cell_size;

    // Detect bounds from particle exchange config or use field-proportional default.
    // Common spatio-flux configs: 50×50 for 10×10, etc.
    // For now use a fixed 50×50 (the most common).
    // TODO: read from config
    let bounds_x = 50.0;
    let bounds_y = 50.0;

    // Remove closing </svg> tag
    if let Some(pos) = svg.rfind("</svg>") {
        svg.truncate(pos);
    }

    for (i, (_, p)) in particles.iter().enumerate() {
        if let Some(pos) = p.as_map().and_then(|m| m.get("position")).and_then(|v| v.as_list()) {
            let x = pos.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
            let y = pos.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
            let mass = get_mass(p);

            // Map particle position to SVG coordinates.
            let px = (x / bounds_x * svg_w as f64) as i32;
            let py = if flip_y {
                svg_h as i32 - (y / bounds_y * svg_h as f64) as i32
            } else {
                (y / bounds_y * svg_h as f64) as i32
            };
            // Use actual physics radius if available, else derive from mass
            let phys_radius = get_radius(p);
            let radius = (phys_radius / bounds_x * svg_w as f64).max(0.5).min(cell_size as f64 / 2.0) as i32;
            let (r, g, b) = TAB20[i % TAB20.len()];

            let _ = write!(svg,
                "<circle cx=\"{px}\" cy=\"{py}\" r=\"{radius}\" fill=\"rgb({r},{g},{b})\" \
                 stroke=\"white\" stroke-width=\"1\" opacity=\"0.9\"/>");
        }
    }

    svg.push_str("</svg>");
}

fn get_mass(particle: &Value) -> f64 {
    if let Some(map) = particle.as_map() {
        // Sum sub_masses if available (for community particles)
        if let Some(sub_masses) = map.get("sub_masses").and_then(|v| v.as_map()) {
            let total: f64 = sub_masses.values().filter_map(|v| v.as_f64()).sum();
            if total > 0.0 {
                return total;
            }
        }
        map.get("mass").and_then(|v| v.as_f64()).unwrap_or(0.0)
    } else {
        0.0
    }
}

fn get_radius(particle: &Value) -> f64 {
    // Use stored radius if available and non-zero, else derive from mass.
    // For sub_masses particles, always derive from total mass (sub_masses may grow).
    if let Some(map) = particle.as_map() {
        let has_sub_masses = map.get("sub_masses")
            .and_then(|v| v.as_map())
            .is_some_and(|sm| sm.values().any(|v| v.as_f64().is_some()));
        if !has_sub_masses {
            if let Some(r) = map.get("radius").and_then(|v| v.as_f64()) {
                if r > 0.0 {
                    return r;
                }
            }
        }
    }
    let mass = get_mass(particle).max(0.001);
    (mass / (0.015 * std::f64::consts::PI)).sqrt()
}

/// Matplotlib tab20 color palette (20 distinct colors).
const TAB20: [(u8, u8, u8); 20] = [
    (0x1f, 0x77, 0xb4), (0xaf, 0xc7, 0xe8), (0xff, 0x7f, 0x0e), (0xff, 0xbb, 0x78),
    (0x2c, 0xa0, 0x2c), (0x98, 0xdf, 0x8a), (0xd6, 0x27, 0x28), (0xff, 0x98, 0x96),
    (0x94, 0x67, 0xbd), (0xc5, 0xb0, 0xd5), (0x8c, 0x56, 0x4b), (0xc4, 0x9c, 0x94),
    (0xe3, 0x77, 0xc2), (0xf7, 0xb6, 0xd2), (0x7f, 0x7f, 0x7f), (0xc7, 0xc7, 0xc7),
    (0xbc, 0xbd, 0x22), (0xdb, 0xdb, 0x8d), (0x17, 0xbe, 0xcf), (0x9e, 0xda, 0xe5),
];

fn tab20_color(i: usize) -> plotters::style::RGBColor {
    let (r, g, b) = TAB20[i % TAB20.len()];
    plotters::style::RGBColor(r, g, b)
}

/// Render particle traces as compact SVG polylines.
fn render_particle_traces(title: &str, _times: &[f64], states: &[Value]) -> String {
    let mut traces: IndexMap<String, Vec<(f64, f64)>> = IndexMap::new();

    for state in states {
        if let Some(particles) = state.as_map().and_then(|m| m.get("particles")).and_then(|v| v.as_map()) {
            for (pid, particle) in particles {
                if let Some(pos) = particle.as_map().and_then(|m| m.get("position")).and_then(|v| v.as_list()) {
                    let x = pos.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let y = pos.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    traces.entry(pid.clone()).or_default().push((x, y));
                }
            }
        }
    }

    let (w, h) = (400, 400);
    let bounds = detect_domain_bounds(&states[0]);
    let (bx0, by0, bx1, by1) = bounds;
    let sx = w as f64 / (bx1 - bx0).max(1.0);
    let sy = h as f64 / (by1 - by0).max(1.0);

    let mut svg = format!(
        "<svg width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" xmlns=\"http://www.w3.org/2000/svg\">\
         <rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>\
         <text x=\"{mid}\" y=\"16\" text-anchor=\"middle\" font-size=\"14\" font-family=\"sans-serif\">{title} traces</text>",
        mid = w / 2,
    );

    for (i, (_, pts)) in traces.iter().enumerate() {
        if pts.len() < 2 { continue; }
        let (r, g, b) = TAB20[i % TAB20.len()];
        let points: String = pts.iter()
            .map(|(x, y)| format!("{:.0},{:.0}", (x - bx0) * sx, h as f64 - (y - by0) * sy))
            .collect::<Vec<_>>()
            .join(" ");
        let _ = write!(svg,
            "<polyline points=\"{points}\" fill=\"none\" stroke=\"rgb({r},{g},{b})\" stroke-width=\"1\" opacity=\"0.7\"/>");
    }

    svg.push_str("</svg>");
    svg
}

/// Render particle mass over time as compact SVG polylines.
fn render_particle_mass(title: &str, times: &[f64], states: &[Value]) -> String {
    let mut mass_series: IndexMap<String, Vec<(f64, f64)>> = IndexMap::new();

    for (t_idx, state) in states.iter().enumerate() {
        let t = times.get(t_idx).copied().unwrap_or(0.0);
        if let Some(particles) = state.as_map().and_then(|m| m.get("particles")).and_then(|v| v.as_map()) {
            for (pid, particle) in particles {
                let mass = get_mass(particle);
                mass_series.entry(pid.clone()).or_default().push((t, mass));
            }
        }
    }

    let t_max = *times.last().unwrap_or(&1.0);
    let y_max = mass_series
        .values()
        .flat_map(|s| s.iter())
        .map(|(_, m)| *m)
        .fold(0.01_f64, f64::max) * 1.1;

    let (w, h) = (500, 300);
    let margin = 40;
    let pw = (w - margin * 2) as f64;
    let ph = (h - margin * 2) as f64;

    let mut svg = format!(
        "<svg width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" xmlns=\"http://www.w3.org/2000/svg\">\
         <rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>\
         <text x=\"{mid}\" y=\"16\" text-anchor=\"middle\" font-size=\"14\" font-family=\"sans-serif\">{title} mass</text>\
         <text x=\"{mid}\" y=\"{bot}\" text-anchor=\"middle\" font-size=\"10\" font-family=\"sans-serif\">time</text>\
         <text x=\"10\" y=\"{ymid}\" text-anchor=\"middle\" font-size=\"10\" font-family=\"sans-serif\" transform=\"rotate(-90 10 {ymid})\">mass</text>",
        mid = w / 2, bot = h - 5, ymid = h / 2,
    );

    for (i, (_, pts)) in mass_series.iter().enumerate() {
        if pts.is_empty() { continue; }
        let (r, g, b) = TAB20[i % TAB20.len()];
        let points: String = pts.iter()
            .map(|(t, m)| {
                let x = margin as f64 + (t / t_max) * pw;
                let y = (h - margin) as f64 - (m / y_max) * ph;
                format!("{x:.0},{y:.0}")
            })
            .collect::<Vec<_>>()
            .join(" ");
        let _ = write!(svg,
            "<polyline points=\"{points}\" fill=\"none\" stroke=\"rgb({r},{g},{b})\" stroke-width=\"1.5\" opacity=\"0.7\"/>");
    }

    svg.push_str("</svg>");
    svg
}

/// Build the final HTML report page.
/// Infer grid dimensions from a field value.
/// If it's a 2D array (list of lists), use outer=ny, inner=nx.
/// Otherwise fall back to sqrt.
fn infer_grid_dims(field_val: Option<&Value>, flat_len: usize) -> (usize, usize) {
    if let Some(Value::List(rows)) = field_val {
        if let Some(Value::List(first_row)) = rows.first() {
            let ny = rows.len();
            let nx = first_row.len();
            if nx > 0 && ny > 0 {
                return (nx, ny);
            }
        }
    }
    // Fallback: square-ish
    let n = (flat_len as f64).sqrt().ceil() as usize;
    let nx = n.max(1);
    let ny = (flat_len + nx - 1) / nx;
    (nx, ny)
}

/// Detect the bounding box of all particles across all timesteps.
/// Extract the domain bounds from process configs in the state.
/// Looks for "bounds" in known process configs (newtonian_particles,
/// brownian_movement, enforce_boundaries, etc.)
fn detect_domain_bounds(state: &Value) -> (f64, f64, f64, f64) {
    let check_keys = [
        "newtonian_particles", "brownian_movement", "enforce_boundaries",
        "particle_exchange",
    ];
    if let Some(map) = state.as_map() {
        for key in &check_keys {
            if let Some(bounds) = map.get(*key)
                .and_then(|v| v.as_map())
                .and_then(|m| m.get("config"))
                .and_then(|v| v.as_map())
                .and_then(|m| m.get("bounds"))
                .and_then(|v| v.as_list())
            {
                let bx = bounds.first().and_then(|v| v.as_f64()).unwrap_or(50.0);
                let by = bounds.get(1).and_then(|v| v.as_f64()).unwrap_or(50.0);
                return (0.0, 0.0, bx, by);
            }
        }
    }
    // Fallback: scan particle positions
    (0.0, 0.0, 50.0, 50.0)
}

/// Render a single frame of particle positions as inline SVG.
fn render_particle_frame(
    state: &Value,
    bounds: (f64, f64, f64, f64),
    _frame_idx: usize,
) -> String {
    let (bx0, by0, bx1, by1) = bounds;
    let w = 300;
    let h = 300;
    let scale_x = w as f64 / (bx1 - bx0).max(1.0);
    let scale_y = h as f64 / (by1 - by0).max(1.0);

    let mut svg = format!(
        "<svg width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" xmlns=\"http://www.w3.org/2000/svg\">\
         <rect width=\"{w}\" height=\"{h}\" fill=\"#f0f0f0\" stroke=\"#ccc\"/>"
    );

    if let Some(particles) = state.as_map().and_then(|m| m.get("particles")).and_then(|v| v.as_map()) {
        for (i, (_, p)) in particles.iter().enumerate() {
            if let Some(pos) = p.as_map().and_then(|m| m.get("position")).and_then(|v| v.as_list()) {
                let x = pos.first().and_then(|v| v.as_f64()).unwrap_or(0.0);
                let y = pos.get(1).and_then(|v| v.as_f64()).unwrap_or(0.0);
                let (r, g, b) = TAB20[i % TAB20.len()];
                let px = ((x - bx0) * scale_x) as i32;
                let py = h as i32 - ((y - by0) * scale_y) as i32; // flip Y
                // Use actual physics radius, scaled to SVG pixels
                let phys_radius = get_radius(p);
                let radius = (phys_radius * scale_x).max(0.5).min(30.0) as i32;
                let _ = write!(
                    svg,
                    "<circle cx=\"{px}\" cy=\"{py}\" r=\"{radius}\" fill=\"rgb({r},{g},{b})\" opacity=\"0.8\"/>"
                );
            }
        }
    }

    svg.push_str("</svg>");
    svg
}

fn sanitize_html(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

fn build_html(results: &[SimResult], output_dir: &Path, scale: ReportScale) -> String {
    let ran = results.iter().filter(|r| matches!(r.status, SimStatus::Ok { .. })).count();
    let skipped = results.len() - ran;
    let total_ms: u128 = results.iter().map(|r| r.runtime_ms).sum();

    let mut html = format!(r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Spatio-Flux Report (Prism/Rust) [{scale_label}]</title>
<style>
  body {{ font-family: -apple-system, sans-serif; max-width: 1800px; margin: 0 auto; padding: 20px; background: #fafafa; }}
  h1 {{ color: #333; border-bottom: 2px solid #4a9eff; padding-bottom: 10px; }}
  h2 {{ color: #555; margin-top: 40px; }}
  .summary {{ background: #e8f4fd; padding: 15px; border-radius: 8px; margin: 20px 0; }}
  .sim {{ background: white; border: 1px solid #ddd; border-radius: 8px; padding: 20px; margin: 20px 0; }}
  .sim.skipped {{ opacity: 0.6; }}
  .status {{ display: inline-block; padding: 2px 10px; border-radius: 4px; font-size: 12px; font-weight: bold; }}
  .status.ok {{ background: #d4edda; color: #155724; }}
  .status.skip {{ background: #fff3cd; color: #856404; }}
  .process-types {{ font-size: 12px; color: #666; margin: 5px 0; }}
  .plots {{ display: flex; flex-wrap: wrap; gap: 20px; margin-top: 15px; }}
  .plots img, .plots object {{ max-width: 100%; border: 1px solid #eee; border-radius: 4px; }}
  a {{ color: #4a9eff; }}
  table {{ border-collapse: collapse; width: 100%; }}
  th, td {{ padding: 8px 12px; text-align: left; border-bottom: 1px solid #eee; }}
  th {{ background: #f5f5f5; }}
  .anim-container {{ position: relative; display: inline-block; width: 100%; }}
  .anim-container svg {{ width: 100%; height: auto; }}
  .anim-label {{ font-size: 12px; color: #666; }}
  .anim-controls {{ margin-top: 4px; font-size: 12px; }}
  .anim-controls button {{ padding: 2px 8px; cursor: pointer; }}
</style>
<script>
function startAnimInline(id, frames) {{
  var el = document.getElementById(id);
  var label = document.getElementById(id + '_label');
  if (!el || !frames || frames.length === 0) return;
  var idx = 0;
  function show() {{
    if (!frames[idx]) return;
    el.innerHTML = frames[idx].svg;
    var svg = el.querySelector('svg');
    if (svg) {{ svg.removeAttribute('width'); svg.removeAttribute('height'); svg.style.width = '100%'; }}
    if (label) label.textContent = 't=' + frames[idx].t.toFixed(1);
  }}
  show();
  setInterval(function() {{
    idx = (idx + 1) % frames.length;
    show();
  }}, 80);
}}
</script>
</head>
<body>
<h1>Spatio-Flux Test Suite Report</h1>
<p><em>Generated by Prism (Rust)</em></p>

<div class="summary">
  <strong>{ran}</strong> simulations ran &middot;
  <strong>{skipped}</strong> skipped &middot;
  <strong>{total_ms}</strong>ms total runtime &middot;
  <strong>{}</strong> total simulations
</div>

<table>
<tr><th>Simulation</th><th>Status</th><th>Processes</th><th>Runtime</th></tr>
"#, results.len(), scale_label = scale.label());

    for r in results {
        let (status_class, status_text) = match &r.status {
            SimStatus::Ok { .. } => ("ok", "RAN"),
            SimStatus::Skipped(_) => ("skip", "SKIPPED"),
        };
        let _ = write!(
            html,
            "<tr>\n  <td><a href=\"#{name}\">{name}</a></td>\n  \
             <td><span class=\"status {status_class}\">{status_text}</span></td>\n  \
             <td>{n}</td>\n  <td>{ms}ms</td>\n</tr>\n",
            name = r.name,
            status_class = status_class,
            status_text = status_text,
            n = r.n_processes,
            ms = r.runtime_ms,
        );
    }
    html.push_str("</table>\n\n");

    // Detail sections
    for r in results {
        let skipped_class = if matches!(r.status, SimStatus::Skipped(_)) { " skipped" } else { "" };
        let _ = write!(
            html,
            "<div class=\"sim{skipped}\" id=\"{name}\">\n<h2>{name}</h2>\n",
            skipped = skipped_class,
            name = r.name,
        );

        // Unique process types
        let mut unique_types = r.process_types.clone();
        unique_types.sort();
        unique_types.dedup();
        let _ = write!(
            html,
            "<p class=\"process-types\">{n} processes: {types}</p>\n\
             <p><a href=\"{name}.json\">{name}.json</a></p>\n",
            n = r.n_processes,
            types = unique_types.join(", "),
            name = r.name,
        );

        match &r.status {
            SimStatus::Ok {
                times,
                has_scalar_fields,
                has_probes,
                spatial_field_names,
                has_particles,
            } => {
                let _ = write!(
                    html,
                    "<p>Duration: {:.1}s ({} steps, {}ms)</p>\n",
                    times.last().unwrap_or(&0.0),
                    times.len() - 1,
                    r.runtime_ms,
                );

                html.push_str("<div class=\"plots\">");

                // Viz
                let _ = write!(
                    html,
                    "<div style=\"width:100%\"><h3>Composition</h3><object data=\"{name}_viz.svg\" type=\"image/svg+xml\" style=\"width:100%\"></object></div>",
                    name = r.name,
                );

                // Timeseries (only if scalar fields exist)
                if *has_scalar_fields {
                    let _ = write!(
                        html,
                        "<div><h3>Timeseries</h3><object data=\"{name}_timeseries.svg\" \
                         type=\"image/svg+xml\" style=\"width:100%;max-width:800px\"></object></div>",
                        name = r.name,
                    );
                }

                // Spatial probe timeseries
                if *has_probes {
                    let _ = write!(
                        html,
                        "<div><h3>Spatial Probes</h3><object data=\"{name}_probes.svg\" \
                         type=\"image/svg+xml\" style=\"width:100%;max-width:800px\"></object></div>",
                        name = r.name,
                    );
                }

                // Spatial field animations — side by side in a row
                if !spatial_field_names.is_empty() {
                    html.push_str("</div>\n\
                        <h3>Spatial Fields</h3>\n\
                        <div style=\"display:flex;flex-wrap:wrap;gap:16px;align-items:flex-start;justify-content:space-around;width:100%\">");

                    for mol_id in spatial_field_names {
                        let anim_id = format!("anim_{}_{}", sanitize_html(&r.name), sanitize_html(mol_id));
                        let frames_path = output_dir.join(format!("{}_{}_frames.json", r.name, mol_id));

                        let frames_data = std::fs::read_to_string(&frames_path).unwrap_or_default();
                        if !frames_data.is_empty() {
                            let _ = write!(
                                html,
                                "<div style=\"text-align:center;flex:1\">\
                                 <div style=\"font-weight:bold;margin-bottom:4px\">{mol_id}</div>\
                                 <div class=\"anim-label\" id=\"{anim_id}_label\">t=0</div>\
                                 <div class=\"anim-container\" id=\"{anim_id}\"></div>\
                                 <script>startAnimInline('{anim_id}',{frames_data})</script>\
                                 </div>",
                            );
                        }
                    }

                    html.push_str("</div>\n");

                    // Snapshots as compact matrix: each row = field name + snapshot strip
                    html.push_str("<h3>Snapshots</h3>\n\
                        <div style=\"overflow-x:auto\">\
                        <table style=\"border-collapse:collapse;font-size:11px\">");
                    for mol_id in spatial_field_names {
                        let static_url = format!("{}_{}_spatial.svg", r.name, mol_id);
                        let _ = write!(
                            html,
                            "<tr>\
                             <td style=\"padding:2px 8px 2px 0;font-weight:bold;vertical-align:top;white-space:nowrap\">{mol_id}</td>\
                             <td style=\"padding:2px\"><object data=\"{static_url}\" type=\"image/svg+xml\"></object></td>\
                             </tr>",
                        );
                    }
                    html.push_str("</table></div>\n<div class=\"plots\">");
                }

                // Particle plots (only if particles exist)
                if *has_particles {
                    // Animation
                    let panim_id = format!("panim_{}", sanitize_html(&r.name));
                    let frames_path = output_dir.join(format!("{}_particles_frames.json", r.name));
                    let frames_data = std::fs::read_to_string(&frames_path).unwrap_or_default();
                    if !frames_data.is_empty() {
                        let _ = write!(
                            html,
                            "</div>\n\
                             <h3>Particle Animation</h3>\
                             <div style=\"display:flex;gap:16px;align-items:flex-start;justify-content:space-around;width:100%\">\
                             <div>\
                             <div class=\"anim-label\" id=\"{panim_id}_label\">t=0</div>\
                             <div class=\"anim-container\" id=\"{panim_id}\"></div>\
                             <script>startAnimInline('{panim_id}',{frames_data})</script>\
                             </div>\
                             <div><object data=\"{name}_traces.svg\" type=\"image/svg+xml\" width=\"400\"></object></div>\
                             <div><object data=\"{name}_mass.svg\" type=\"image/svg+xml\" width=\"400\"></object></div>\
                             </div>\n\
                             <div><object data=\"{name}_particles_spatial.svg\" type=\"image/svg+xml\"></object></div>\n\
                             <div class=\"plots\">",
                            name = r.name,
                        );
                    } else {
                        let _ = write!(
                            html,
                            "<div><h3>Particle Traces</h3><object data=\"{name}_traces.svg\" \
                             type=\"image/svg+xml\" width=\"400\"></object></div>\
                             <div><h3>Particle Mass</h3><object data=\"{name}_mass.svg\" \
                             type=\"image/svg+xml\" width=\"500\"></object></div>",
                            name = r.name,
                        );
                    }
                }

                html.push_str("</div>\n");
            }
            SimStatus::Skipped(reason) => {
                let _ = write!(html, "<p><em>Skipped: {reason}</em></p>\n");
                // Still show viz if available
                let _ = write!(
                    html,
                    "<div class=\"plots\"><div style=\"width:100%\"><h3>Composition</h3>\
                     <object data=\"{name}_viz.svg\" type=\"image/svg+xml\" style=\"width:100%\"></object></div></div>",
                    name = r.name,
                );
            }
        }

        html.push_str("</div>\n\n");
    }

    html.push_str("</body></html>");
    html
}
