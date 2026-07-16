//! Shared Criterion config for all sift-core bench targets.

use std::time::Duration;

use criterion::Criterion;

#[must_use]
pub fn sift_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(6))
        .sample_size(100)
        .significance_level(0.05)
        .noise_threshold(0.05)
        .configure_from_args()
}
