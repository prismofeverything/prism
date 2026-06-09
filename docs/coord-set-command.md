# `chrysalis coord set` — update a heartbeat from DATA (the remain-parsable invariant)

> **Design note** (author: `simplify`; implementers: `lang` (CLI + unparser) with
> `mesh`/`unify` (coord stewards); algebra from `core`). Proposed in-session after the
> agent board wedged on hand-edited `.ys` heartbeats more than once. The cargo-test
> backstop already exists — `crates/chrysalis/tests/coord_heartbeats_parse.rs` (every
> `coord/*.ys` parses + the board renders). This note specifies the *correct-by-
> construction* fix the guard checks for.

## The problem

The agent mesh board (`coord/board.ys`) merges every peer's heartbeat
(`coord/<peer>.ys`) through one `map[any] mesh` link. So a **single unparsable
heartbeat takes the whole board render down for every peer** — and the heartbeats are
hand-edited `.ys`, whose string fields carry metacharacters: `'` is the string
delimiter (a literal apostrophe must be doubled `''`), `{...}` is interpolation
(braces in prose evaluate). Hand-authoring that escaping is LLM-/human-fallible, the
blast radius is everyone, and it has bitten us repeatedly (synth/unify/simplify
apostrophes; lang's braces).

## The principle — serialize from data, don't "filter" syntax

This is a **serialization-escaping** bug, not a "validate then clean" problem. You
**cannot** safely *filter* a stray metacharacter after the fact — a `{` is ambiguous
(interpolation the author meant, or a literal?), so any auto-fix guesses intent. The
fix is to **never hand-serialize**:

> The agent supplies the heartbeat as **data** (raw strings / numbers / lists); a tool
> serializes it to `.ys` through the **canonical unparser**, which escapes correctly by
> construction. "Remain parsable" then holds because the unparser is the parser's tested
> inverse — `parse(unparse(x)) == x` (already pinned by `unparse_roundtrip` /
> `ys_files_roundtrip`).

This is the homoiconic core (`quote`/`reify`) applied to our own coordination
substrate, and the same "discipline → mechanism" move as the one-door guards.

## What already exists (reuse, don't clone)

- `chrysalis check <file>` validates a heartbeat parses; `chrysalis run coord/board.ys`
  validates the merge.
- the unparser (`chrysalis format` / `cmd_format`) **already** escapes braces +
  doubled-apostrophes correctly (core's fix) and is round-trip-tested.
- `chrysalis coord push <peer> '<json>'` **already** takes the heartbeat as JSON data
  for the live socket (`coord serve`/`push`/`pull`).

The only unsafe path is the one we actually use day-to-day: **hand-editing the durable
`coord/<peer>.ys` file.** The work is to give the *file* path the safety the *socket*
path already has — not to build new machinery.

## The command

```
chrysalis coord set <peer> <field=value>...      # ergonomic: set fields, auto-bump tick
chrysalis coord set <peer> --json '<patch>'      # power form: merge a JSON patch (mirrors `coord push`)
```

Examples:
```
chrysalis coord set simplify status='GREEN — full suite 225/0' note='joined the board'
chrysalis coord set simplify --json '{"build":{"state":"green","green_tick":8}}'
```

Semantics: **read → Value → apply → unparse → round-trip gate → write.**

1. **read** `coord/<peer>.ys` → `Value` (`parse_program` → the `def <peer> = {...}` map;
   a missing file starts from `{}`).
2. **apply** the field update as a schema-algebra `apply` (overwrite the named slots) —
   `prism_schema::algebra::apply`, **not** an ad-hoc `IndexMap::insert` (stay inside the
   closed algebra). `value` strings arrive as raw argv data, so they are escaped by the
   serializer, never by the author.
3. **auto-bump `tick`** (read current, `+1`) — the monotone heartbeat counter; one fewer
   thing to hand-edit (and forget).
4. **unparse** the updated `Value` via the canonical formatter (the `cmd_format`
   machinery) — correct escaping by construction.
5. **round-trip gate (the invariant, enforced):** `parse_program` the unparsed text; if
   it fails, **abort the write** and report. The file only ever transitions good→good
   (monotone, like the build green-watermark). This is the "remain-parsable invariant of
   some kind," realized at the write.
6. **write** `coord/<peer>.ys`.

Thin-layer (`feedback_chrysalis_thin_layer`): steps 2/4 **call** prism's algebra +
chrysalis's own unparser; clone nothing. Closed-algebra (`feedback_schema_algebra`): the
update is `apply`, a named op, not munging.

## The invariant + its guard

`crates/chrysalis/tests/coord_heartbeats_parse.rs` pins **remain-parsable** in
`cargo test`: every `coord/*.ys` parses **and** the board renders end-to-end. The
command (step 5) makes that *correct-by-construction*; the guard is the **backstop** for
any hand-edit that bypasses the command, and the net that already protects us *before*
the command lands. Belt and suspenders, deliberately.

## Suggested slices

- **Slice 1 — the round-trip-gated write** (`coord set <peer> --json`): the safety. Read
  → apply JSON patch → unparse → gate → write. Smallest correct-by-construction core.
- **Slice 2 — ergonomic `field=value` setters + auto tick-bump**: the daily-use surface.
- **Slice 3 — unify the file path with `coord push`/`pull`**: one update path for both
  the durable file and the live socket board (the file is the at-rest form of the same
  data the socket gossips). Removes the file/socket split — a `simplify` convergence.

## References

- `coord/ROSTER.md` § CALM regimes (the board is a non-monotone shared invariant —
  keep-it-green / coordinate-first) · § Build coordination (the monotone green-watermark
  pattern this mirrors at the write).
- `docs/homoiconic-unification.md` (`quote`/`reify`; the `.ys`-as-data thesis the coord
  board already dogfoods).
- `feedback_chrysalis_thin_layer`, `feedback_schema_algebra` (the disciplines the
  implementation must hold).
