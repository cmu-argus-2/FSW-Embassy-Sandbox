// Stats is plain arithmetic with no clock behind it, so the summary logic the whole
// comparison rests on is checked here rather than on hardware.
use fsw_lib::bench::Stats;

// ---------------------------------------------------------------- Stats

#[test]
fn summarises_a_series_of_samples() {
    let mut stats = Stats::new(10);
    for sample in [100u32, 200, 300, 400, 500] {
        stats.record(sample);
    }

    assert_eq!(stats.count(), 5);
    assert_eq!(stats.min(), 100);
    assert_eq!(stats.max(), 500);
    assert_eq!(stats.mean(), 300);
}

#[test]
fn an_empty_series_reports_zeros_rather_than_garbage() {
    let stats = Stats::new(10);
    assert_eq!(
        (stats.count(), stats.min(), stats.max(), stats.mean()),
        (0, 0, 0, 0)
    );
    assert_eq!(stats.percentile(99), 0);
}

#[test]
fn percentiles_land_in_the_right_bucket() {
    // 100 samples, one per value 0..99, with 10us buckets
    let mut stats = Stats::new(10);
    for sample in 0..100u32 {
        stats.record(sample);
    }

    // The 50th smallest sample is 49us, in the bucket covering 40-49
    assert_eq!(stats.percentile(50), 50);
    // The 99th is 98us, in the bucket covering 90-99
    assert_eq!(
        stats.percentile(99),
        99,
        "clamped to the largest sample seen"
    );
    assert_eq!(stats.percentile(100), 99);
}

#[test]
fn one_slow_sample_moves_p99_and_max_but_not_the_median() {
    let mut stats = Stats::new(50);
    for _ in 0..99 {
        stats.record(100);
    }
    stats.record(5_000); // one late iteration

    assert_eq!(stats.max(), 5_000);
    assert_eq!(
        stats.percentile(50),
        150,
        "median should stay with the bulk of samples"
    );
    assert!(
        stats.percentile(99) < 5_000,
        "p99 of 100 samples should not be the single outlier, got {}",
        stats.percentile(99)
    );
    // The mean is dragged up, which is why percentiles are reported alongside it
    assert_eq!(stats.mean(), 149);
}

#[test]
fn no_percentile_can_read_below_the_maximum_sample() {
    // 64 buckets of 10us cover 0..640us, so this sample lands in the overflow bucket.
    // Reporting the bucket bound there would claim a p100 of 640us against a 10ms
    // maximum, which would be indefensible in a comparison.
    let mut stats = Stats::new(10);
    stats.record(50);
    stats.record(10_000);

    assert_eq!(stats.count(), 2);
    assert_eq!(
        stats.max(),
        10_000,
        "max is exact even when the histogram saturates"
    );
    assert_eq!(
        stats.percentile(100),
        10_000,
        "overflow bucket reports the exact max"
    );
    for p in 0..=100 {
        assert!(
            stats.percentile(p) <= stats.max(),
            "p{p} = {} exceeded max {}",
            stats.percentile(p),
            stats.max()
        );
    }
}

#[test]
fn percentiles_never_exceed_the_largest_sample_in_a_short_series() {
    // A single 5us sample sits in the bucket bounded at 10us; reporting 10us would
    // overstate a measurement we know exactly
    let mut stats = Stats::new(10);
    stats.record(5);

    assert_eq!(stats.percentile(50), 5);
    assert_eq!(stats.percentile(100), 5);
}

#[test]
fn reset_clears_the_series_but_keeps_the_resolution() {
    let mut stats = Stats::new(25);
    stats.record(1_000);
    stats.reset();

    assert_eq!(stats.count(), 0);
    assert_eq!(
        stats.resolution_us(),
        25,
        "bucket width should survive the reset"
    );
}

#[test]
fn resolution_is_reported_so_readers_know_what_is_exact() {
    let stats = Stats::new(50);
    assert_eq!(stats.resolution_us(), 50);
    // Zero would divide by zero in record(), so it is forced to the finest resolution
    assert_eq!(Stats::new(0).resolution_us(), 1);
}
