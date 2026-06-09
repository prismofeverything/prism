//! The device-boundary seam, tested without hardware (A3).
//!
//! The realtime driver's data path is: mono `Signal` block → [`interleave`] →
//! lock-free ring → cpal callback. cpal needs a device, but the *seam* — the
//! interleave + the ring round-trip — is pure Rust, so we test it deterministically
//! here. (The actual playback is `examples/play.rs`, run with `--features realtime`
//! on real hardware.) This is the offline-first discipline applied to the realtime
//! path: test the data flow in CI, leave only the wall-clock to the device.

use prism_audio::interleave;
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;

#[test]
fn interleave_and_ring_roundtrip() {
    let mono = vec![0.1f32, -0.2, 0.3, -0.4];

    // Mono → stereo: the same sample on both channels.
    let inter = interleave(&mono, 2);
    assert_eq!(inter, vec![0.1, 0.1, -0.2, -0.2, 0.3, 0.3, -0.4, -0.4]);

    // Round-trip the interleaved block through a lock-free ring — the engine-thread
    // ↔ audio-thread seam the realtime driver pushes into and the cpal callback
    // pops from. The values must survive unchanged.
    let (mut prod, mut cons) = HeapRb::<f32>::new(16).split();
    assert_eq!(prod.push_slice(&inter), inter.len());
    let mut got = vec![0.0f32; inter.len()];
    assert_eq!(cons.pop_slice(&mut got), inter.len());
    assert_eq!(got, inter);

    // Deinterleaving channel 0 recovers the original mono block.
    let ch0: Vec<f32> = got.iter().step_by(2).copied().collect();
    assert_eq!(ch0, mono);
}

#[test]
fn interleave_mono_is_identity() {
    let mono = vec![0.5f32, -0.5, 0.25];
    assert_eq!(interleave(&mono, 1), mono);
}
