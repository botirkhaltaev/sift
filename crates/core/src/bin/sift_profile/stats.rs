//! Durations collected per `run_index` iteration for distribution stats.

use std::time::Duration;

#[derive(Clone, Debug)]
pub struct DurationStats {
    pub sum: Duration,
    pub min: Duration,
    pub max: Duration,
    pub mean_ns: u128,
    pub p50_ns: u128,
    pub p95_ns: u128,
    pub p99_ns: u128,
}

/// # Panics
///
/// Panics if `samples` is empty (callers must check `is_empty` first).
pub fn duration_stats(samples: &mut [Duration]) -> DurationStats {
    assert!(
        !samples.is_empty(),
        "duration_stats requires at least one sample"
    );
    samples.sort();
    let n = samples.len();
    let sum: Duration = samples.iter().sum();
    let mean_ns = sum.as_nanos() / u128::try_from(n).unwrap_or(1);
    let min = samples[0];
    let max = samples[n - 1];
    DurationStats {
        sum,
        min,
        max,
        mean_ns,
        p50_ns: percentile_ns(samples, 50, 100),
        p95_ns: percentile_ns(samples, 95, 100),
        p99_ns: percentile_ns(samples, 99, 100),
    }
}

/// `idx = ⌊(n−1) · num / den⌋` — standard discrete percentile index on sorted samples.
fn percentile_ns(sorted: &[Duration], num: usize, den: usize) -> u128 {
    let n = sorted.len();
    if n == 1 {
        return sorted[0].as_nanos();
    }
    let idx = (n - 1).saturating_mul(num) / den;
    sorted[idx.min(n - 1)].as_nanos()
}
