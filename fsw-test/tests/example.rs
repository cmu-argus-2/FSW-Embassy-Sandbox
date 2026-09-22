#[tokio::test]
async fn driver() {
    // Configure expectations
    use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTransaction};
    use fsw_lib::drivers::adm1176::ADM1176;

    // Simulate a successful reading: [Voltage MSB, Current MSB, Combined LSB]
    let expectations = [I2cTransaction::read(0x40, vec![0x80, 0x40, 0x00])];

    let mut i2c = I2cMock::new(&expectations);
    {
        let mut sensor = ADM1176::new(&mut i2c, 0x40);

        match sensor.read_voltage_current().await {
            Ok((voltage, current)) => {
                println!(
                    "Mock Test - Voltage: {:.2}V, Current: {:.2}A",
                    voltage, current
                );
                assert!(voltage > 0.0);
            }
            Err(e) => {
                panic!("Test Failed: {:?}", e);
            }
        }
    }

    // Tell the mock that we are finished and it should verify the transactions
    i2c.done();
}
