//! `AudioOut` — the audio device as a **sink in the bigraph** (the world-boundary face,
//! `docs/web-bigraphs.md` §9 / `docs/categorical-core.md`). You WIRE a `Signal` into its
//! `input`; it writes each block to the output device. Because the device drains at
//! real-time and the engine fills *ahead* of it, a full ring **back-pressures**
//! `update`, which paces the whole engine to wall-clock time. So `chrysalis run
//! patch.ys` *plays* simply because the sink is in the graph — there is no `--play`
//! flag, no mode (Felleisen: a wired-in sink IS the play). It is the audio dual of the
//! `web:` served face — one boundary, different backend.
//!
//! `AudioOut` PASSES THROUGH (`out = input`), so the same patch **renders** offline
//! (`out` carries the Signal as JSON) AND **plays** under `--features realtime` — only
//! the side effect of touching hardware differs.
//!
//! Implementation note (the `Send` boundary): `Process: Send + Sync`, but a
//! `cpal::Stream` is `!Send`. So `AudioOut` holds only the Send ring **producer**; a
//! dedicated audio thread (spawned in [`AudioOut::from_config`]) owns the `!Send` stream
//! and lets cpal drain the ring. The lock-free ring is the Send seam (the same one
//! [`crate::device`] uses). No device → silent passthrough (never blocks).

#[cfg(feature = "realtime")]
pub use realtime::AudioOut;
#[cfg(not(feature = "realtime"))]
pub use stub::AudioOut;

// ── the real device sink (behind `realtime`) ─────────────────────────────────
#[cfg(feature = "realtime")]
mod realtime {
    use std::any::Any;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use cpal::traits::StreamTrait;
    use cpal::{BufferSize, SampleRate, StreamConfig};
    use indexmap::IndexMap;
    use ringbuf::traits::{Observer, Producer, Split};
    use ringbuf::HeapRb;

    use prism_bigraph::{Process, Schema, Update, Value};

    use crate::device::interleave;
    use crate::signal::{signal_from_slice, signal_to_vec, signal_type};

    /// The ring producer half (the exact type `HeapRb::split()` yields).
    type RingProd = <HeapRb<f32> as Split>::Prod;

    pub struct AudioOut {
        block: usize,
        sample_rate: f64,
        channels: u16,
        /// `Some` when a device is open; `None` ⇒ silent passthrough (no device found).
        ring: Option<Arc<Mutex<RingProd>>>,
        /// Total ring capacity in samples (interleaved) — for the drain-on-drop check.
        capacity: usize,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl AudioOut {
        pub fn from_config(config: &Value) -> Self {
            let block = config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(256) as usize;
            let sample_rate = config
                .get_field("sample_rate")
                .and_then(|v| v.as_f64())
                .unwrap_or(48_000.0);
            let device_name = config
                .get_field("device")
                .and_then(|v| v.as_str())
                .map(str::to_string);

            match open_device(device_name, sample_rate) {
                Ok((prod, channels, capacity, stop, thread)) => AudioOut {
                    block,
                    sample_rate,
                    channels,
                    ring: Some(Arc::new(Mutex::new(prod))),
                    capacity,
                    stop,
                    thread: Some(thread),
                },
                Err(e) => {
                    // No device (headless / unavailable) → run as a silent passthrough so
                    // the patch still RUNS (and renders offline). Never blocks.
                    eprintln!("AudioOut: no output device ({e}); passing through silently");
                    AudioOut {
                        block,
                        sample_rate,
                        channels: 1,
                        ring: None,
                        capacity: 0,
                        stop: Arc::new(AtomicBool::new(false)),
                        thread: None,
                    }
                }
            }
        }
    }

    /// Open the output device, build the ring, and spawn the audio thread that owns the
    /// `!Send` stream + lets cpal drain the ring. Returns the Send producer + control.
    fn open_device(
        device_name: Option<String>,
        sample_rate: f64,
    ) -> anyhow::Result<(RingProd, u16, usize, Arc<AtomicBool>, JoinHandle<()>)> {
        let device = gorgon::audio::find_output_device(device_name.as_deref())?;
        let channels = gorgon::audio::max_output_channels(&device)?.clamp(1, 2);
        let config = StreamConfig {
            channels,
            sample_rate: SampleRate(sample_rate as u32),
            buffer_size: BufferSize::Default,
        };
        // ~200 ms of slack so the engine can run ahead of the device before back-pressuring.
        let capacity = (sample_rate * 0.2) as usize * channels as usize;
        let (prod, cons) = HeapRb::<f32>::new(capacity).split();

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        // The `!Send` stream is created + owned ENTIRELY on this thread (cpal runs the
        // callback on its own thread; we just keep the handle alive until `stop`).
        let thread = std::thread::spawn(move || {
            let stream = match gorgon::audio::build_output_stream(&device, &config, cons) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("AudioOut: build output stream: {e}");
                    return;
                }
            };
            if let Err(e) = stream.play() {
                eprintln!("AudioOut: play: {e}");
                return;
            }
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
            // `stream` drops here → the device stops.
        });

        Ok((prod, channels, capacity, stop, thread))
    }

    impl Process for AudioOut {
        fn inputs(&self) -> IndexMap<String, Schema> {
            IndexMap::from([("input".to_string(), signal_type())])
        }

        fn outputs(&self) -> IndexMap<String, Schema> {
            // Passthrough — so the played Signal can also be tapped / rendered.
            IndexMap::from([("out".to_string(), signal_type())])
        }

        fn interval(&self) -> f64 {
            self.block as f64 / self.sample_rate
        }

        fn update(&self, state: &Value, _interval: f64) -> Update {
            let input = state.get_field("input").map(signal_to_vec).unwrap_or_default();

            if let Some(ring) = &self.ring {
                let block = interleave(&input, self.channels);
                let need = block.len();
                // THE PACING: wait until the device has drained enough room, then push.
                // While the ring is full the engine is blocked here → real-time tempo.
                loop {
                    if self.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let mut prod = ring.lock().expect("AudioOut ring poisoned");
                    if prod.vacant_len() >= need {
                        prod.push_slice(&block);
                        break;
                    }
                    drop(prod);
                    std::thread::sleep(Duration::from_millis(1));
                }
            }

            Update::value(Value::tree([("out", signal_from_slice(&input))]))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    impl Drop for AudioOut {
        fn drop(&mut self) {
            // Let the tail of the ring play out before stopping the device (else the last
            // ~200 ms is cut off), bounded so a stalled device can't hang the drop.
            if let Some(ring) = &self.ring {
                let deadline = Instant::now() + Duration::from_millis(400);
                while Instant::now() < deadline {
                    let drained = {
                        let prod = ring.lock().expect("AudioOut ring poisoned");
                        prod.vacant_len() >= self.capacity.saturating_sub(self.channels as usize)
                    };
                    if drained {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            self.stop.store(true, Ordering::Relaxed);
            if let Some(h) = self.thread.take() {
                let _ = h.join();
            }
        }
    }

    impl std::fmt::Debug for AudioOut {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AudioOut")
                .field("channels", &self.channels)
                .field("device", &self.ring.is_some())
                .finish()
        }
    }
}

// ── the offline passthrough stub (no `realtime` feature) ─────────────────────
#[cfg(not(feature = "realtime"))]
mod stub {
    use std::any::Any;

    use indexmap::IndexMap;
    use prism_bigraph::{Process, Schema, Update, Value};

    use crate::signal::{signal_from_slice, signal_to_vec, signal_type};

    /// Without the `realtime` feature there is no device — `AudioOut` is a transparent
    /// passthrough (`out = input`), so a patch that wires it still RENDERS offline (the
    /// Signal flows through) and only *plays* once built `--features realtime`.
    #[derive(Clone, Debug)]
    pub struct AudioOut {
        block: usize,
        sample_rate: f64,
    }

    impl AudioOut {
        pub fn from_config(config: &Value) -> Self {
            AudioOut {
                block: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(256) as usize,
                sample_rate: config
                    .get_field("sample_rate")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(48_000.0),
            }
        }
    }

    impl Process for AudioOut {
        fn inputs(&self) -> IndexMap<String, Schema> {
            IndexMap::from([("input".to_string(), signal_type())])
        }
        fn outputs(&self) -> IndexMap<String, Schema> {
            IndexMap::from([("out".to_string(), signal_type())])
        }
        fn interval(&self) -> f64 {
            self.block as f64 / self.sample_rate
        }
        fn update(&self, state: &Value, _interval: f64) -> Update {
            let input = state.get_field("input").map(signal_to_vec).unwrap_or_default();
            Update::value(Value::tree([("out", signal_from_slice(&input))]))
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }
}
