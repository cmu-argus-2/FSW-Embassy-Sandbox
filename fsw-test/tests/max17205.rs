use embedded_hal_mock::eh1::i2c::{Mock as I2cMock, Transaction as I2cTransaction};
use fsw_lib::drivers::max17205::MAX17205 as max17205;

const READ_ADDR: u8 = 0x36;
const WRITE_ADDR: u8 = 0x0B;

#[tokio::test]
async fn read_soc() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(READ_ADDR, vec![0x06]),
        I2cTransaction::read(READ_ADDR, vec![0x00, 0x50]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_soc().await {
        Ok(soc) => {
            println!("soc {} \n", soc);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_capacity() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(READ_ADDR, vec![0x05]),
        I2cTransaction::read(READ_ADDR, vec![0x00, 0x10]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_capacity().await {
        Ok(capacity) => {
            println!("capacity {} \n", capacity);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_current() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(READ_ADDR, vec![0x0A]),
        I2cTransaction::read(READ_ADDR, vec![0x00, 0xFF]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_current().await {
        Ok(current) => {
            println!("current {} \n", current);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_voltage() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(READ_ADDR, vec![0xDA]),
        I2cTransaction::read(READ_ADDR, vec![0xE8, 0x03]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_voltage().await {
        Ok(voltage) => {
            println!("voltage {} \n", voltage);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_midvoltage() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(READ_ADDR, vec![0x09]),
        I2cTransaction::read(READ_ADDR, vec![0x00, 0x10]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_midvoltage().await {
        Ok(midvoltage) => {
            println!("midvoltage {} \n", midvoltage);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_cycles() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(READ_ADDR, vec![0x17]),
        I2cTransaction::read(READ_ADDR, vec![0x05, 0x00]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_cycles().await {
        Ok(cycles) => {
            println!("cycles {} \n", cycles);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_tte() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(READ_ADDR, vec![0x11]),
        I2cTransaction::read(READ_ADDR, vec![0x64, 0x00]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_tte().await {
        Ok(tte) => {
            println!("tte {} \n", tte);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_ttf() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(READ_ADDR, vec![0x20]),
        I2cTransaction::read(READ_ADDR, vec![0xC8, 0x00]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_ttf().await {
        Ok(ttf) => {
            println!("ttf {} \n", ttf);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_time_pwrup() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(READ_ADDR, vec![0xBE]),
        I2cTransaction::read(READ_ADDR, vec![0x10, 0x27]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_time_pwrup().await {
        Ok(time_pwrup) => {
            println!("time_pwrup {} \n", time_pwrup);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_temperature() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(READ_ADDR, vec![0x08]),
        I2cTransaction::read(READ_ADDR, vec![0x00, 0x01]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_temperature().await {
        Ok(temperature) => {
            println!("temperature {} \n", temperature);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_temperature_ain1() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(WRITE_ADDR, vec![0x34]),
        I2cTransaction::read(WRITE_ADDR, vec![0x0F, 0x0B]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_temperature_ain1().await {
        Ok(temperature_ain1) => {
            println!("temperature_ain1 {} \n", temperature_ain1);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_temperature_ain2() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(WRITE_ADDR, vec![0x3B]),
        I2cTransaction::read(WRITE_ADDR, vec![0x79, 0x0A]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_temperature_ain2().await {
        Ok(temperature_ain2) => {
            println!("temperature_ain2 {} \n", temperature_ain2);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn read_temperature_die() {
    // Configure expectations
    let expectations = [
        I2cTransaction::write(WRITE_ADDR, vec![0x35]),
        I2cTransaction::read(WRITE_ADDR, vec![0x73, 0x0B]),
    ];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.read_temperature_die().await {
        Ok(temperature_die) => {
            println!("temperature_die {} \n", temperature_die);
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}

#[tokio::test]
async fn reset() {
    // Configure expectations
    let expectations = [I2cTransaction::write(READ_ADDR, vec![0xBB, 0x01, 0x00])];
    let i2c = I2cMock::new(&expectations);
    let mut sensor = max17205::new(i2c, READ_ADDR, WRITE_ADDR);
    match sensor.reset().await {
        Ok(()) => {
            println!("reset ok \n");
        }
        Err(e) => {
            println!("{}", e);
        }
    }
    let mut i2c = sensor.i2c;
    i2c.done();
    assert!(true);
}
