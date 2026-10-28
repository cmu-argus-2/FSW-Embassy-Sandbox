// Expected values were generated independently in Python: the BCD register bytes and
// epoch conversions with the datetime module, and the temperatures from the datasheet's
// 12-bit two's complement format at 1/16 C per count.
use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTransaction};
use fsw_lib::drivers::rv3032::{self, DateTime, RV3032};

const ADDR: u8 = 0x51;

#[tokio::test]
async fn set_unix_time_writes_bcd_and_clears_only_the_power_flags() {
    let expectations = [
        // 2025-09-14T17:13:20Z: seconds, minutes, hours, weekday, date, month, year
        I2cTransaction::write(ADDR, vec![0x01, 0x20, 0x13, 0x17, 0x01, 0x14, 0x09, 0x25]),
        // Status has PORF and VLF set plus an unrelated flag (UF), which must survive
        I2cTransaction::write_read(ADDR, vec![0x0D], vec![0b0010_0011]),
        I2cTransaction::write(ADDR, vec![0x0D, 0b0010_0000]),
    ];
    let mut i2c = I2cMock::new(&expectations);

    let res = RV3032::new(&mut i2c, ADDR)
        .set_unix_time(1_757_870_000)
        .await;

    assert!(res.is_ok(), "got {res:?}");
    i2c.done();
}

#[tokio::test]
async fn unix_time_reads_epoch() {
    // (clock registers 0x01-0x07, expected epoch)
    let cases: [([u8; 7], i64); 3] = [
        ([0x20, 0x13, 0x17, 0x01, 0x14, 0x09, 0x25], 1_757_870_000), // 2025-09-14T17:13:20Z
        ([0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x00], 946_684_800),   // 2000-01-01T00:00:00Z
        ([0x59, 0x59, 0x23, 0x01, 0x31, 0x12, 0x99], 4_102_444_799), // 2099-12-31T23:59:59Z
    ];
    for (bytes, expected) in cases {
        let mut i2c = I2cMock::new(&[I2cTransaction::write_read(ADDR, vec![0x01], bytes.to_vec())]);
        let ts = RV3032::new(&mut i2c, ADDR).unix_time().await;
        assert!(
            matches!(ts, Ok(t) if t == expected),
            "{bytes:02X?}: got {ts:?}, want {expected}"
        );
        i2c.done();
    }
}

#[tokio::test]
async fn datetime_ignores_the_bits_the_chip_does_not_use() {
    // Bit 7 of seconds and minutes, and the upper bits of hours/date/month, read as 0
    // on this part, but mask them anyway so a stray bit cannot corrupt the time
    let raw = [0xA0, 0x93, 0x57, 0x0F, 0xD4, 0xE9, 0x25];
    let mut i2c = I2cMock::new(&[I2cTransaction::write_read(ADDR, vec![0x01], raw.to_vec())]);

    let dt = RV3032::new(&mut i2c, ADDR).datetime().await.unwrap();

    assert_eq!(
        (dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second),
        (2025, 9, 14, 17, 13, 20)
    );
    i2c.done();
}

#[tokio::test]
async fn rejects_times_the_chip_cannot_store() {
    // 1999-12-31T23:59:59Z is before 2000, so nothing should be written
    let mut i2c = I2cMock::new(&[]);
    let res = RV3032::new(&mut i2c, ADDR).set_unix_time(946_684_799).await;
    assert!(
        matches!(res, Err(rv3032::Error::InvalidTime)),
        "got {res:?}"
    );
    i2c.done();

    // And 2100 is past the two-digit year
    let mut i2c = I2cMock::new(&[]);
    let too_late = DateTime {
        year: 2100,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
    };
    let res = RV3032::new(&mut i2c, ADDR).set_datetime(&too_late).await;
    assert!(
        matches!(res, Err(rv3032::Error::InvalidTime)),
        "got {res:?}"
    );
    i2c.done();
}

#[tokio::test]
async fn unix_time_rejects_an_impossible_stored_date() {
    // Month 0, as an uninitialised chip would read
    let mut i2c = I2cMock::new(&[I2cTransaction::write_read(
        ADDR,
        vec![0x01],
        vec![0, 0, 0, 1, 0x01, 0x00, 0x00],
    )]);
    let ts = RV3032::new(&mut i2c, ADDR).unix_time().await;
    assert!(matches!(ts, Err(rv3032::Error::InvalidTime)), "got {ts:?}");
    i2c.done();
}

#[tokio::test]
async fn lost_power_reports_either_flag() {
    // (status byte, expected)
    let cases = [
        (0x00, false),
        (0b0000_0010, true),
        (0b0000_0001, true),
        (0b1000_0000, false),
    ];
    for (status, expected) in cases {
        let mut i2c = I2cMock::new(&[I2cTransaction::write_read(ADDR, vec![0x0D], vec![status])]);
        let lost = RV3032::new(&mut i2c, ADDR).lost_power().await.unwrap();
        assert_eq!(lost, expected, "status {status:#010b}");
        i2c.done();
    }
}
