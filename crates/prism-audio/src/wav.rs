//! A minimal, dependency-free 16-bit PCM WAV writer — for listening to an
//! offline render. (Audio internally is f32/f64; WAV-16 is just the artifact
//! format. A1 needs nothing more; richer I/O lands with the device boundary.)

use std::io;
use std::path::Path;

/// Write mono f32 samples (clamped to `[-1, 1]`) as a 16-bit PCM WAV file.
pub fn write_wav_i16(path: impl AsRef<Path>, samples: &[f32], sample_rate: u32) -> io::Result<()> {
    let channels: u16 = 1;
    let bits: u16 = 16;
    let block_align: u16 = channels * bits / 8;
    let byte_rate: u32 = sample_rate * block_align as u32;
    let data_bytes: u32 = samples.len() as u32 * block_align as u32;

    let mut buf = Vec::with_capacity(44 + data_bytes as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // audio format = PCM
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits.to_le_bytes());
    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_bytes.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }

    std::fs::write(path, buf)
}
