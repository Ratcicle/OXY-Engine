//! Optional thread-local counters. Disabled builds inline updates to no-ops.
use serde::Serialize;
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct Counters {
    pub entity_queries: u64,
    pub entity_scans: u64,
    pub hierarchy_visits: u64,
    pub matrices: u64,
    pub index_builds: u64,
    pub matrix_hits: u64,
    pub boxes: u64,
    pub candidates: u64,
    pub overlaps: u64,
    pub actions: u64,
    pub event_entity_visits: u64,
    pub graph_nodes_copied: u64,
    pub steps: u64,
    pub physics_prepare_ns: u64,
    pub physics_filter_ns: u64,
    pub physics_resolve_ns: u64,
    pub movement_ns: u64,
    pub areas_ns: u64,
    pub tasks_ns: u64,
    pub animation_ns: u64,
    pub fixed_step_ns: u64,
    pub input_ns: u64,
    pub character_prepare_ns: u64,
    pub character_motor_ns: u64,
    /// Inclusive subset of character_motor_ns (do not add both).
    pub character_resolve_ns: u64,
    pub character_sensors_ns: u64,
    /// Outside fixed steps: interpolation, presentation queries and camera.
    pub presentation_ns: u64,
}
#[cfg(feature = "profiling")]
thread_local! { static COUNTERS: std::cell::RefCell<Counters> = Default::default(); }
#[inline]
pub fn count(update: impl FnOnce(&mut Counters)) {
    #[cfg(feature = "profiling")]
    COUNTERS.with(|c| update(&mut c.borrow_mut()));
    #[cfg(not(feature = "profiling"))]
    let _ = update;
}
pub fn take() -> Counters {
    #[cfg(feature = "profiling")]
    {
        COUNTERS.with(|c| std::mem::take(&mut *c.borrow_mut()))
    }
    #[cfg(not(feature = "profiling"))]
    {
        Counters::default()
    }
}
pub fn timed<T>(f: impl FnOnce() -> T, record: impl FnOnce(&mut Counters, u64)) -> T {
    #[cfg(feature = "profiling")]
    let start = std::time::Instant::now();
    let result = f();
    #[cfg(feature = "profiling")]
    count(|c| record(c, start.elapsed().as_nanos() as u64));
    #[cfg(not(feature = "profiling"))]
    let _ = record;
    result
}
pub const ENABLED: bool = cfg!(feature = "profiling");
