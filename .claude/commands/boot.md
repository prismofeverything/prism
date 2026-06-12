---
description: Boot into a coordination role — join the board, read your charter (coord/<role>.next) + the roster, and resume where that role left off.
argument-hint: "<role>"
---

You are booting into a role on the prism multi-agent coordination board. Your role:
$ARGUMENTS

(If no role is given above, ask the human which role to boot into before proceeding.)

Boot in, in order. Use the chrysalis binary directly
(`/mnt/data/archive/prism-target/debug/chrysalis`, or `chrysalis` if installed):

1. **Join the board — one command.**
   `chrysalis coord set <role> task='booting — reading <role>.next'`
   This CREATES `coord/<role>.ys` from the skeleton if absent and joins
   `coord/board.ys`. Never hand-edit `coord/<role>.ys` — always go through
   `coord set` (the gated codec).

2. **Read your charter — `coord/<role>.next`.** Your durable resume: the role's
   purview + what it last left off on. Rebuild your sense of the role from it. If it
   does NOT exist, this is a **new role**: read `coord/ROSTER.md` for the purview, ask
   the human for specifics, and write a `coord/<role>.next` to establish it.

3. **Read the shared context.** `coord/ROSTER.md` (roles, protocol, the CALM /
   build-coordination rules) and the board — who else is live (`chrysalis run
   coord/board.ys --time 1`, or raw-read `coord/*.ys`). Note any `to_<role>` asks
   peers left for you.

4. **Orient against the durable record.** Skim `docs/NEXT-SESSION.md` (status) and
   the docs your role stewards (`docs/doc-stewardship.md`).

5. **Resume.** Pick up from the "Now / next" in `coord/<role>.next`, answer any peer
   asks on the board, and tell the human what you booted into and your first move.

This is the dual of `/signoff` (the wind-down). Boot opens the session; signoff closes it.
