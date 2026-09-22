// Expected values were generated independently in Python: the OPT4003 CRC from the
// datasheet formula, and the DS3231 dates from Python's datetime module.
use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTransaction};
use fsw_lib::drivers::ds3231::{self, DS3231};
use fsw_lib::drivers::opt4003::{self, OPT4003};

const OPT_ADDR: u8 = 0x44;
const RTC_ADDR: u8 = 0x68;

/// Flags register with only CONV_READY set, then the 4 result bytes.
fn opt4003_read(result: [u8; 4]) -> Vec<I2cTransaction> {
    vec![
        I2cTransaction::write_read(OPT_ADDR, vec![0x0C], vec![0x00, 0x04]),
        I2cTransaction::write_read(OPT_ADDR, vec![0x00], result.to_vec()),
    ]
}

#[tokio::test]
async fn opt4003_accepts_valid_crc() {
    // (result bytes, expected lux)
    let cases: [([u8; 4], f32); 3] = [
        ([0x31, 0x23, 0x45, 0x95], 319.1382), // E=3, R=0x12345, C=9, CRC=0x5
        ([0x00, 0x00, 0x01, 0x01], 0.000535), // E=0, R=0x00001, C=0, CRC=0x1
        ([0x8F, 0xFF, 0xFF, 0xFF], 143612.832), // E=8, R=0xFFFFF, C=15, CRC=0xF
    ];
    for (bytes, expected) in cases {
        let mut i2c = I2cMock::new(&opt4003_read(bytes));
        let lux = OPT4003::new(&mut i2c, OPT_ADDR).lux().await;
        match lux {
            Ok(lux) => assert!(
                (lux - expected).abs() <= expected * 1e-4,
                "{bytes:02X?}: got {lux}, want {expected}"
            ),
            Err(e) => panic!("{bytes:02X?}: unexpected error {e:?}"),
        }
        i2c.done();
    }
}

#[tokio::test]
async fn opt4003_rejects_bad_crc() {
    // Same as the first valid case, with the CRC nibble changed from 0x5 to 0x4
    let mut i2c = I2cMock::new(&opt4003_read([0x31, 0x23, 0x45, 0x94]));
    let lux = OPT4003::new(&mut i2c, OPT_ADDR).lux().await;
    assert!(
        matches!(lux, Err(opt4003::Error::CrcMismatch)),
        "got {lux:?}"
    );
    i2c.done();
}

#[tokio::test]
async fn ds3231_set_unix_time_only_clears_eosc_and_osf() {
    let expectations = [
        // 2025-09-14T17:13:20Z = 1757870000
        I2cTransaction::write(
            RTC_ADDR,
            vec![0x00, 0x20, 0x13, 0x17, 0x01, 0x14, 0x09, 0x25],
        ),
        // CONTROL 0x9C (EOSC set, INTCN + rate select set) -> only EOSC cleared
        I2cTransaction::write_read(RTC_ADDR, vec![0x0E], vec![0x9C]),
        I2cTransaction::write(RTC_ADDR, vec![0x0E, 0x1C]),
        // STATUS 0x88 (OSF + EN32kHz set) -> only OSF cleared
        I2cTransaction::write_read(RTC_ADDR, vec![0x0F], vec![0x88]),
        I2cTransaction::write(RTC_ADDR, vec![0x0F, 0x08]),
    ];
    let mut i2c = I2cMock::new(&expectations);
    let res = DS3231::new(&mut i2c, RTC_ADDR)
        .set_unix_time(1_757_870_000)
        .await;
    assert!(res.is_ok(), "got {res:?}");
    i2c.done();
}

#[tokio::test]
async fn ds3231_unix_time_reads_epoch() {
    // (register bytes 0x00-0x06, expected epoch)
    let cases: [([u8; 7], i64); 2] = [
        ([0x20, 0x13, 0x17, 0x01, 0x14, 0x09, 0x25], 1_757_870_000), // 2025-09-14T17:13:20Z
        ([0x59, 0x59, 0x23, 0x05, 0x31, 0x12, 0x99], 4_102_444_799), // 2099-12-31T23:59:59Z
    ];
    for (bytes, expected) in cases {
        let mut i2c = I2cMock::new(&[I2cTransaction::write_read(
            RTC_ADDR,
            vec![0x00],
            bytes.to_vec(),
        )]);
        let ts = DS3231::new(&mut i2c, RTC_ADDR).unix_time().await;
        assert!(
            matches!(ts, Ok(t) if t == expected),
            "{bytes:02X?}: got {ts:?}, want {expected}"
        );
        i2c.done();
    }
}

#[tokio::test]
async fn ds3231_rejects_times_it_cannot_store() {
    // 1999-12-31T23:59:59Z is before 2000; nothing should be written to the chip
    let mut i2c = I2cMock::new(&[]);
    let res = DS3231::new(&mut i2c, RTC_ADDR)
        .set_unix_time(946_684_799)
        .await;
    assert!(
        matches!(res, Err(ds3231::Error::InvalidTime)),
        "got {res:?}"
    );
    i2c.done();
}

#[tokio::test]
async fn ds3231_unix_time_rejects_invalid_register_date() {
    // Month 0 is not a real date (e.g. an uninitialized chip)
    let mut i2c = I2cMock::new(&[I2cTransaction::write_read(
        RTC_ADDR,
        vec![0x00],
        vec![0, 0, 0, 1, 0x01, 0x00, 0x00],
    )]);
    let ts = DS3231::new(&mut i2c, RTC_ADDR).unix_time().await;
    assert!(matches!(ts, Err(ds3231::Error::InvalidTime)), "got {ts:?}");
    i2c.done();
}
