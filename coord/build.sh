#!/usr/bin/env bash
# coord/build.sh — the BUILD CHANNEL, automated (ROSTER §Build coordination).
#
# "Using the build channel" = run THIS instead of bare cargo. It removes the friction
# that made the channel a stale record instead of a live signal:
#   1. READS every peer's build state RAW (grep — the channel must NOT depend on the
#      binary it rebuilds) and warns if a peer is mid-build on a crate you share
#      (same crate ⇒ you will block on cargo's target lock; BATCH instead).
#   2. LEASES — sets your build.state=building BEFORE the heavy build, so a downstream
#      agent sees it and batches instead of firing into your lock.
#   3. BUILDS your cargo command.
#   4. BROADCASTS — build.state=green @ green_tick++ on success, or =broken on failure.
#      (green_tick is a MONOTONE watermark: a new tick = a new checkpoint downstream
#      may pull + rebuild against. broken = a global STOP for your downstream.)
#
# Writes go through `chrysalis coord set` (the gated codec — never wedges the board);
# it uses the LAST-GOOD chrysalis binary, which cargo never overwrites on a failed
# build, so the broadcaster works even mid-break.
#
# Usage:  coord/build.sh <agent> -- <cargo cmd...>
#   e.g.  coord/build.sh pkg  -- cargo test -p chrysalis --test suite
#         coord/build.sh core -- cargo test -p prism-bigraph --test suite
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"
AGENT="${1:?usage: coord/build.sh <agent> -- <cargo cmd...>}"; shift
[ "${1:-}" = "--" ] && shift
[ "$#" -gt 0 ] || { echo "coord/build.sh: no build command given" >&2; exit 2; }
CMD=("$@")

# The relocated chrysalis binary (target-dir from .cargo/config.toml; default target/).
TARGET="$(grep -oE 'target-dir *= *"[^"]+"' .cargo/config.toml 2>/dev/null | grep -oE '"[^"]+"' | tr -d '"')"
BIN="${TARGET:-target}/debug/chrysalis"

state_of() {
  grep -A6 'build:' "coord/$1.ys" 2>/dev/null | tr '\n' ' ' \
    | grep -oE "state: '[^']*'" | head -1 | grep -oE "building|broken|green|idle"
}

# --- 1. READ the channel RAW -------------------------------------------------
echo "── build channel ─────────────────────────────"
busy=""
for f in coord/*.ys; do
  a="$(basename "$f" .ys)"; [ "$a" = board ] && continue
  line="$(grep -A6 'build:' "$f" 2>/dev/null | tr '\n' ' ' \
          | grep -oE "state: '[^']*'.*green_tick: [0-9]+" | head -1)"
  [ -n "$line" ] && printf "  %-9s %s\n" "$a" "$line"
  [ "$a" != "$AGENT" ] && [ "$(state_of "$a")" = building ] && busy="$busy $a"
done
echo "──────────────────────────────────────────────"
if [ -n "${busy// }" ]; then
  echo "⚠ BUILDING now:${busy} — if you share a crate you will block on cargo's lock."
  echo "  Consider BATCHING (design / write tests that don't need to run yet) until green."
fi

# --- 2. LEASE ----------------------------------------------------------------
"$BIN" coord set "$AGENT" build.state=building build.cmd="${CMD[*]}" >/dev/null 2>&1 \
  || echo "  (lease write skipped — last-good binary missing at $BIN)"

# --- 3. BUILD ----------------------------------------------------------------
echo "▶ ${CMD[*]}"
"${CMD[@]}"; CODE=$?

# --- 4. BROADCAST ------------------------------------------------------------
if [ "$CODE" -eq 0 ]; then
  TICK="$(grep -A6 'build:' "coord/$AGENT.ys" 2>/dev/null | grep -oE 'green_tick: [0-9]+' | grep -oE '[0-9]+' | head -1)"
  NEXT=$(( ${TICK:-0} + 1 ))
  "$BIN" coord set "$AGENT" build.state=green build.green_tick="$NEXT" >/dev/null 2>&1
  echo "✓ $AGENT GREEN @ green_tick $NEXT — downstream may pull + rebuild."
else
  "$BIN" coord set "$AGENT" build.state=broken >/dev/null 2>&1
  echo "✗ $AGENT BROKEN (exit $CODE) — a STOP for your downstream until fixed."
fi
exit "$CODE"
