// Expected values were computed independently in Python from the datasheet scaling
// the driver uses: voltage = raw * 26.35/4096, current = raw * (0.10584/4096) / 0.01,
// with raw_voltage = ((buf[0] << 8) | (buf[2] & 0xF0)) >> 4 and
// raw_current = (buf[1] << 4) | (buf[2] & 0x0F).
use embedded_hal_async::i2c::{ErrorKind, NoAcknowledgeSource};
use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTransaction};
use fsw_lib::drivers::adm1176::ADM1176;

const ADDR: u8 = 0x40;

fn close(got: f32, want: f32, label: &str) {
    assert!(
        (got - want).abs() <= 1e-4 * want.abs().max(1.0),
        "{label}: got {got}, want {want}"
    );
}

#[tokio::test]
async fn decodes_voltage_and_current() {
    // (reading, volts, amps)
    let cases: [([u8; 3], f32, f32); 3] = [
        ([0x80, 0x40, 0x00], 13.175, 2.646),
        ([0x00, 0xFF, 0x0F], 0.0, 10.581416),
        ([0x12, 0x34, 0x56], 1.8849, 2.165379),
    ];

    for (reading, volts, amps) in cases {
        let mut i2c = I2cMock::new(&[I2cTransaction::read(ADDR, reading.to_vec())]);
        {
            let mut sensor = ADM1176::new(&mut i2c, ADDR);
            let (got_v, got_i) = sensor.read_voltage_current().await.unwrap();
            close(got_v, volts, "voltage");
            close(got_i, amps, "current");
        }
        i2c.done();
    }
}

#[tokio::test]
async fn current_keeps_the_high_bits_of_the_reading() {
    // The current register's top bits live in buf[1]. Shifting a u8 left by four
    // before widening drops them, which reads most real currents as zero:
    // buf[1] = 0x40 should give raw 1024, not 0.
    let mut i2c = I2cMock::new(&[I2cTransaction::read(ADDR, vec![0x00, 0x40, 0x00])]);
    {
        let mut sensor = ADM1176::new(&mut i2c, ADDR);
        let (_, current) = sensor.read_voltage_current().await.unwrap();
        close(current, 2.646, "current with high bits set");
        assert!(
            current > 0.0,
            "a reading of 0x40 must not decode as no current"
        );
    }
    i2c.done();
}

#[tokio::test]
async fn reports_bus_errors_rather_than_a_reading() {
    let mut i2c = I2cMock::new(&[I2cTransaction::read(ADDR, vec![0x00, 0x00, 0x00])
        .with_error(ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address))]);
    {
        let mut sensor = ADM1176::new(&mut i2c, ADDR);
        assert!(sensor.read_voltage_current().await.is_err());
    }
    i2c.done();
}
