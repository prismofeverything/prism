//! `Logic` — Boolean operations over gate Signals, plus a T-flip-flop. `and` / `or` /
//! `xor` / `nand` of two gates `a`, `b` (treating `> threshold` as true), and `flip` —
//! a toggle that flips on every rising edge of `a` (i.e. a ÷2 divider). The Schlappi /
//! analog-logic idiom: combine clocks and gates into new rhythms (XOR two clocks for a
//! polyrhythm; flip-flop a clock for half-time). State: the flip-flop bit + `a`'s edge.

use std::any::Any;

use indexmap::IndexMap;
use prism_bigraph::{Process, Schema, Update, Value};

use crate::modulation::{at, cv_in, rising_edge};
use crate::signal::{signal_from_slice, signal_type};

#[derive(Clone, Debug)]
pub struct Logic {
    pub block_size: usize,
    pub threshold: f64,
}

impl Logic {
    pub fn from_config(config: &Value) -> Self {
        Self {
            block_size: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(512) as usize,
            threshold: config.get_field("threshold").and_then(|v| v.as_f64()).unwrap_or(0.5),
        }
    }
}

impl Process for Logic {
    fn inputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("a".to_string(), signal_type()),
            ("b".to_string(), signal_type()),
            ("flip_state".to_string(), Schema::float()),
            ("a_z".to_string(), Schema::float()),
        ])
    }

    fn outputs(&self) -> IndexMap<String, Schema> {
        IndexMap::from([
            ("and".to_string(), signal_type()),
            ("or".to_string(), signal_type()),
            ("xor".to_string(), signal_type()),
            ("nand".to_string(), signal_type()),
            ("flip".to_string(), signal_type()),
            ("flip_state".to_string(), Schema::overwrite(Schema::float())),
            ("a_z".to_string(), Schema::overwrite(Schema::float())),
        ])
    }

    fn interval(&self) -> f64 {
        self.block_size as f64 / 48_000.0
    }

    fn update(&self, state: &Value, _interval: f64) -> Update {
        let a = cv_in(state, "a");
        let b = cv_in(state, "b");
        let mut flip = state.get_field("flip_state").and_then(|v| v.as_f64()).unwrap_or(0.0) != 0.0;
        let mut a_z = state.get_field("a_z").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let n = self.block_size;
        let th = self.threshold;
        let (mut o_and, mut o_or, mut o_xor, mut o_nand, mut o_flip) = (
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
            Vec::with_capacity(n),
        );
        let g = |x: f64| if x > th { 1.0_f32 } else { 0.0 };
        for i in 0..n {
            let (av, bv) = (at(&a, i), at(&b, i));
            let (ga, gb) = (av > th, bv > th);
            o_and.push(g((ga && gb) as i32 as f64));
            o_or.push(g((ga || gb) as i32 as f64));
            o_xor.push(g((ga ^ gb) as i32 as f64));
            o_nand.push(g((!(ga && gb)) as i32 as f64));
            if rising_edge(a_z, av, th) {
                flip = !flip;
            }
            a_z = av;
            o_flip.push(if flip { 1.0 } else { 0.0 });
        }

        Update::value(Value::tree([
            ("and", signal_from_slice(&o_and)),
            ("or", signal_from_slice(&o_or)),
            ("xor", signal_from_slice(&o_xor)),
            ("nand", signal_from_slice(&o_nand)),
            ("flip", signal_from_slice(&o_flip)),
            ("flip_state", Value::float(if flip { 1.0 } else { 0.0 })),
            ("a_z", Value::float(a_z)),
        ]))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
