//! A2 (mix-bus semantics) — the load-bearing proof that answers the design
//! question "should an audio buffer be its own type?": **yes**. The `Signal`
//! type gives a mix bus *additive reconcile within a tick* + *replace apply
//! across ticks*, reusing `Array`'s summing but not its accumulation. This is
//! verified end-to-end through the engine (synthesis-bigraphs.md §III).

use prism_audio::{render_kernel, render_mix, Oscillator, Wave};

const RATE: f64 = 48_000.0;
const BLOCK: usize = 256;
const N: usize = 40;

#[test]
fn two_oscillators_sum_into_one_bus_each_frame() {
    let a = Oscillator::new(Wave::Sine, 220.0, RATE, BLOCK).with_amplitude(0.3);
    let b = Oscillator::new(Wave::Sine, 330.0, RATE, BLOCK).with_amplitude(0.4);

    // Each oscillator rendered alone (the standalone references).
    let ra = render_kernel(&a, N);
    let rb = render_kernel(&b, N);
    // Both patched to ONE `Signal` bus.
    let mix = render_mix(vec![a, b], N);

    assert_eq!(mix.len(), ra.len());
    // Bus == a + b at every sample. This single equality proves BOTH:
    //  - within-tick SUM: it's a+b, not last-wins (which would equal rb), and
    //  - across-tick REPLACE: it's *this* frame's a[i]+b[i], not a growing
    //    accumulation (which would diverge after the first block).
    for i in 0..mix.len() {
        let expected = ra[i] + rb[i];
        assert!(
            (mix[i] - expected).abs() < 1e-6,
            "sample {i}: bus {} != a+b {} — mixing/replace semantics broken",
            mix[i],
            expected
        );
    }
}

#[test]
fn single_source_bus_does_not_accumulate() {
    // One oscillator on a `Signal` bus must equal its standalone render — the
    // bus holds THIS frame, not a sum over ticks. (Accumulating apply would
    // make frame k ≈ k× the block and blow past amplitude immediately.)
    let a = Oscillator::new(Wave::Saw, 110.0, RATE, BLOCK).with_amplitude(0.5);
    let bus = render_mix(vec![a.clone()], N);
    let solo = render_kernel(&a, N);

    assert_eq!(bus.len(), solo.len());
    for i in 0..bus.len() {
        assert!(
            (bus[i] - solo[i]).abs() < 1e-6,
            "sample {i}: bus {} != solo {} — bus is accumulating across ticks",
            bus[i],
            solo[i]
        );
    }
}

#[test]
fn three_voices_mix_and_stay_bounded() {
    // A small chord — three detuned oscillators summed. Their amplitudes sum
    // to 0.6, so the mix stays within [-0.6, 0.6]; and it must equal the
    // sample-wise sum of the three standalone renders.
    let parts = [
        Oscillator::new(Wave::Sine, 220.0, RATE, BLOCK).with_amplitude(0.2),
        Oscillator::new(Wave::Sine, 277.18, RATE, BLOCK).with_amplitude(0.2),
        Oscillator::new(Wave::Sine, 329.63, RATE, BLOCK).with_amplitude(0.2),
    ];
    let refs: Vec<Vec<f32>> = parts.iter().map(|o| render_kernel(o, N)).collect();
    let mix = render_mix(parts.to_vec(), N);

    for i in 0..mix.len() {
        let expected: f32 = refs.iter().map(|r| r[i]).sum();
        assert!((mix[i] - expected).abs() < 1e-6, "sample {i}: 3-way mix");
        assert!(mix[i].abs() <= 0.6 + 1e-4, "sample {i}: sum of amps bounds it");
    }
}
