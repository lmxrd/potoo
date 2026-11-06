//! Per-phase timers for the hit path (`POST /api/collect`), to see where
//! the time goes under load: waiting for the database's writer lock,
//! running SQL, or everything else. Without the `profiling` feature every
//! function here is a plain call-through that the compiler removes, so
//! production builds pay nothing.

use std::future::Future;

#[derive(Clone, Copy)]
pub enum Phase {
    /// The whole handler, from the parsed request to the response.
    Total,
    /// The site lookup: from memory, or one read on a miss.
    Site,
    /// The GeoIP lookup.
    Geo,
    /// The daily salt of the visitor hash.
    Salt,
    /// Queueing the hit for the writer.
    Enqueue,
    /// The writer task, per batch: the whole batch, the wait for the
    /// database's writer connection, and the transaction itself.
    Batch,
    WriterLockWait,
    WriterExec,
}

#[cfg(not(feature = "profiling"))]
mod imp {
    use super::*;

    #[inline(always)]
    pub async fn timed<F: Future>(_: Phase, future: F) -> F::Output {
        future.await
    }

    #[inline(always)]
    pub fn time<T>(_: Phase, f: impl FnOnce() -> T) -> T {
        f()
    }
}

#[cfg(feature = "profiling")]
mod imp {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
    use std::time::{Duration, Instant};

    const NAMES: [&str; 8] = [
        "handler total",
        "site lookup",
        "geo lookup",
        "salt",
        "enqueue",
        "writer: batches (total)",
        "writer: wait for conn",
        "writer: transaction",
    ];
    static NANOS: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];
    static COUNT: [AtomicU64; 8] = [const { AtomicU64::new(0) }; 8];

    fn add(phase: Phase, since: Instant) {
        NANOS[phase as usize].fetch_add(since.elapsed().as_nanos() as u64, Relaxed);
        COUNT[phase as usize].fetch_add(1, Relaxed);
    }

    pub async fn timed<F: Future>(phase: Phase, future: F) -> F::Output {
        let start = Instant::now();
        let output = future.await;
        add(phase, start);
        output
    }

    pub fn time<T>(phase: Phase, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let output = f();
        add(phase, start);
        output
    }

    /// CPU seconds this process has used, from /proc (Linux only).
    fn cpu_seconds() -> f64 {
        let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
            return 0.0;
        };
        // The fields after the command name, which may contain spaces.
        let fields: Vec<&str> = stat
            .rsplit_once(')')
            .map(|(_, rest)| rest.split_whitespace().collect())
            .unwrap_or_default();
        let ticks = |i: usize| {
            fields
                .get(i)
                .and_then(|f| f.parse::<f64>().ok())
                .unwrap_or(0.0)
        };
        (ticks(11) + ticks(12)) / 100.0 // utime + stime, in clock ticks
    }

    /// Print the mean time per phase every few seconds, for the hits
    /// handled in that interval, and the CPU the process used per hit.
    pub fn spawn_reporter() {
        tokio::spawn(async move {
            let mut last_nanos = [0u64; 8];
            let mut last_count = [0u64; 8];
            let mut last_cpu = cpu_seconds();
            let mut last_time = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let now = Instant::now();
                let cpu = cpu_seconds();
                let hits = COUNT[0].load(Relaxed) - last_count[0];
                if hits > 0 {
                    let secs = (now - last_time).as_secs_f64();
                    println!(
                        "[profile] {:.0} hits/s, {:.2} cores, {:.0} us cpu per hit",
                        hits as f64 / secs,
                        (cpu - last_cpu) / secs,
                        (cpu - last_cpu) * 1e6 / hits as f64,
                    );
                    let batches = COUNT[Phase::Batch as usize].load(Relaxed)
                        - last_count[Phase::Batch as usize];
                    if batches > 0 {
                        println!(
                            "[profile]   {:.0} batches/s, {:.1} hits per batch",
                            batches as f64 / secs,
                            hits as f64 / batches as f64
                        );
                    }
                    for (i, name) in NAMES.iter().enumerate() {
                        let n = COUNT[i].load(Relaxed);
                        let ns = NANOS[i].load(Relaxed);
                        let (dn, dns) = (n - last_count[i], ns - last_nanos[i]);
                        if dn > 0 {
                            println!(
                                "[profile]   {name:<24} {:>8.0} us per hit",
                                dns as f64 / hits as f64 / 1000.0
                            );
                        }
                        last_count[i] = n;
                        last_nanos[i] = ns;
                    }
                }
                last_cpu = cpu;
                last_time = now;
            }
        });
    }
}

pub use imp::*;
