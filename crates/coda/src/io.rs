//! Coda corpus I/O — loaders for public sperm-whale datasets.
//!
//! `LoadDominicaCSV` reads Sharma 2024's `DominicaCodas.csv` (8,718 codas
//! across 18 columns; ICI columns padded with zeros up to 9) and emits a
//! `list[Coda]` where each Coda carries `{coda_id, n_clicks, duration, icis,
//! coda_type, clan, unit, idn}`. `nClicks - 1` ICIs per coda are valid; the
//! loader trims trailing zero-padded entries by `nClicks`.

use std::any::Any;
use std::fs::File;
use std::io::{BufRead, BufReader};

use indexmap::IndexMap;
use prism_bigraph::{ProcessNode, ProcessRegistry, Schema, Step, Update, Value};

#[derive(Debug)]
pub struct LoadDominicaCSV {
    /// Absolute or relative path to the CSV.
    path: String,
    /// Optional cap on rows read. `None` ⇒ load all (~8,718).
    limit: Option<usize>,
}

impl LoadDominicaCSV {
    pub fn from_config(config: &Value) -> Self {
        let path = config
            .get_field("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let limit = config
            .get_field("limit")
            .and_then(|v| v.as_f64())
            .map(|f| f as usize);
        Self { path, limit }
    }

    fn parse(&self) -> Vec<Value> {
        let Ok(file) = File::open(&self.path) else {
            return Vec::new();
        };
        let reader = BufReader::new(file);
        let mut codas: Vec<Value> = Vec::new();
        // CSV columns:
        // codaNUM2018, Date, nClicks, Duration, ICI1..ICI9, CodaType, Clan, Unit, UnitNum, IDN
        // [0]          [1]    [2]      [3]       [4..13]    [13]      [14]  [15]  [16]     [17]
        for (idx, line) in reader.lines().enumerate() {
            let Ok(line) = line else { continue };
            if idx == 0 {
                continue; // skip BOM-prefixed header
            }
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() < 18 {
                continue;
            }
            let n_clicks_f = parts[2].parse::<f64>().unwrap_or(0.0);
            let n = n_clicks_f as usize;
            let n_icis = n.saturating_sub(1).min(9);
            let icis: Vec<Value> = (0..n_icis)
                .filter_map(|j| parts.get(4 + j).and_then(|s| s.parse::<f64>().ok()))
                .map(Value::float)
                .collect();
            let coda = Value::tree([
                ("_type", Value::from("Coda")),
                ("coda_id", Value::from(parts[0])),
                ("n_clicks", Value::float(n_clicks_f)),
                ("duration", Value::float(parts[3].parse().unwrap_or(0.0))),
                ("icis", Value::List(icis)),
                ("coda_type", Value::from(parts[13])),
                ("clan", Value::from(parts[14])),
                ("unit", Value::from(parts[15])),
                ("idn", Value::float(parts[17].parse().unwrap_or(0.0))),
            ]);
            codas.push(coda);
            if let Some(lim) = self.limit {
                if codas.len() >= lim {
                    break;
                }
            }
        }
        codas
    }
}

impl Step for LoadDominicaCSV {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::new()
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("codas".to_string(), Schema::Any),
            ("n".to_string(), Schema::float()),
        ])
    }

    fn update(&self, _state: &Value) -> Update {
        let codas = self.parse();
        let n = codas.len() as f64;
        Update::value(Value::tree([
            ("codas", Value::List(codas)),
            ("n", Value::float(n)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Register coda's I/O processes into a [`ProcessRegistry`].
pub fn register_processes(reg: &mut ProcessRegistry) {
    reg.register("LoadDominicaCSV", |config| {
        ProcessNode::Step(Box::new(LoadDominicaCSV::from_config(&config)))
    });
}
