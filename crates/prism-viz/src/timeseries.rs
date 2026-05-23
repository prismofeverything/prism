//! Time-series line plots → self-contained SVG (via plotters). Used by the
//! process-contract `TimeSeries::overlay` method. (A future `prism-svg` will
//! represent SVG as place-graph values rather than an opaque plotters string —
//! see the plan.)

use indexmap::IndexMap;

/// Render `series` (each `name → values`, sampled at `times`) as an SVG line
/// chart titled `title`. `force_log` forces a log y-axis; otherwise it is chosen
/// automatically for multi-scale data.
pub fn render_timeseries_svg(
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
