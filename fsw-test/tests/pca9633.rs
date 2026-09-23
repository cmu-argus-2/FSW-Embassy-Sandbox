// Expected register writes were recorded by running the flight driver
// (FSW-mainboard flight/hal/drivers/pca9633.py) against a simulated register map.
mod common;

use common::{Event, FakeDelay, FakeRegs, take_events, writes};
use fsw_lib::drivers::pca9633::{self, PCA9633};

const ADDR: u8 = 0x60;
// Power-on MODE1, MODE2 and LEDOUT
const POWER_ON: &[(u8, u8)] = &[(0x00, 0x91), (0x01, 0x05), (0x08, 0x00)];

#[tokio::test]
async fn matches_flight_driver_register_writes() {
    let mut regs = FakeRegs::new(POWER_ON);
    let log = regs.log.clone();
    let mut delay = FakeDelay { log: log.clone() };
    let mut pca = PCA9633::new(&mut regs, ADDR);

    pca.init().await.unwrap();
    assert_eq!(
        take_events(&log),
        writes(&[
            (0x00, 0x81),
            (0x01, 0x05),
            (0x08, 0x80),
            (0x05, 0xFF),
            (0x08, 0x40),
            (0x08, 0x42),
            (0x02, 0xFF),
            (0x08, 0x41),
            (0x08, 0x49),
            (0x03, 0xFF),
            (0x08, 0x45),
            (0x08, 0x65),
            (0x04, 0xFF),
            (0x08, 0x55),
        ]),
        "init"
    );

    pca.set_pwm(1, 200).await.unwrap();
    assert_eq!(
        take_events(&log),
        writes(&[(0x08, 0x59), (0x03, 0x37)]),
        "set_pwm(1, 200)"
    );

    pca.enable_driver().await.unwrap();
    assert_eq!(
        take_events(&log),
        writes(&[(0x08, 0x99), (0x05, 0x00), (0x00, 0x81)]),
        "enable_driver"
    );

    pca.disable_driver().await.unwrap();
    assert_eq!(
        take_events(&log),
        writes(&[
            (0x08, 0x99),
            (0x05, 0xFF),
            (0x08, 0x9A),
            (0x02, 0xFF),
            (0x08, 0x9A),
            (0x03, 0xFF),
            (0x08, 0xAA),
            (0x04, 0xFF),
            (0x00, 0x91),
        ]),
        "disable_driver"
    );

    // burn = set_pwm + enable_driver, wait, disable_driver
    pca.burn(2, 10, 1500, &mut delay).await.unwrap();
    let mut expected = writes(&[
        (0x08, 0xAA),
        (0x04, 0xF5),
        (0x08, 0xAA),
        (0x05, 0x00),
        (0x00, 0x81),
    ]);
    expected.push(Event::DelayMs(1500));
    expected.extend(writes(&[
        (0x08, 0xAA),
        (0x05, 0xFF),
        (0x08, 0xAA),
        (0x02, 0xFF),
        (0x08, 0xAA),
        (0x03, 0xFF),
        (0x08, 0xAA),
        (0x04, 0xFF),
        (0x00, 0x91),
    ]));
    assert_eq!(take_events(&log), expected, "burn(2, 10, 1500ms)");
}

#[tokio::test]
async fn burn_disables_driver_even_if_enable_fails() {
    // Chip awake with nothing burning
    let mut regs = FakeRegs::new(&[(0x00, 0x81), (0x01, 0x05), (0x08, 0x55)]);
    regs.fail_writes_to = Some(0x05); // PWM3, the supply enable channel
    let log = regs.log.clone();
    let mut delay = FakeDelay { log: log.clone() };

    let result = PCA9633::new(&mut regs, ADDR)
        .burn(0, 50, 1000, &mut delay)
        .await;

    assert!(
        matches!(result, Err(pca9633::Error::I2c(_))),
        "got {result:?}"
    );
    assert!(
        !take_events(&log).contains(&Event::DelayMs(1000)),
        "must not wait after a failed enable"
    );
    assert_eq!(
        &regs.regs[0x02..=0x04],
        &[0xFF, 0xFF, 0xFF],
        "burn wire channels off"
    );
    assert_eq!(regs.regs[0x00] & 0x10, 0x10, "chip put to sleep");
}

#[tokio::test]
async fn rejects_invalid_channels_without_writing() {
    let mut regs = FakeRegs::new(POWER_ON);
    let log = regs.log.clone();
    let mut delay = FakeDelay { log: log.clone() };
    let mut pca = PCA9633::new(&mut regs, ADDR);

    let res = pca.set_pwm(4, 1).await;
    assert!(
        matches!(res, Err(pca9633::Error::InvalidChannel)),
        "set_pwm(4): got {res:?}"
    );
    // Channel 3 is the supply enable, not a burn wire
    let res = pca.burn(3, 1, 1, &mut delay).await;
    assert!(
        matches!(res, Err(pca9633::Error::InvalidChannel)),
        "burn(3): got {res:?}"
    );
    assert_eq!(take_events(&log), vec![]);
}
