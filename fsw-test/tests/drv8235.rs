// Expected register writes and readings were recorded by running the flight driver
// (FSW-mainboard flight/hal/drivers/drv8235.py) against a simulated register map.
mod common;

use common::{FakeRegs, take_events, writes};
use fsw_lib::drivers::drv8235::{DRV8235, Faults};

const ADDR: u8 = 0x30;

fn assert_close(label: &str, got: Option<f32>, want: Option<f32>) {
    match (got, want) {
        (Some(g), Some(w)) => assert!(
            (g - w).abs() <= 1e-4 * w.abs().max(1.0),
            "{label}: got {g}, want {w}"
        ),
        _ => assert_eq!(got, want, "{label}"),
    }
}

#[tokio::test]
async fn init_matches_flight_driver() {
    let mut regs = FakeRegs::new(&[]);
    let log = regs.log.clone();
    DRV8235::new(&mut regs, ADDR).init().await.unwrap();
    assert_eq!(
        take_events(&log),
        writes(&[
            (0x0D, 0x04),
            (0x0D, 0x0C),
            (0x0D, 0x0C),
            (0x0E, 0x18),
            (0x0F, 0x00),
            (0x0C, 0x10),
            (0x13, 0xC0),
            (0x14, 0x52),
            (0x09, 0x80),
            (0x09, 0x82),
        ]),
        "init from all-zero registers"
    );

    // Starting from all-ones registers checks that only the intended bits change
    let ones: Vec<(u8, u8)> = (0x00..=0x19).map(|reg| (reg, 0xFF)).collect();
    let mut regs = FakeRegs::new(&ones);
    let log = regs.log.clone();
    DRV8235::new(&mut regs, ADDR).init().await.unwrap();
    assert_eq!(
        take_events(&log),
        writes(&[
            (0x0D, 0xFF),
            (0x0D, 0xFF),
            (0x0D, 0xFC),
            (0x0E, 0xFF),
            (0x0F, 0x00),
            (0x0C, 0xFF),
            (0x13, 0xFF),
            (0x14, 0x52),
            (0x09, 0xFF),
            (0x09, 0xFF),
        ]),
        "init from all-ones registers"
    );
}

enum Set {
    Throttle(Option<f32>),
    Volts(Option<f32>),
    Raw(Option<i16>),
}

#[tokio::test]
async fn throttle_matches_flight_driver() {
    // (setter, register writes, throttle(), throttle_volts(), throttle_raw())
    let cases: [(Set, &[(u8, u8)], Option<f32>, Option<f32>, Option<i16>); 12] = [
        (
            Set::Throttle(Some(1.0)),
            &[(0x0F, 0x28), (0x0D, 0x0E)],
            Some(0.9943333),
            Some(6.6932),
            Some(40),
        ),
        (
            Set::Throttle(Some(-0.5)),
            &[(0x0F, 0x14), (0x0D, 0x0D)],
            Some(-0.494),
            Some(-3.3466),
            Some(-20),
        ),
        (
            Set::Throttle(Some(2.0)),
            &[(0x0F, 0x28), (0x0D, 0x0E)],
            Some(0.9943333),
            Some(6.6932),
            Some(40),
        ),
        (
            Set::Throttle(Some(0.0)),
            &[(0x0F, 0x00), (0x0D, 0x0F)],
            Some(0.0),
            Some(0.0),
            Some(0),
        ),
        (
            Set::Throttle(None),
            &[(0x0F, 0x00), (0x0D, 0x0C)],
            None,
            None,
            None,
        ),
        (
            Set::Volts(Some(3.0)),
            &[(0x0F, 0x12), (0x0D, 0x0E)],
            Some(0.4496667),
            Some(3.01194),
            Some(18),
        ),
        (
            Set::Volts(Some(-50.0)),
            &[(0x0F, 0xFF), (0x0D, 0x0D)],
            Some(-6.3333333),
            Some(-42.66915),
            Some(-255),
        ),
        (
            Set::Volts(Some(0.1)),
            &[(0x0F, 0x01), (0x0D, 0x0E)],
            Some(0.0253333),
            Some(0.16733),
            Some(1),
        ),
        (
            Set::Volts(None),
            &[(0x0F, 0x00), (0x0D, 0x0C)],
            None,
            None,
            None,
        ),
        (
            Set::Raw(Some(100)),
            &[(0x0F, 0x64), (0x0D, 0x0E)],
            Some(2.4826667),
            Some(16.733),
            Some(100),
        ),
        // drv8235.py writes 0x01 here: it stores -255 in the register without masking, so
        // full reverse becomes almost off. The Rust driver writes the intended 0xFF.
        (
            Set::Raw(Some(-300)),
            &[(0x0F, 0xFF), (0x0D, 0x0D)],
            Some(-6.3333333),
            Some(-42.66915),
            Some(-255),
        ),
        (
            Set::Raw(Some(0)),
            &[(0x0F, 0x00), (0x0D, 0x0F)],
            Some(0.0),
            Some(0.0),
            Some(0),
        ),
    ];

    let mut regs = FakeRegs::new(&[]);
    let log = regs.log.clone();
    let mut drv = DRV8235::new(&mut regs, ADDR);
    drv.init().await.unwrap();
    take_events(&log);

    for (set, expected_writes, throttle, volts, raw) in cases {
        let label = match set {
            Set::Throttle(v) => {
                drv.set_throttle(v).await.unwrap();
                format!("set_throttle({v:?})")
            }
            Set::Volts(v) => {
                drv.set_throttle_volts(v).await.unwrap();
                format!("set_throttle_volts({v:?})")
            }
            Set::Raw(v) => {
                drv.set_throttle_raw(v).await.unwrap();
                format!("set_throttle_raw({v:?})")
            }
        };
        assert_eq!(take_events(&log), writes(expected_writes), "{label} writes");
        assert_close(
            &format!("{label} -> throttle()"),
            drv.throttle().await.unwrap(),
            throttle,
        );
        assert_close(
            &format!("{label} -> throttle_volts()"),
            drv.throttle_volts().await.unwrap(),
            volts,
        );
        assert_eq!(
            drv.throttle_raw().await.unwrap(),
            raw,
            "{label} -> throttle_raw()"
        );
        assert!(
            take_events(&log).is_empty(),
            "{label}: getters must not write"
        );
    }
}

#[tokio::test]
async fn reads_voltage_and_current() {
    let mut regs = FakeRegs::new(&[(0x04, 100), (0x05, 50)]);
    let (volts, amps) = DRV8235::new(&mut regs, ADDR)
        .read_voltage_current()
        .await
        .unwrap();
    assert_close("volts", Some(volts), Some(16.733));
    assert_close("amps", Some(amps), Some(0.7255));
}

#[tokio::test]
async fn faults_match_flight_driver_and_clear_only_when_reported() {
    // (FAULT_STATUS, expected faults, expected writes); CONFIG0 is 0x82 after init
    let cases: [(u8, Faults, &[(u8, u8)]); 3] = [
        (
            0b1011_0000,
            Faults {
                stall: true,
                overcurrent: true,
                ..Faults::default()
            },
            &[(0x09, 0x82)],
        ),
        // Flags without the FAULT bit are ignored, as in drv8235.py
        (0b0011_0000, Faults::default(), &[]),
        (
            0b1000_1110,
            Faults {
                overvoltage: true,
                thermal_shutdown: true,
                undervoltage_lockout: true,
                ..Faults::default()
            },
            &[(0x09, 0x82)],
        ),
    ];
    for (status, expected, expected_writes) in cases {
        let mut regs = FakeRegs::new(&[(0x00, status), (0x09, 0x82)]);
        let log = regs.log.clone();
        let faults = DRV8235::new(&mut regs, ADDR).faults().await.unwrap();
        assert_eq!(faults, expected, "status {status:#010b}");
        assert_eq!(
            faults.any(),
            expected != Faults::default(),
            "status {status:#010b} any()"
        );
        assert_eq!(
            take_events(&log),
            writes(expected_writes),
            "status {status:#010b} writes"
        );
    }
}
