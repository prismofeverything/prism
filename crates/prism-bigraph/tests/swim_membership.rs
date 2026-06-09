//! #62 / grand-synthesis M1 — SWIM membership (slice 1: AUTO-DISCOVERY).
//!
//! Peers find each other with NO hardcoded port list: membership is a per-source
//! `map[id -> {port}]` mesh link, and the existing gossip disseminates it
//! infection-style. A peer joins knowing only a SEED; the membership spreads so
//! every peer learns every peer — B discovers C transitively through the seed A.

use std::time::{Duration, Instant};

use prism_bigraph::protocols::Membership;

fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    cond()
}

fn ids(m: &Membership) -> Vec<String> {
    let mut v: Vec<String> = m.members().into_iter().map(|(id, _)| id).collect();
    v.sort();
    v
}

#[test]
fn peers_auto_discover_via_a_seed_with_no_hardcoded_list() {
    let tick = Duration::from_millis(10);

    // A is the seed — it knows no peers yet.
    let a = Membership::join("a", &[], tick).unwrap();
    std::thread::sleep(Duration::from_millis(50)); // A's membership server up.

    // B and C each know ONLY the seed A — NOT each other.
    let b = Membership::join("b", &[a.port()], tick).unwrap();
    let c = Membership::join("c", &[a.port()], tick).unwrap();

    // Through continuous gossip over the membership mesh link, every peer learns
    // EVERY peer — B discovers C (and vice-versa) transitively through the seed.
    let want = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    assert!(
        wait_until(Duration::from_secs(5), || ids(&a) == want
            && ids(&b) == want
            && ids(&c) == want),
        "all peers auto-discovered the full membership with no hardcoded list: \
         a={:?} b={:?} c={:?}",
        ids(&a),
        ids(&b),
        ids(&c)
    );

    // And the gossip set each peer derives is the membership minus itself (so the
    // mesh would gossip the whole cluster) — confirm B sees a real port for C.
    let c_port = c.port();
    assert!(
        b.members().iter().any(|(id, p)| id == "c" && *p == c_port),
        "B learned C's real address through the seed: {:?}",
        b.members()
    );
}
