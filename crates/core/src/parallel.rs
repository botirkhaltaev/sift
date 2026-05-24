use std::sync::OnceLock;

/// The kind of parallel work being considered.
pub enum ParallelWorkload {
    IndexBuild,
    CandidateScan,
}

static PARALLEL_THRESHOLD: OnceLock<usize> = OnceLock::new();

/// Minimum item count before Rayon parallelism is used for a given workload.
///
/// Value is `8 * effective_threads`, where `effective_threads` is
/// `min(RAYON_NUM_THREADS, available_parallelism)` when `RAYON_NUM_THREADS` is set and valid,
/// else `available_parallelism`. If there is only one effective thread, returns [`usize::MAX`]
/// so the sequential path is always used.
///
/// The result is computed **once per process** on first call (including `RAYON_NUM_THREADS` read).
/// Changing the env var after that has no effect until restart.
#[must_use]
pub fn parallel_threshold(_workload: ParallelWorkload) -> usize {
    *PARALLEL_THRESHOLD.get_or_init(|| {
        let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        let rayon_threads = std::env::var("RAYON_NUM_THREADS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok());
        let effective = rayon_threads
            .filter(|&n| n > 0)
            .map_or(cpus, |rt| rt.min(cpus))
            .max(1);
        if effective <= 1 {
            usize::MAX
        } else {
            effective.saturating_mul(8)
        }
    })
}
