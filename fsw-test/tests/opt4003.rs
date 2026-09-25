// Expected values come from the CRC formula in the OPT4003 datasheet, computed
// independently in Python.
use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTransaction};
use fsw_lib::drivers::opt4003::{self, OPT4003};

const OPT_ADDR: u8 = 0x44;

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
