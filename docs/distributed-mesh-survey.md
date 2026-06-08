# Distributed P2P/Mesh Computing for Process Bigraphs on a Tailscale Mesh — A Cited Survey

*Background research, 2026-06-08 (deep-research agent). Grounds the native-mesh
direction (`docs/NEXT-SESSION.md` #62, memory `mesh_as_protocol`). Scope: building
blocks for (1) coordinator-free shared **links**, (2) graph-rewrite **reactions**
across machines, (3) neighbour↔neighbour **streaming composites**. Confidence flags:
**[PROVEN]** / **[CONTESTED]** / **[SPARSE]**.*

> Method note: in this run the deep-research skill + WebFetch were permission-denied,
> so the agent surveyed via WebSearch only — ~20 angled queries, each load-bearing
> claim triangulated across multiple independent sources. Precise figures (e.g.
> Automerge 5 min→600 ms) are corroborated across secondary summaries; verify against
> the primary PDF before quoting in print.

## 0. The two results that govern everything

**CALM theorem — *the* deep result [PROVEN].** Hellerstein & Alvaro, *Keeping CALM:
When Distributed Consistency Is Easy* (CACM 2020 / arXiv:1901.01930); conjectured PODS
2010, formalized by Ameloot, Neven & Van den Bussche (PODS 2011 / JACM 2013). **A
problem has a consistent, coordination-free distributed implementation iff it is
monotonic.** Monotonic = output only grows as input grows; new facts never retract
earlier conclusions, so all delivery orders converge. Sharp consequence: anything
requiring negation/aggregation/"is this the final answer?" is non-monotonic and
**provably needs coordination**. Canonical example: *garbage collection itself*
("garbage is identified by the non-existence of a path … inherently non-monotonic").
Deletion, counting-to-threshold, "no more writers exist" — non-monotonic. Escape
hatch: keep the *fast path* monotonic, push non-monotonic reclamation (tombstone GC)
**off the critical path** — coordinate lazily, in the background, rolling.

**CAP / PACELC [PROVEN].** CAP (Brewer; Gilbert–Lynch proof): under Partition, choose
Consistency or Availability. PACELC (Abadi 2010/2012): even with no partition (Else),
trade Latency vs Consistency. A leaderless symmetric mesh is a deliberate **PA/EL**
choice — available under partition, low-latency normally, at the price of strong
consistency. Correct default for prism **only for monotonic state**; a non-monotonic
link merge has bought coordination latency whether you admit it or not.

## 1. Shared links with no coordinator — exactly what is safe

**CRDTs are the proven building block [PROVEN].** Shapiro, Preguiça, Baquero &
Zawirski, *A Comprehensive Study of Convergent and Commutative Replicated Data Types*
(INRIA RR-7506, 2011). Two families:
- **State-based (CvRDT):** state is a **join-semilattice**; `merge` is the LUB.
  Converges iff merge is **commutative, associative, idempotent** (X⊔X=X, X⊔Y=Y⊔X,
  (X⊔Y)⊔Z=X⊔(Y⊔Z)) *and* mutators are **inflationary** (X ⊑ m(X)). Theorem: under
  eventual delivery, any state with the monotonic-semilattice property is **Strongly
  Eventually Consistent**. This is the link-merge contract, verbatim.
- **Op-based (CmRDT):** broadcast ops; converges iff concurrent ops **commute** AND
  the transport gives **reliable causal-order, exactly-once delivery** (Baquero/
  Almeida/Shoker 2014). The causal/exactly-once requirement is a real obligation on
  the network layer — see §2.
- **Delta-state (δ-CRDT) — ADOPT [PROVEN].** Almeida, Shoker & Baquero, *Delta State
  Replicated Data Types* (JPDC 2018 / arXiv:1603.01529; arXiv:1410.2803). δ-mutators
  return a small delta joined into local+remote state — **state-based safety
  (idempotent, no exactly-once) at op-based bandwidth.** The right shape for prism: a
  link update is a δ(S), joined on receipt; matches the existing "an update IS a
  delta" model + the `serialize_update`/`realize_update` boundary codec.

**Map the link schema's merge onto the lattice — and the cliff edges:**

| Link merge intent | Coordination-free? | CRDT |
|---|---|---|
| Additive pool **with per-source accounting** | ✅ | G/PN-Counter (per-replica vectors) |
| Plain additive scalar (`x += n`, no per-source slot) | ❌ **double-counts on retry** — not idempotent | must become a counter |
| Key-union / `map[Reaction] ∪` (grow-only) | ✅ | G-Set / OR-Map (grow-only = easy case) |
| Set with **removal** | ⚠️ unique-tag bookkeeping | OR-Set (tombstones) |
| Last-writer-wins register | ⚠️ **only with a clock** | LWW-Register + Lamport; loses concurrent writes |
| Sequence / ordered children | ⚠️ | RGA / Fugue / Eg-walker |

Two traps the literature is unanimous on:
- **Additive without per-source accounting is the classic footgun:** a bare increment
  merged twice (anti-entropy, retransmit) is not idempotent → over-counts. The
  PN-Counter exists precisely to fix this via a per-replica increment/decrement
  vector. (prism: `crdt_laws.rs` pins this — bare `Float` additive ❌, per-source map
  ✅.)
- **LWW needs a clock and silently discards the loser.** Wall-clock → skew-prone;
  Lamport/version-vectors → deterministic but a *policy that throws away one of two
  concurrent updates*. Fine for cosmetic fields, dangerous where both writes matter.

**The hard CRDT limits [PROVEN]:**
- **Tombstones & unbounded metadata.** OR-Set/2P-Set keep tombstones forever; GC of
  them "would require all replicas to remove the element at the same time → global
  coordination" — i.e. exactly the non-monotonic GC CALM says needs coordination.
  Causal stability is the tool: discard metadata only once causally stable (every
  replica has seen it), which needs version vectors. **Budget metadata GC from day
  one; it's a known way these systems die.**
- **CRDTs cannot enforce a global invariant** (no double-spend, "unique winner",
  capacity bound) — non-monotonic → coordination. CALM proves you can't CRDT around it.

**Causality machinery, when forced past plain merge [PROVEN]:** Lamport clocks; vector
clocks / version vectors (O(replicas)); **Dotted Version Vectors** (Preguiça/Baquero,
arXiv:1011.5808; Gonçalves et al. 2012) — separate the *present* dot from the *causal
past*, size linear in **servers not clients** (the Riak sibling-explosion fix). For a
bounded mesh of engines, plain version vectors usually suffice; DVVs if ingestion fans
in through few nodes.

**Consensus is last resort [PROVEN].** Paxos (Lamport) / Raft (Ongaro–Ousterhout):
linearizable agreement, but quorum + leader + unavailable under partition (CAP). Use
**only** for non-monotonic decisions — membership, tombstone-GC barriers, "exactly
one" — never the hot path.

## 2. The mesh: what Tailscale gives, and what it does not

**Tailscale/WireGuard [PROVEN, bounded].** You get a flat, authenticated, E2E-encrypted
**L3** mesh: stable per-engine IP, mutual auth, NAT traversal (DISCO + STUN-style hole
punching), **DERP** ciphertext-only relays when direct UDP fails (symmetric/CGNAT). It
does **NOT** solve: **message reliability/ordering** (it's an L3 tunnel, not reliable
causal broadcast — which op-based CRDTs demand), **membership/liveness**, **partition
detection**, app-level convergence. DERP is TCP, not perf-optimized — don't assume
uniform latency.

**Membership/failure detection — adopt [PROVEN]:** **SWIM** (Das, Gupta & Motivala,
IPDPS 2002): randomized direct+indirect probing → constant per-node load, log-time
infection dissemination, **suspicion mechanism** to cut false positives. HashiCorp
**Lifeguard** hardens it. Your "who are my neighbours / who died" layer.

**Dissemination — adopt [PROVEN]:** **Epidemic/gossip** (Demers et al., PODC 1987):
**anti-entropy** (pairwise full reconcile — reliable, slow, the convergence backstop)
+ **rumor-mongering** (fast, probabilistic). The natural transport for δ-CRDTs.

**DHTs (Kademlia, Chord) [PROVEN] — but likely NOT needed.** Kademlia (Maymounkov–
Mazières 2002; XOR metric, O(log n)) solves *global lookup at planet scale*. On a small
authenticated tailnet you already have addresses + full membership → DHT is
over-engineering unless you outgrow one tailnet. libp2p packages GossipSub + Kademlia
+ transports if you later need them.

## 3. Computation over the mesh — reactions and streaming composites

**The warning to internalize [PROVEN, foundational]:** Waldo, Wyant, Wollrath &
Kendall, *A Note on Distributed Computing* (Sun Labs 1994). Treating remote calls as
local **fundamentally fails** — local vs remote differ irreducibly in **latency,
memory access, concurrency, partial failure**. Companion: the **8 Fallacies of
Distributed Computing** (Deutsch/Gosling). **Do not** expose a remote reaction as a
transparent local call; make remoteness/async/failure first-class. prism's "ship the
reaction as data, fire it where the state is" is the *correct* instinct — it's **Remote
Evaluation** (Fuggetta–Picco–Vigna, *Understanding Code Mobility*, IEEE TSE 1998) — keep
it explicit.

**Three models for redex→reactum across machines:**
- **Object-capabilities / CapTP — strongest fit for "send the rewrite to the state"
  [PROVEN, niche].** The E lineage → **Spritely Goblins** + **OCapN/CapTP**, **Cap'n
  Proto RPC**. Killer feature: **promise pipelining** — message the *result* of a
  prior call before it resolves, collapsing multi-hop round trips into one. For a
  reaction walking several composites across hosts, that's the round-trip collapse you
  want. Capabilities = **unforgeable refs** to remote composites/ports (a natural link
  endpoint) with auth built in. **Pitfall [PROVEN]:** CapTP distributed GC is
  **ACYCLIC-ONLY** ("full cycle-collecting DGC needs GC hooks we don't have"), so live
  distributed **cycles leak** (erights.org "dagc"; Shapiro et al. SSP-chains, PODC
  1992). Bigraph link graphs are *full of cycles* → **don't rely on auto-DGC** for
  cross-host link/port refs; use an explicit lease/epoch/causal-stability sweep (the
  non-monotone, off-hot-path pattern). A genuine named hard problem.
- **Actors — proven, weaker location-transparency.** Erlang/OTP: **let-it-crash +
  supervision** is the gold standard for partial failure. But distributed Erlang's
  `net_kernel` is famously **split-brain-prone** (remedy: quorum/3+ nodes = consensus
  for membership). **Orleans virtual actors** ("grains", Bernstein et al., MSR): virtual
  actor-space + directory (identity→location), auto-activation/placement — elegant, but
  the directory is a *coordination dependency*, leans toward a managed cluster, not a
  flat leaderless mesh. Actors model "a composite is an addressable message-processing
  process" well; "a single rewrite atomically spanning composites on different hosts"
  **poorly** (a distributed transaction).
- **"Ship the rule" (what prism does) — keep it, frame as mobile code [PROVEN].** Send
  the redex→reactum to the host of the matched subgraph (Remote Evaluation). Sidesteps
  the worst RPC fallacies *as long as* its effect on shared link state goes through the
  **CRDT merge** (reorder/dup-safe), not an imperative remote mutation. **Catch:** a
  reaction whose redex spans composites on two machines is local to neither — a genuine
  distributed atomic action → coordination (2PC/consensus) OR relaxation (fire
  optimistically, converge the link via CRDT, tolerate transient cross-host
  inconsistency). Decide per-reaction; CALM tells you which can be coordination-free.

**Recommendation for (2)/(3):** composites as **actors** (mailbox, supervision,
let-it-crash) for the streaming/neighbour plane; **capability refs** for stable
cross-host link endpoints + promise-pipelined dispatch; **single-host** reactions
atomic/local; **cross-host** reactions fire optimistically + converge the link via its
δ-CRDT merge, consensus only for non-monotone link schemas; manage cross-host ref
lifecycle **explicitly** (no trust in acyclic DGC).

## 4. Real-world case studies — what shipped, what bit

- **Yjs / Automerge [PROVEN].** CRDT collaborative editing works at scale *now*, after
  a hard perf fight: Kleppmann's **columnar/run-length binary encoding** → docs ~1.5–2×
  content size; Automerge ~5 min & 2 s keystroke stalls → ~600 ms for a 260k-keystroke
  trace (josephg "CRDTs go brrr"; Ink&Switch). Lessons: (a) naïve CRDT memory/CPU is
  brutal; interning + run-length + internal-list-not-tree are mandatory; (b) sequence
  interleaving needed new algorithms (Fugue, Eg-walker). For prism: a link *value* is
  small, but **link metadata (version vectors, tombstones) is where blow-up lives** —
  same compaction discipline.
- **Figma — the honest counter-example [PROVEN, important].** Evan Wallace: Figma
  **deliberately rejected** pure CRDTs and OT for **server-authoritative property-level
  LWW** + fractional indexing, because "CRDTs are designed for decentralized systems
  with no central authority… since we are centralized we can remove this overhead."
  Lesson: **if you can tolerate one authority, you get a simpler, leaner system.**
  prism's leaderless choice is a real cost; pay it where decentralization is required
  (it is, for a flat mesh), and *consider per-link homing* (a "home" owner = a mini
  authority) for links whose merge is awkward.
- **Secure Scuttlebutt [PROVEN, offline-first].** Single-writer **append-only signed
  logs** + social gossip (Tarr; Kermarrec–Lavoie–Tarr, DICG 2020). Wins: superb
  offline-first, no coordinator, sybil resistance via transitive interest. Bit them:
  single-writer logs can't be edited/pruned (identity = an ever-growing immutable
  chain), partial replication awkward, key=device painful. Pattern: per-engine
  append-only op-logs are a clean monotonic substrate for op-based link updates; pruning
  = the same tombstone/causal-stability issue.
- **Matrix federation [PROVEN, cautionary].** **State Resolution v2** — "the only system
  implementing access control over an eventually-consistent partial order without
  consensus" — *and* a cautionary tale: v1 had **state-reset** bugs (revert room state);
  the event-DAG + auth-DAG machinery is subtle and repeatedly mis-stepped (arXiv:
  1910.06295). Lesson: conflict resolution over a partial order without consensus is
  *possible* but a minefield — **specify it as a tested algebra** (prism's discipline).
- **Croquet / Multisynq [PROVEN, different point].** **Deterministic replicated
  computation:** every peer runs a bit-identical VM; a *stateless* "reflector" only
  timestamps/orders external events; identical inputs → identical state → almost no
  state transmitted (only events). If prism reactions are deterministic, you could ship
  *events* and replay rather than ship *state* — but needs a total input order (the
  reflector = a tiny sequencer = a coordination point) + strict determinism. Hold as an
  alternative for tightly-coupled clusters.
- **IPFS/libp2p [PROVEN]:** content-addressing + Kademlia + Bitswap; great for immutable
  blocks, **not** mutable convergent state (IPNS weak). **Merkle-CRDTs** (Sanjuán et
  al., arXiv:2004.00107) bridge it. **Holochain [CONTESTED/SPARSE]:** agent-centric
  source-chains + validating DHT, no global consensus — philosophically close to
  prism's per-engine model, but maturity/validation guarantees debated; inspiration,
  not a dependency. **Blockchains [PROVEN wrong-hammer]:** global Byzantine consensus is
  the most expensive coordination; on an *authenticated* tailnet you're **not** Byzantine
  → BFT/Nakamoto is the wrong tool.

## 5. The symmetric ↔ authoritative axis

| | **Leaderless / symmetric** (Dynamo, Cassandra, Riak, SSB, prism) | **Leader-based / authoritative** (Raft, Spanner, Figma, Orleans-dir) |
|---|---|---|
| Availability under partition | High (PA) | Lower (leader/quorum side) |
| Latency (normal) | Low, local (EL) | Often a leader round-trip |
| Consistency | Eventual/causal; convergence is the app's job | Linearizable/strong "for free" |
| Complexity | In **client/merge logic + metadata GC** | In **the consensus protocol** |
| Invariants / "exactly one" | **Cannot** without bolt-on coordination (CALM) | Natural |
| Byzantine | Out of scope on auth'd mesh; 3f+1 if adversarial | Same |

Not a single global choice — the mature pattern is **per-datum**: most links
symmetric/CRDT; a *minority* of non-monotonic links "homed" on one owner or guarded by
a small consensus group. Tailnet membership is authenticated → **Byzantine tolerance is
not your problem; don't pay for it.**

## 6. Recommendation for bigraph-on-Tailscale

1. **Links = δ-state CRDTs whose merge is the link *schema's* join.** Enforce, as
   executable law in the schema algebra, that every link merge is **commutative +
   associative + idempotent + inflationary** (a semilattice). The algebra **rejects** a
   schema whose merge isn't (bare additive → must be PN-Counter; LWW → must carry a
   clock). The CRDT correctness contract as the closure invariant. (*Started:
   `prism-schema/tests/crdt_laws.rs`.*)
2. **Classify each link by CALM.** Monotonic merges run coordinator-free; non-monotonic
   (removal/tombstone-GC, "unique", capacity) get a homed owner or a lazy,
   causally-stable rolling sweep — *never* the firing path.
3. **Transport:** SWIM membership + gossip/anti-entropy for δ dissemination, directly
   over Tailscale L3. Reliable-causal-broadcast shim **only if** you keep op-based (not
   delta-state) links. Skip DHTs until you outgrow one tailnet.
4. **Reactions:** keep "ship the rule" (Remote Evaluation), explicit + async (Waldo /
   8 fallacies). Single-host atomic; cross-host fire optimistically + converge via the
   link's CRDT; consensus strictly for non-monotone effects. Consider capabilities +
   promise pipelining (Goblins/CapTP, Cap'n Proto) for cross-host refs + round-trip
   collapse.
5. **Lifecycle:** do **not** rely on automatic distributed GC — acyclic-only, bigraph
   links cycle. Reclaim cross-host refs with an explicit epoch/lease/causal-stability
   scheme. Budget tombstone GC + metadata compaction from day one.
6. **Specify conflict resolution as a tested algebra, not ad hoc** (Matrix lesson) —
   already prism's discipline.

### Open / hard problems (honest) [SPARSE / frontier]
- **Topology-rewriting over distributed state** — reactions that restructure the
  place/link graph across hosts *atomically*: no off-the-shelf convergent graph-rewrite
  under concurrency. **Sparse.**
- **Schema-defined merge that *evolves*** (the AlChemy direction: a `map[Reaction]`
  whose contents are reactions that change merge behaviour) — meta-circular convergence
  is **unstudied**.
- **Distributed cyclic GC** for live capability/link graphs — known-hard, **open**.
- **Coordination-free *with invariants*** — CALM proves the wall; "almost-monotone"
  escapes (escrow/reservations, bounded counters) are bespoke per-invariant.
  **Contested/active.**
- **Causal stability & metadata GC at scale** without a global barrier — partial
  solutions (δ-CRDT causal stability), no clean general answer. **Active.**

### Key sources
- Hellerstein & Alvaro, *Keeping CALM*, arXiv:1901.01930 / CACM 2020; Ameloot–Neven–Van
  den Bussche, PODS 2011.
- Shapiro, Preguiça, Baquero, Zawirski, *A Comprehensive Study of CvRDTs/CmRDTs*, INRIA
  RR-7506, 2011. Almeida, Shoker, Baquero, *Delta State Replicated Data Types*, JPDC
  2018 / arXiv:1603.01529. Preguiça et al., *Dotted Version Vectors*, arXiv:1011.5808.
- Abadi, *PACELC*, IEEE Computer 2012; Gilbert & Lynch (CAP) 2002.
- Das/Gupta/Motivala, *SWIM*, IPDPS 2002. Demers et al., *Epidemic Algorithms*, PODC
  1987. Maymounkov & Mazières, *Kademlia*, IPTPS 2002.
- Waldo/Wyant/Wollrath/Kendall, *A Note on Distributed Computing*, Sun Labs 1994;
  Deutsch/Gosling, *8 Fallacies*. Fuggetta/Picco/Vigna, *Understanding Code Mobility*,
  IEEE TSE 1998.
- Spritely Goblins/OCapN CapTP docs; erights.org "dagc"; Shapiro et al., *SSP Chains*,
  PODC 1992. Cap'n Proto RPC. Bernstein et al., *Orleans virtual actors*, MSR.
  Armstrong, *Erlang/let-it-crash*.
- Kleppmann/Ink&Switch + josephg "CRDTs go brrr"; Wallace, *How Figma's multiplayer
  works*; Kermarrec–Lavoie–Tarr, *Gossiping with Append-Only Logs in SSB*, DICG 2020;
  Matrix State-Resolution-v2 analyses (arXiv:1910.06295); Smith, *Croquet/TeaTime*, DLS
  2020; Sanjuán et al., *Merkle-CRDTs*, arXiv:2004.00107.
