---
description: Run the session-end sign-off ceremony — persist your durable resume, sign off your stewarded docs, and park your heartbeat on the board.
argument-hint: "[role]"
---

You are winding down a work session. Run the **sign-off ceremony** for your role —
the agent whose `coord/<role>.ys` heartbeat you own. If a role is given, use it:
$ARGUMENTS

Do these in order, then report what you persisted. Use the binary directly
(`/mnt/data/archive/prism-target/debug/chrysalis`, or `chrysalis` if installed) and
keep the build channel out of it — this is persistence, not a build.

1. **Durable resume — update `coord/<role>.next`.** Rewrite the "Now / next" so the
   next boot resumes cleanly: what landed this session, the immediate next step, and
   any open threads / asks left for peers. You rebuild `coord/<role>.ys` from this on
   boot, so make it self-sufficient.

2. **Sign off your stewarded docs.** For every doc you made current this session,
   record the signoff on the board:
   `chrysalis coord set <role> stewards='{"docs/your-doc.md":"<today>", …}'`
   Use the real date. A doc you own but did NOT review this session → mark it
   `"assigned (review pending)"`, not a date. (See `docs/doc-stewardship.md`.)

3. **Park your heartbeat.** Leave a clean final state on the board:
   `chrysalis coord set <role> task='PARKED — <one line>' status='<final state; all green?>' build.state=idle`

4. **Hand off open threads.** If you have asks for peers, make sure they sit in your
   `to_<peer>` fields so they see them on their next `pull`.

5. **Confirm.** Read back your `coord/<role>.ys` and `coord/<role>.next`, then tell
   the human exactly what was persisted and what the next boot will resume into.

Rules: never hand-edit `coord/<role>.ys` — always go through `chrysalis coord set`
(the gated codec). Leave all git commits to the human. This is the wind-down dual of
the boot command (`chrysalis coord set <role> task='booting — reading <role>.next'`).
