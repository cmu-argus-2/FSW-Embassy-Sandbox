/*
 * Measurement harness for the three-port flight software comparison.
 *
 * Every measurement is reported as one CSV row over defmt so the Rust, Zephyr and
 * F Prime ports produce directly comparable numbers:
 *
 *   BENCH,port,category,metric,subject,stat,value,unit,n
 *
 * A run's output can be filtered with `grep ^BENCH` and concatenated across ports.
 *
 * Resolution: embassy-time runs at 1 MHz here (embassy-rp selects tick-hz-1_000_000),
 * so Instant has 1 us granularity and the microsecond units below are real rather
 * than rounded. Confirm with `cargo tree -i embassy-time-driver -e features` if the
 * dependency ever changes; a 32 kHz tick would quantise every number to 31 us.
 *
 * The arithmetic in Stats has no dependency on the clock or the executor, so it is
 * unit tested on the host. The timers that feed it follow in a separate change.
 */

/// Identifies this port in every row it emits.
pub const PORT: &str = "rust-embassy";

/// Which load the measurement was taken under. A task alone on the CPU meets its
/// deadline on any of the three ports; the ports diverge under contention, so the
/// scenario is part of every row rather than a note beside the results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Scenario {
    /// The measured task alone.
    Idle,
    /// Every planned task at its real rate.
    Nominal,
    /// Nominal plus a task that deliberately overruns its period.
    Overload,
}

impl Scenario {
    pub fn as_str(self) -> &'static str {
        match self {
            Scenario::Idle => "idle",
            Scenario::Nominal => "nominal",
            Scenario::Overload => "overload",
        }
    }
}

/// Identifies one measurement run: which load, and which repeat.
///
/// Results are only comparable across ports when both are stated, and a single run
/// cannot support a claim at all: the spec calls for at least 5 runs per scenario and
/// reporting the spread alongside the median.
#[derive(Debug, Clone, Copy)]
pub struct Run {
    pub scenario: Scenario,
    /// 1-based repeat index.
    pub index: u32,
}

impl Run {
    pub fn new(scenario: Scenario, index: u32) -> Self {
        Self { scenario, index }
    }
}

/// How many histogram buckets Stats keeps for percentile estimates.
const BUCKETS: usize = 64;

/// Summary of a series of samples in microseconds.
///
/// `count`, `min`, `max` and `mean` are exact. Percentiles come from a fixed
/// histogram, so they are reported as the upper bound of the bucket the sample
/// falls in: `percentile(99)` is the smallest bucket bound below which 99% of
/// samples fall, which overestimates by less than one bucket width.
///
/// Samples beyond the last bucket (`BUCKETS * bucket_us`) still count, and a
/// percentile that lands in that overflow bucket reports the exact `max` rather
/// than the bucket bound, so a percentile can never read lower than the maximum.
#[derive(Debug)]
pub struct Stats {
    count: u32,
    min: u32,
    max: u32,
    sum: u64,
    bucket_us: u32,
    buckets: [u32; BUCKETS],
}

impl Stats {
    /// `bucket_us` sets percentile resolution: 64 buckets of this width, so 50us
    /// buckets resolve percentiles to 50us anywhere in 0..3.2ms.
    pub const fn new(bucket_us: u32) -> Self {
        Self {
            count: 0,
            min: u32::MAX,
            max: 0,
            sum: 0,
            bucket_us: if bucket_us == 0 { 1 } else { bucket_us },
            buckets: [0; BUCKETS],
        }
    }

    pub fn record(&mut self, sample_us: u32) {
        // Saturating rather than wrapping: a measurement tool must never report a
        // number that silently rolled over
        self.count = self.count.saturating_add(1);
        self.sum = self.sum.saturating_add(sample_us as u64);
        if sample_us < self.min {
            self.min = sample_us;
        }
        if sample_us > self.max {
            self.max = sample_us;
        }
        let idx = (sample_us / self.bucket_us) as usize;
        let idx = idx.min(BUCKETS - 1);
        self.buckets[idx] = self.buckets[idx].saturating_add(1);
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    /// Exact smallest sample, or 0 if nothing was recorded.
    pub fn min(&self) -> u32 {
        if self.count == 0 { 0 } else { self.min }
    }

    /// Exact largest sample.
    pub fn max(&self) -> u32 {
        self.max
    }

    /// Exact arithmetic mean, truncated to whole microseconds.
    pub fn mean(&self) -> u32 {
        if self.count == 0 {
            0
        } else {
            (self.sum / self.count as u64) as u32
        }
    }

    /// Estimated `p`th percentile in microseconds, p in 0..=100.
    ///
    /// Returns the bucket's upper bound, except in the overflow bucket where it
    /// returns the exact maximum. Never exceeds `max()`.
    pub fn percentile(&self, p: u32) -> u32 {
        if self.count == 0 {
            return 0;
        }
        // 1-based rank of the sample we want, rounded up: p99 of 100 samples is the 99th
        let rank = (self.count as u64 * p.min(100) as u64).div_ceil(100).max(1);
        let mut seen = 0u64;
        for (i, hits) in self.buckets.iter().enumerate() {
            seen += *hits as u64;
            if seen >= rank {
                if i == BUCKETS - 1 {
                    // Overflow bucket has no meaningful upper bound
                    return self.max;
                }
                // A bound can still exceed the largest sample in a short series
                return (((i as u32) + 1) * self.bucket_us).min(self.max);
            }
        }
        self.max
    }

    /// Percentile resolution in microseconds; emitted alongside results so a reader
    /// can tell which figures are exact and which are bucketed.
    pub fn resolution_us(&self) -> u32 {
        self.bucket_us
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.bucket_us);
    }

    /// Emit one CSV row per statistic, plus the histogram resolution.
    ///
    /// `category` is performance, efficiency, stability or correctness; `subject`
    /// names what was measured, e.g. "eps_task".
    pub fn report(&self, run: Run, category: &str, metric: &str, subject: &str, unit: &str) {
        for (stat, value) in [
            ("min", self.min()),
            ("mean", self.mean()),
            ("p50", self.percentile(50)),
            ("p99", self.percentile(99)),
            ("max", self.max()),
        ] {
            row(
                run,
                category,
                metric,
                subject,
                stat,
                value,
                unit,
                self.count(),
            );
        }
        row(
            run,
            category,
            metric,
            subject,
            "resolution",
            self.resolution_us(),
            unit,
            self.count(),
        );
    }
}

/// Emit a single measurement row. Use this directly for one-off values such as a
/// reset count or an image size.
pub fn row(
    run: Run,
    category: &str,
    metric: &str,
    subject: &str,
    stat: &str,
    value: u32,
    unit: &str,
    n: u32,
) {
    defmt::info!(
        "BENCH,{},{},{},{},{},{},{},{},{},{}",
        PORT,
        run.scenario.as_str(),
        run.index,
        category,
        metric,
        subject,
        stat,
        value,
        unit,
        n
    );
}
