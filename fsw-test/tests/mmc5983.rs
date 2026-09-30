// Expected values were computed independently in Python from the datasheet:
// 18-bit counts at 16384 counts/gauss with the no-field reading at 131072, and
// temperature as -75 C + 0.8 C per count.
mod common;

use common::{Event, FakeDelay, FakeRegs, take_events, writes};
use fsw_lib::drivers::mmc5983::{self, Bandwidth, MMC5983, Raw};

const ADDR: u8 = 0x30;

/// Registers of a healthy part sitting in a given field, measurement already done.
/// Bytes are X/Y/Z high and mid, then the packed low bits in 0x06.
fn measured(bytes: [u8; 7]) -> Vec<(u8, u8)> {
    let mut regs = vec![(0x2F, 0x30), (0x08, 0b11)]; // product ID, both done flags
    for (i, b) in bytes.iter().enumerate() {
        regs.push((i as u8, *b));
    }
    regs
}

fn close(got: f32, want: f32, label: &str) {
    assert!(
        (got - want).abs() <= 1e-3,
        "{label}: got {got}, want {want}"
    );
}

#[tokio::test]
async fn init_rejects_a_part_that_is_not_an_mmc5983() {
    let mut regs = FakeRegs::new(&[(0x2F, 0x10)]);
    let log = regs.log.clone();
    let mut delay = FakeDelay { log: log.clone() };

    let res = MMC5983::new(&mut regs, ADDR)
        .init(Bandwidth::Hz100, &mut delay)
        .await;

    assert!(
        matches!(res, Err(mmc5983::Error::BadId(0x10))),
        "got {res:?}"
    );
}

#[tokio::test]
async fn init_sets_bandwidth_and_magnetises_the_bridge() {
    let mut regs = FakeRegs::new(&[(0x2F, 0x30)]);
    let log = regs.log.clone();
    let mut delay = FakeDelay { log: log.clone() };

    MMC5983::new(&mut regs, ADDR)
        .init(Bandwidth::Hz400, &mut delay)
        .await
        .unwrap();

    // Bandwidth into control 1, then a SET pulse with time to complete
    let mut expected = writes(&[(0x0A, 0b10), (0x09, 1 << 3)]);
    expected.push(Event::DelayMs(1));
    assert_eq!(take_events(&log), expected);
}

#[tokio::test]
async fn measure_assembles_18_bit_counts_and_converts_to_gauss() {
    // +1 G on X, no field on Y, -2 G on Z
    let mut regs = FakeRegs::new(&measured([0x90, 0x00, 0x80, 0x00, 0x60, 0x00, 0x00]));
    let log = regs.log.clone();
    let mut delay = FakeDelay { log: log.clone() };
    let mut mag = MMC5983::new(&mut regs, ADDR);

    let raw = mag.measure(&mut delay).await.unwrap();
    assert_eq!(
        raw,
        Raw {
            x: 147_456,
            y: 131_072,
            z: 98_304
        }
    );

    let field = raw.to_field();
    close(field.x, 1.0, "x");
    close(field.y, 0.0, "y");
    close(field.z, -2.0, "z");

    // The measurement has to be triggered before the registers are read
    assert_eq!(take_events(&log), writes(&[(0x09, 1 << 0)]));
}

#[tokio::test]
async fn measure_uses_the_packed_low_bits() {
    // Bottom two bits of each axis live in 0x06 as X[1:0] Y[1:0] Z[1:0] 0 0
    let mut regs = FakeRegs::new(&measured([
        0x80,
        0x00,
        0x80,
        0x00,
        0x80,
        0x00,
        0b11_10_01_00,
    ]));
    let log = regs.log.clone();
    let mut delay = FakeDelay { log: log.clone() };

    let raw = MMC5983::new(&mut regs, ADDR)
        .measure(&mut delay)
        .await
        .unwrap();

    assert_eq!(
        raw,
        Raw {
            x: 131_075,
            y: 131_074,
            z: 131_073
        }
    );
}

#[tokio::test]
async fn measure_times_out_when_the_measurement_never_completes() {
    // Status stays 0, so the done flag never appears
    let mut regs = FakeRegs::new(&[(0x2F, 0x30), (0x08, 0x00)]);
    let log = regs.log.clone();
    let mut delay = FakeDelay { log: log.clone() };

    let res = MMC5983::new(&mut regs, ADDR).measure(&mut delay).await;

    assert!(matches!(res, Err(mmc5983::Error::Timeout)), "got {res:?}");
    // It still asked for a measurement, and waited rather than spinning
    let events = take_events(&log);
    assert_eq!(events[0], Event::Write(0x09, 1 << 0));
    assert!(
        events
            .iter()
            .filter(|e| matches!(e, Event::DelayMs(_)))
            .count()
            >= 25,
        "expected the poll loop to wait between reads: {events:?}"
    );
}

#[tokio::test]
async fn temperature_converts_counts_to_celsius() {
    for (counts, celsius) in [(0u8, -75.0f32), (100, 5.0), (255, 129.0)] {
        let mut regs = FakeRegs::new(&[(0x2F, 0x30), (0x08, 0b10), (0x07, counts)]);
        let log = regs.log.clone();
        let mut delay = FakeDelay { log: log.clone() };

        let temp = MMC5983::new(&mut regs, ADDR)
            .temperature(&mut delay)
            .await
            .unwrap();

        close(temp, celsius, "temperature");
        // Temperature is its own measurement, triggered by TM_T rather than TM_M
        assert_eq!(take_events(&log)[0], Event::Write(0x09, 1 << 1));
    }
}

#[tokio::test]
async fn offset_cancelled_read_pulses_set_then_reset_around_its_measurements() {
    let mut regs = FakeRegs::new(&measured([0x90, 0x00, 0x80, 0x00, 0x60, 0x00, 0x00]));
    let log = regs.log.clone();
    let mut delay = FakeDelay { log: log.clone() };

    let field = MMC5983::new(&mut regs, ADDR)
        .field_offset_cancelled(&mut delay)
        .await
        .unwrap();

    let events: Vec<Event> = take_events(&log)
        .into_iter()
        .filter(|e| matches!(e, Event::Write(_, _)))
        .collect();
    assert_eq!(
        events,
        writes(&[
            (0x09, 1 << 3), // SET
            (0x09, 1 << 0), // measure
            (0x09, 1 << 4), // RESET
            (0x09, 1 << 0), // measure
        ])
    );

    // Both measurements read the same registers here, so the field cancels to zero
    close(field.x, 0.0, "x");
    close(field.y, 0.0, "y");
    close(field.z, 0.0, "z");
}

#[test]
fn compensate_removes_the_bridge_offset() {
    // Offset of +0.5 G on every axis, true field +1 G on X, 0 on Y, -2 G on Z.
    // SET reads +H + offset, RESET reads -H + offset.
    let per_gauss = 16_384i64;
    let null = 131_072i64;
    let offset = per_gauss / 2;
    let build = |h: [i64; 3], sign: i64| Raw {
        x: (null + sign * h[0] * per_gauss + offset) as u32,
        y: (null + sign * h[1] * per_gauss + offset) as u32,
        z: (null + sign * h[2] * per_gauss + offset) as u32,
    };
    let set = build([1, 0, -2], 1);
    let reset = build([1, 0, -2], -1);

    let field = mmc5983::compensate(set, reset);
    close(field.x, 1.0, "x");
    close(field.y, 0.0, "y");
    close(field.z, -2.0, "z");

    // And the offset itself comes back out of the same pair
    let off = mmc5983::offset(set, reset);
    close(off.x, 0.5, "offset x");
    close(off.y, 0.5, "offset y");
    close(off.z, 0.5, "offset z");
}
