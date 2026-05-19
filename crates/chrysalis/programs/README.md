# Chrysalis programs (`.ys`)

Source files in the chrysalis surface syntax. Extension: `.ys`
(chrYSalis).

## Current state

The chrysalis parser is **not yet implemented** (task #12). These
files are the **specification** of what the surface syntax should
look like — the canonical reference for the language design. The
runtime today consumes hand-built ASTs from
[`crates/chrysalis/src/fixtures/`](../src/fixtures/) that should
correspond 1:1 with the surface forms here.

When the parser lands, these `.ys` files will be the actual inputs:
the test suite will load each `.ys`, parse to AST, and verify
behavioural parity with the hand-built fixture.

## Files

| `.ys` file | AST fixture | Status |
|---|---|---|
| [`grow_divide.ys`](grow_divide.ys) | [`fixtures/grow_divide.rs`](../src/fixtures/grow_divide.rs) | ✅ end-to-end runs (tier-1 benchmark #1) |
| `mapk.ys` *(planned)* | not yet | tier-1 benchmark #2 |
| `mr_closure.ys` *(planned)* | not yet | tier-1 benchmark #3 (static M/R) |
| `mr_evolving.ys` *(planned)* | not yet | tier-2 benchmark (evolving M/R closures) |

## Syntax overview

See [`docs/chrysalis-design.md`](../../../docs/chrysalis-design.md)
for the full design. The kernel is small:

- `K[args](body)` — term construction
- `a | b` — parallel composition (symmetric monoidal tensor)
- `~{port: target}` / `->{port: target}` — input / output port bindings
- `?name`, `?name : Sort` — pattern variables
- `~name` — link variable
- `!` — unbound port
- `=>` — reaction redex/reactum separator
- lowercase **definers** (`process`, `step`, `composite`, `reaction`,
  `pattern`) introduce entities; capitalised **controls** (`Cell`,
  `Grow`, `MEK`) are the constructed values.

## Why `.ys`

Short. Memorable. The "Y" stands out — most other extensions don't
use it.
