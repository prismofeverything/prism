//! Generate an SVG comparison plot of Rust vs Python grow-divide benchmarks.

fn main() {
    // Rust data (release build, composite agents, rate=0.1)
    // After O(n²)→O(n) fix: in-place _add/_remove + dedup discover_processes
    let rust_data = [
        (5.0, 1, 0.5),
        (10.0, 2, 0.5),
        (20.0, 4, 0.5),
        (30.0, 16, 1.0),
        (40.0, 32, 2.0),
        (50.0, 64, 5.0),
        (55.0, 128, 11.0),
        (60.0, 256, 19.0),
        (65.0, 256, 22.0),
        (70.0, 512, 45.0),
        (75.0, 1024, 104.0),
    ];

    // Python data (process-bigraph composite, rate=0.1)
    let python_data = [
        (5.0, 1, 11.0),
        (10.0, 2, 31.0),
        (20.0, 4, 90.0),
        (30.0, 8, 220.0),
        (40.0, 32, 657.0),
        (50.0, 64, 1638.0),
        (55.0, 64, 2107.0),
        (60.0, 128, 3768.0),
        (65.0, 256, 6477.0),
        (70.0, 256, 8637.0),
        (75.0, 512, 15032.0),
    ];

    let w = 600;
    let h = 400;
    let margin = 60;
    let pw = w - 2 * margin;
    let ph = h - 2 * margin;

    // Log scale for time
    let max_time = 40000.0_f64;
    let min_time = 0.1_f64;
    let log_min = min_time.log10();
    let log_max = max_time.log10();

    let max_dur = 65.0_f64;

    let x_of = |dur: f64| -> i32 {
        margin as i32 + ((dur / max_dur) * pw as f64) as i32
    };
    let y_of = |ms: f64| -> i32 {
        let log_val = ms.max(min_time).log10();
        let frac = (log_val - log_min) / (log_max - log_min);
        (margin + ph) as i32 - (frac * ph as f64) as i32
    };

    let mut svg = format!(
        "<svg width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" xmlns=\"http://www.w3.org/2000/svg\">\
         <rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>\
         <text x=\"{}\" y=\"20\" text-anchor=\"middle\" font-size=\"14\" font-family=\"sans-serif\" font-weight=\"bold\">\
         Grow-Divide Benchmark: Rust vs Python</text>",
        w / 2
    );

    // Grid lines for log scale
    for &ms in &[0.1, 1.0, 10.0, 100.0, 1000.0, 10000.0] {
        let y = y_of(ms);
        svg.push_str(&format!(
            "<line x1=\"{margin}\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\" stroke=\"#eee\" stroke-width=\"1\"/>",
            margin + pw
        ));
        let label = if ms >= 1000.0 { format!("{}s", ms as i32 / 1000) }
                    else if ms >= 1.0 { format!("{}ms", ms as i32) }
                    else { format!("0.1ms") };
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" text-anchor=\"end\" font-size=\"10\" font-family=\"sans-serif\" fill=\"#666\">{label}</text>",
            margin - 5, y + 3
        ));
    }

    // X axis labels
    for &dur in &[0, 10, 20, 30, 40, 50, 60] {
        let x = x_of(dur as f64);
        svg.push_str(&format!(
            "<text x=\"{x}\" y=\"{}\" text-anchor=\"middle\" font-size=\"10\" font-family=\"sans-serif\" fill=\"#666\">{dur}</text>",
            h - 10
        ));
    }

    // Axes
    svg.push_str(&format!(
        "<line x1=\"{margin}\" y1=\"{margin}\" x2=\"{margin}\" y2=\"{}\" stroke=\"#333\" stroke-width=\"1\"/>",
        margin + ph
    ));
    svg.push_str(&format!(
        "<line x1=\"{margin}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#333\" stroke-width=\"1\"/>",
        margin + ph, margin + pw, margin + ph
    ));

    // Axis labels
    svg.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" font-family=\"sans-serif\">Simulation Duration (s)</text>",
        w / 2, h - 2
    ));
    svg.push_str(&format!(
        "<text x=\"12\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" font-family=\"sans-serif\" \
         transform=\"rotate(-90 12 {})\">Wall Time (log scale)</text>",
        h / 2, h / 2
    ));

    // Plot Rust line
    let rust_points: String = rust_data.iter()
        .map(|&(dur, _, ms)| format!("{},{}", x_of(dur), y_of(ms)))
        .collect::<Vec<_>>()
        .join(" ");
    svg.push_str(&format!(
        "<polyline points=\"{rust_points}\" fill=\"none\" stroke=\"#2ca02c\" stroke-width=\"2.5\"/>"
    ));
    for &(dur, agents, ms) in &rust_data {
        let x = x_of(dur);
        let y = y_of(ms);
        svg.push_str(&format!(
            "<circle cx=\"{x}\" cy=\"{y}\" r=\"4\" fill=\"#2ca02c\"/>"
        ));
        if agents > 1 {
            svg.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"8\" font-family=\"sans-serif\" fill=\"#2ca02c\">{agents}</text>",
                x + 6, y - 4
            ));
        }
    }

    // Plot Python line
    let python_points: String = python_data.iter()
        .map(|&(dur, _, ms)| format!("{},{}", x_of(dur), y_of(ms)))
        .collect::<Vec<_>>()
        .join(" ");
    svg.push_str(&format!(
        "<polyline points=\"{python_points}\" fill=\"none\" stroke=\"#d62728\" stroke-width=\"2.5\"/>"
    ));
    for &(dur, agents, ms) in &python_data {
        let x = x_of(dur);
        let y = y_of(ms);
        svg.push_str(&format!(
            "<circle cx=\"{x}\" cy=\"{y}\" r=\"4\" fill=\"#d62728\"/>"
        ));
        if agents > 1 {
            svg.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"8\" font-family=\"sans-serif\" fill=\"#d62728\">{agents}</text>",
                x + 6, y - 4
            ));
        }
    }

    // Legend
    let lx = margin + pw - 150;
    let ly = margin + 20;
    svg.push_str(&format!(
        "<rect x=\"{lx}\" y=\"{ly}\" width=\"145\" height=\"50\" fill=\"white\" stroke=\"#ccc\" rx=\"4\"/>"
    ));
    svg.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#2ca02c\" stroke-width=\"2.5\"/>",
        lx + 10, ly + 18, lx + 30, ly + 18
    ));
    svg.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"11\" font-family=\"sans-serif\">Rust (release)</text>",
        lx + 35, ly + 22
    ));
    svg.push_str(&format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#d62728\" stroke-width=\"2.5\"/>",
        lx + 10, ly + 38, lx + 30, ly + 38
    ));
    svg.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"11\" font-family=\"sans-serif\">Python</text>",
        lx + 35, ly + 42
    ));

    // Annotation: numbers are agent counts
    svg.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"9\" font-family=\"sans-serif\" fill=\"#999\" font-style=\"italic\">\
         Numbers show agent count at each point</text>",
        margin + 5, margin + ph - 5
    ));

    svg.push_str("</svg>");

    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/report/grow_divide_benchmark.svg");
    std::fs::write(out, &svg).unwrap();
    println!("Wrote {out}");
}
