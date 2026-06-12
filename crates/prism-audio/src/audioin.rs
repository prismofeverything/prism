//! `AudioIn` — the input device as a SOURCE (mic / line / the ES-9) — the dual of
//! [`AudioOut`](crate::audioout). The cpal input callback fills a ring on its own thread;
//! `update` drains a block (channels averaged to mono) onto `out`. Process live input:
//! `AudioIn → Svf → AudioOut` is a live effect. Paired with `AudioOut` in one patch, the
//! engine is paced by the output sink and `AudioIn` drains one block per tick — a duplex
//! loop. Behind `realtime`; without it (or with no input device) it is a silence source.
//!
//! Same `Send` boundary as `AudioOut`: a spawned thread owns the `!Send` stream (the
//! callback holds the ring PRODUCER); `AudioIn` holds the Send ring CONSUMER.

#[cfg(feature = "realtime")]
pub use realtime::AudioIn;
#[cfg(not(feature = "realtime"))]
pub use stub::AudioIn;

#[cfg(feature = "realtime")]
mod realtime {
    use std::any::Any;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::Duration;

    use cpal::traits::StreamTrait;
    use cpal::{BufferSize, SampleRate, StreamConfig};
    use indexmap::IndexMap;
    use ringbuf::traits::{Consumer, Split};
    use ringbuf::HeapRb;

    use prism_bigraph::{Process, Schema, Update, Value};

    use crate::signal::{signal_from_slice, signal_type};

    type RingCons = <HeapRb<f32> as Split>::Cons;

    pub struct AudioIn {
        block: usize,
        sample_rate: f64,
        channels: u16,
        cons: Option<Arc<Mutex<RingCons>>>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl AudioIn {
        pub fn from_config(config: &Value) -> Self {
            let block = config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(256) as usize;
            let sample_rate = config
                .get_field("sample_rate")
                .and_then(|v| v.as_f64())
                .unwrap_or(48_000.0);
            let device_name = config.get_field("device").and_then(|v| v.as_str()).map(str::to_string);
            match open_input(device_name, sample_rate) {
                Ok((cons, channels, stop, thread)) => AudioIn {
                    block,
                    sample_rate,
                    channels,
                    cons: Some(Arc::new(Mutex::new(cons))),
                    stop,
                    thread: Some(thread),
                },
                Err(e) => {
                    eprintln!("AudioIn: no input device ({e}); producing silence");
                    AudioIn {
                        block,
                        sample_rate,
                        channels: 1,
                        cons: None,
                        stop: Arc::new(AtomicBool::new(false)),
                        thread: None,
                    }
                }
            }
        }
    }

    fn open_input(
        device_name: Option<String>,
        sample_rate: f64,
    ) -> anyhow::Result<(RingCons, u16, Arc<AtomicBool>, JoinHandle<()>)> {
        let device = gorgon::audio::find_input_device(device_name.as_deref())?;
        let channels = gorgon::audio::max_input_channels(&device)?.clamp(1, 2);
        let config = StreamConfig {
            channels,
            sample_rate: SampleRate(sample_rate as u32),
            buffer_size: BufferSize::Default,
        };
        let capacity = (sample_rate * 0.2) as usize * channels as usize;
        let (prod, cons) = HeapRb::<f32>::new(capacity).split();

        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        // The `!Send` stream lives on this thread; its callback owns the ring PRODUCER.
        let thread = std::thread::spawn(move || {
            let stream = match gorgon::audio::build_input_stream(&device, &config, prod) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("AudioIn: build input stream: {e}");
                    return;
                }
            };
            if let Err(e) = stream.play() {
                eprintln!("AudioIn: play: {e}");
                return;
            }
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        Ok((cons, channels, stop, thread))
    }

    impl Process for AudioIn {
        fn inputs(&self) -> IndexMap<String, Schema> {
            IndexMap::new() // a pure source
        }

        fn outputs(&self) -> IndexMap<String, Schema> {
            IndexMap::from([("out".to_string(), signal_type())])
        }

        fn interval(&self) -> f64 {
            self.block as f64 / self.sample_rate
        }

        fn update(&self, _state: &Value, _interval: f64) -> Update {
            let n = self.block;
            let ch = self.channels as usize;
            let mut mono = vec![0.0_f32; n];
            if let Some(cons) = &self.cons {
                let mut c = cons.lock().expect("AudioIn ring poisoned");
                let mut buf = vec![0.0_f32; n * ch];
                let got = c.pop_slice(&mut buf);
                let frames = (got / ch).min(n);
                for (f, m) in mono.iter_mut().enumerate().take(frames) {
                    let sum: f32 = (0..ch).map(|k| buf[f * ch + k]).sum();
                    *m = sum / ch as f32;
                }
                // frames < n (underrun) → the tail stays silence.
            }
            Update::value(Value::tree([("out", signal_from_slice(&mono))]))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    impl Drop for AudioIn {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(h) = self.thread.take() {
                let _ = h.join();
            }
        }
    }

    impl std::fmt::Debug for AudioIn {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("AudioIn")
                .field("channels", &self.channels)
                .field("device", &self.cons.is_some())
                .finish()
        }
    }
}

#[cfg(not(feature = "realtime"))]
mod stub {
    use std::any::Any;

    use indexmap::IndexMap;
    use prism_bigraph::{Process, Schema, Update, Value};

    use crate::signal::{silence, signal_type};

    /// Without `realtime` there is no device — `AudioIn` is a silence source (so a patch
    /// that captures input still compiles + renders offline).
    #[derive(Clone, Debug)]
    pub struct AudioIn {
        block: usize,
        sample_rate: f64,
    }

    impl AudioIn {
        pub fn from_config(config: &Value) -> Self {
            AudioIn {
                block: config.get_field("block").and_then(|v| v.as_i64()).unwrap_or(256) as usize,
                sample_rate: config
                    .get_field("sample_rate")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(48_000.0),
            }
        }
    }

    impl Process for AudioIn {
        fn inputs(&self) -> IndexMap<String, Schema> {
            IndexMap::new()
        }
        fn outputs(&self) -> IndexMap<String, Schema> {
            IndexMap::from([("out".to_string(), signal_type())])
        }
        fn interval(&self) -> f64 {
            self.block as f64 / self.sample_rate
        }
        fn update(&self, _state: &Value, _interval: f64) -> Update {
            Update::value(Value::tree([("out", silence(self.block))]))
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }
}
