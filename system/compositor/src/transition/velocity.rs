//! Per-property velocity tracking — observes how property values change
//! over time (from any source: explicit sets, transitions, or animations)
//! and maintains a velocity estimate via polynomial regression.
//!
//! This foundation enables velocity-matched transitions (fling, spring-match,
//! friction) and is used internally by `ScrollPhysics` for momentum/rubberband.

use std::collections::HashMap;

use super::config::AnimatableProp;

/// Number of samples in the ring buffer per property.
const SAMPLE_COUNT: usize = 8;

/// Maximum age (seconds) for a sample to be considered valid.
/// Samples older than this are excluded from the polynomial fit.
const MAX_SAMPLE_AGE: f64 = 0.15;

/// Per-property ring buffer of recent (time, value) samples.
#[derive(Debug, Clone)]
struct VelocitySamples {
    /// Ring buffer of (timestamp_secs, value) pairs.
    entries: [(f64, f32); SAMPLE_COUNT],
    write_idx: usize,
    count: usize,
}

impl Default for VelocitySamples {
    fn default() -> Self {
        Self {
            entries: [(0.0, 0.0); SAMPLE_COUNT],
            write_idx: 0,
            count: 0,
        }
    }
}

impl VelocitySamples {
    /// Record a value sample at the given time.
    fn sample(&mut self, time: f64, value: f32) {
        self.entries[self.write_idx] = (time, value);
        self.write_idx = (self.write_idx + 1) % SAMPLE_COUNT;
        if self.count < SAMPLE_COUNT {
            self.count += 1;
        }
    }

    /// Get valid samples (not older than MAX_SAMPLE_AGE), newest first.
    fn valid_samples(&self, now: f64) -> Vec<(f64, f32)> {
        let cutoff = now - MAX_SAMPLE_AGE;
        let mut samples = Vec::with_capacity(self.count);

        for i in 0..self.count {
            // Read from newest to oldest
            let idx = (self.write_idx + SAMPLE_COUNT - 1 - i) % SAMPLE_COUNT;
            let (t, v) = self.entries[idx];
            if t >= cutoff {
                samples.push((t, v));
            }
        }
        samples
    }

    /// Estimate velocity (units/sec) using polynomial regression on recent samples.
    ///
    /// Fits a quadratic (degree-2) polynomial to the sample window via
    /// least-squares, then evaluates the derivative at `now`.
    fn velocity(&self, now: f64) -> f32 {
        let samples = self.valid_samples(now);
        if samples.len() < 2 {
            return 0.0;
        }

        // Normalize time around `now` for numerical stability
        // Fit p(t) = c0 + c1*t + c2*t^2 via least squares
        let n = samples.len();
        if n == 2 {
            // Simple finite difference for 2 samples
            let (t0, v0) = samples[1]; // older
            let (t1, v1) = samples[0]; // newer
            let dt = t1 - t0;
            if dt.abs() < 1e-9 {
                return 0.0;
            }
            return ((v1 - v0) as f64 / dt) as f32;
        }

        // Quadratic fit for 3+ samples
        // Normal equations for degree-2 polynomial: [S0, S1, S2; S1, S2, S3; S2, S3, S4] * [c0,c1,c2] = [R0,R1,R2]
        // where Sk = sum(t_i^k), Rk = sum(t_i^k * v_i)
        let mut s = [0.0f64; 5]; // S0..S4
        let mut r = [0.0f64; 3]; // R0..R2

        for &(t, v) in &samples {
            let t = t - now; // center around now
            let v = v as f64;
            let mut tk = 1.0;
            for k in 0..5 {
                s[k] += tk;
                if k < 3 {
                    r[k] += tk * v;
                }
                tk *= t;
            }
        }

        // Solve 3x3 system using Cramer's rule
        let det = det3(s[0], s[1], s[2], s[1], s[2], s[3], s[2], s[3], s[4]);
        if det.abs() < 1e-12 {
            // Degenerate — fall back to finite difference
            let (t0, v0) = samples[samples.len() - 1];
            let (t1, v1) = samples[0];
            let dt = t1 - t0;
            if dt.abs() < 1e-9 {
                return 0.0;
            }
            return ((v1 - v0) as f64 / dt) as f32;
        }

        // c1 = derivative at t=0 (which is `now` after centering)
        let c1 = det3(s[0], r[0], s[2], s[1], r[1], s[3], s[2], r[2], s[4]) / det;
        // c2 not needed for velocity at t=0 since p'(0) = c1

        c1 as f32
    }
}

/// 3x3 determinant.
#[allow(clippy::too_many_arguments)]
fn det3(
    a00: f64,
    a01: f64,
    a02: f64,
    a10: f64,
    a11: f64,
    a12: f64,
    a20: f64,
    a21: f64,
    a22: f64,
) -> f64 {
    a00 * (a11 * a22 - a12 * a21) - a01 * (a10 * a22 - a12 * a20) + a02 * (a10 * a21 - a11 * a20)
}

/// Per-entity velocity tracker component.
///
/// Tracks recent value samples for opted-in animatable properties and
/// estimates instantaneous velocity via polynomial regression.
#[derive(Debug, Clone, Default)]
pub struct VelocityTracker {
    samples: HashMap<AnimatableProp, VelocitySamples>,
}

impl VelocityTracker {
    /// Record a value sample for a property at the current time.
    pub fn sample(&mut self, prop: AnimatableProp, time: f64, value: f32) {
        self.samples.entry(prop).or_default().sample(time, value);
    }

    /// Estimate instantaneous velocity (units/sec) for a property at the given time.
    pub fn velocity(&self, prop: AnimatableProp, now: f64) -> f32 {
        self.samples
            .get(&prop)
            .map(|s| s.velocity(now))
            .unwrap_or(0.0)
    }

    /// Remove tracking for a property.
    pub fn remove(&mut self, prop: AnimatableProp) {
        self.samples.remove(&prop);
    }

    /// Check if any properties are being tracked.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocity_from_constant_motion() {
        let mut tracker = VelocityTracker::default();
        // 100 units/sec constant velocity
        for i in 0..8 {
            let t = 1.0 + i as f64 * 0.016;
            let v = 100.0 * t as f32;
            tracker.sample(AnimatableProp::TranslateX, t, v);
        }
        let now = 1.0 + 7.0 * 0.016;
        let vel = tracker.velocity(AnimatableProp::TranslateX, now);
        // Should be approximately 100.0
        assert!((vel - 100.0).abs() < 5.0, "expected ~100, got {vel}");
    }

    #[test]
    fn velocity_from_two_samples() {
        let mut tracker = VelocityTracker::default();
        tracker.sample(AnimatableProp::TranslateY, 1.0, 0.0);
        tracker.sample(AnimatableProp::TranslateY, 1.1, 50.0);
        let vel = tracker.velocity(AnimatableProp::TranslateY, 1.1);
        // 50 units in 0.1s = 500 units/sec
        assert!((vel - 500.0).abs() < 1.0, "expected ~500, got {vel}");
    }

    #[test]
    fn velocity_zero_when_no_samples() {
        let tracker = VelocityTracker::default();
        let vel = tracker.velocity(AnimatableProp::TranslateX, 1.0);
        assert_eq!(vel, 0.0);
    }

    #[test]
    fn velocity_zero_when_stationary() {
        let mut tracker = VelocityTracker::default();
        for i in 0..5 {
            let t = 1.0 + i as f64 * 0.016;
            tracker.sample(AnimatableProp::TranslateX, t, 42.0);
        }
        let vel = tracker.velocity(AnimatableProp::TranslateX, 1.0 + 4.0 * 0.016);
        assert!(vel.abs() < 1.0, "expected ~0, got {vel}");
    }

    #[test]
    fn stale_samples_excluded() {
        let mut tracker = VelocityTracker::default();
        // Old samples at t=0
        for i in 0..4 {
            tracker.sample(
                AnimatableProp::TranslateX,
                i as f64 * 0.01,
                1000.0 * i as f32,
            );
        }
        // New samples at t=5 (stationary)
        for i in 0..4 {
            let t = 5.0 + i as f64 * 0.016;
            tracker.sample(AnimatableProp::TranslateX, t, 42.0);
        }
        let vel = tracker.velocity(AnimatableProp::TranslateX, 5.0 + 3.0 * 0.016);
        // Old high-velocity samples should be excluded; velocity should be ~0
        assert!(vel.abs() < 10.0, "expected ~0 (stale excluded), got {vel}");
    }

    #[test]
    fn accelerating_motion() {
        let mut tracker = VelocityTracker::default();
        // v = 200*t (acceleration = 200), position = 100*t^2
        for i in 0..8 {
            let t = i as f64 * 0.016;
            let v = 100.0 * t * t;
            tracker.sample(AnimatableProp::TranslateX, t, v as f32);
        }
        let now = 7.0 * 0.016;
        let vel = tracker.velocity(AnimatableProp::TranslateX, now);
        // Expected velocity at t=0.112: 200 * 0.112 = 22.4
        let expected = 200.0 * now as f32;
        assert!(
            (vel - expected).abs() < 5.0,
            "expected ~{expected}, got {vel}"
        );
    }
}
