// src/audio/dsp.rs
//
// UX-010: Audio preprocessing for cleaner input to the transcription engine.
//
// Three lightweight DSP stages applied in-place on f32 PCM buffers (mono or
// interleaved). All stages are allocation-free and cheap enough to run on the
// hot path of the audio pipeline.
//
// 1. `dc_remove`        — remove DC offset by subtracting the block mean.
// 2. `high_pass_80hz`   — 1st-order IIR high-pass, cutoff ~80 Hz, to kill
//                         rumble, HVAC hum and low-frequency noise that would
//                         otherwise push energy into the VAD / STT front-end.
// 3. `peak_normalize_minus3db` — scale the block so the absolute peak sits at
//                         ~-3 dBFS (0.707), preventing clipping while keeping
//                         the signal loud enough for Parakeet to pick up.
//
// Design notes:
// - The high-pass uses the canonical one-pole form  y[n] = a*(y[n-1] + x[n] - x[n-1])
//   with `a` derived from the cutoff and sample rate. State is kept on the
//   caller side via `HighPassState`, so each channel can have its own filter.
// - We deliberately avoid biquads / FFT: this code runs per audio callback and
//   must not allocate. The 1st-order HP is more than enough to remove <80 Hz
//   rumble in speech recordings.
// - Peak normalization is applied per-block. We skip it when the block is
//   effectively silent (peak < 1e-4) to avoid amplifying noise.

use std::f32::consts::PI;

/// Target peak level: -3 dBFS ≈ 0.707. Chosen over -1 dBFS to leave headroom
/// for inter-sample peaks after resampling.
pub const PEAK_NORMALIZE_TARGET: f32 = 0.707;

/// Cutoff frequency for the high-pass filter. 80 Hz is below the fundamental
/// of adult male speech (~85 Hz) so we don't touch the voice, but removes
/// HVAC rumble, desk thumps, and mic-stand vibration.
pub const HIGH_PASS_CUTOFF_HZ: f32 = 80.0;

/// Per-channel state for the 1st-order IIR high-pass. Keep one instance per
/// audio channel and reuse it across blocks so the filter is stable.
#[derive(Debug, Clone, Copy, Default)]
pub struct HighPassState {
    prev_input: f32,
    prev_output: f32,
}

impl HighPassState {
    pub const fn new() -> Self {
        Self { prev_input: 0.0, prev_output: 0.0 }
    }

    pub fn reset(&mut self) {
        self.prev_input = 0.0;
        self.prev_output = 0.0;
    }
}

/// Remove DC offset by subtracting the arithmetic mean of the block.
/// Cheap, O(n), safe on empty slices.
pub fn dc_remove(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }
    let sum: f32 = samples.iter().sum();
    let mean = sum / samples.len() as f32;
    if mean.abs() < 1e-6 {
        return;
    }
    for s in samples.iter_mut() {
        *s -= mean;
    }
}

/// 1st-order IIR high-pass at ~80 Hz. Operates in place on a single channel
/// of PCM samples. For stereo / interleaved audio, call once per channel
/// with its own `HighPassState`.
pub fn high_pass_80hz(samples: &mut [f32], sample_rate: f32, state: &mut HighPassState) {
    if samples.is_empty() || sample_rate <= 0.0 {
        return;
    }
    // RC = 1 / (2*pi*fc). a = RC / (RC + dt).
    let rc = 1.0 / (2.0 * PI * HIGH_PASS_CUTOFF_HZ);
    let dt = 1.0 / sample_rate;
    let a = rc / (rc + dt);

    let mut prev_in = state.prev_input;
    let mut prev_out = state.prev_output;
    for s in samples.iter_mut() {
        let x = *s;
        let y = a * (prev_out + x - prev_in);
        prev_in = x;
        prev_out = y;
        *s = y;
    }
    state.prev_input = prev_in;
    state.prev_output = prev_out;
}

/// Scale the block so its absolute peak sits at `PEAK_NORMALIZE_TARGET`
/// (-3 dBFS). No-op on silent blocks (peak below 1e-4) to avoid amplifying
/// noise floor.
pub fn peak_normalize_minus3db(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }
    let mut peak = 0.0f32;
    for s in samples.iter() {
        let a = s.abs();
        if a > peak {
            peak = a;
        }
    }
    if peak < 1e-4 {
        return;
    }
    let gain = PEAK_NORMALIZE_TARGET / peak;
    // Only apply if it actually changes the signal meaningfully.
    if (gain - 1.0).abs() < 1e-3 {
        return;
    }
    for s in samples.iter_mut() {
        *s = (*s * gain).clamp(-1.0, 1.0);
    }
}

/// Convenience: full preprocessing chain in one call for a single channel.
pub fn preprocess_mono(samples: &mut [f32], sample_rate: f32, state: &mut HighPassState) {
    dc_remove(samples);
    high_pass_80hz(samples, sample_rate, state);
    peak_normalize_minus3db(samples);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_remove_centers_signal() {
        let mut buf = vec![0.5_f32; 1024];
        dc_remove(&mut buf);
        let sum: f32 = buf.iter().sum();
        assert!(sum.abs() < 1e-3, "residual DC after dc_remove: {}", sum);
    }

    #[test]
    fn dc_remove_noop_on_empty() {
        let mut buf: Vec<f32> = vec![];
        dc_remove(&mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn high_pass_attenuates_dc() {
        // DC signal at 1.0 — after HP it should approach 0.
        let mut buf = vec![1.0_f32; 48_000];
        let mut st = HighPassState::new();
        high_pass_80hz(&mut buf, 48_000.0, &mut st);
        // After 1s of DC, output should be well below 0.1.
        let tail_avg = buf[40_000..].iter().sum::<f32>() / 8_000.0;
        assert!(tail_avg.abs() < 0.1, "HP did not kill DC: avg={}", tail_avg);
    }

    #[test]
    fn high_pass_preserves_voice_frequencies() {
        // 1 kHz sine should survive the HP at 80 Hz with minimal loss.
        let sr = 48_000.0;
        let f = 1000.0;
        let mut buf: Vec<f32> = (0..48_000)
            .map(|i| (2.0 * PI * f * i as f32 / sr).sin())
            .collect();
        let mut st = HighPassState::new();
        let orig_peak = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        high_pass_80hz(&mut buf, sr, &mut st);
        let new_peak = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        // Expect <5% loss at 1 kHz for an 80 Hz HP.
        assert!(new_peak > orig_peak * 0.95, "1kHz too attenuated: {} -> {}", orig_peak, new_peak);
    }

    #[test]
    fn peak_normalize_hits_target() {
        let mut buf = vec![0.1_f32; 100];
        buf[50] = 0.2;
        peak_normalize_minus3db(&mut buf);
        let peak = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!((peak - PEAK_NORMALIZE_TARGET).abs() < 1e-3, "peak={}", peak);
    }

    #[test]
    fn peak_normalize_skips_silence() {
        let mut buf = vec![1e-5_f32; 100];
        peak_normalize_minus3db(&mut buf);
        // Should not amplify near-silence.
        let peak = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(peak < 1e-4);
    }

    #[test]
    fn preprocess_chain_runs_without_panics() {
        let sr = 48_000.0;
        let mut buf: Vec<f32> = (0..4_800)
            .map(|i| 0.3 + 0.4 * (2.0 * PI * 440.0 * i as f32 / sr).sin())
            .collect();
        let mut st = HighPassState::new();
        preprocess_mono(&mut buf, sr, &mut st);
        // DC should be removed, peak should be at target.
        let mean: f32 = buf.iter().sum::<f32>() / buf.len() as f32;
        let peak = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(mean.abs() < 0.05, "residual mean after chain: {}", mean);
        assert!(peak <= PEAK_NORMALIZE_TARGET + 1e-3, "peak too high: {}", peak);
    }
}
