//! coda — whale (and beyond) communication research.
//!
//! A `.ys` project with `package coda` in its `project.ys` runs against this
//! crate via chrysalis's codegen path; the [`prelude`] module exposes the
//! `registry` / `methods` / `modules` handshake the codegen expects.
//!
//! ## Long-horizon goal
//!
//! A two-way translator between human language and sperm whale vocalizations,
//! validated by a successful real conversation with a whale.
//!
//! ## Method spine
//!
//! Every research question is a first-class artifact (a `Question`) with
//! runnable computational *operationalizations* — processes that consume the
//! corpus + a candidate hypothesis and emit a *Measurement* that *can fail* to
//! discriminate between hypotheses. The question's *evidence state* only
//! advances when an operationalization fires on real data and produces a
//! discriminating measurement. Survey work generates questions; it does not
//! pre-empt them. The literature is treated as a set of competing hypotheses,
//! not as ground truth.

pub mod io;
pub mod prelude;
pub mod stats;
