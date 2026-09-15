use embedded_hal_async::i2c::I2c;

pub struct MAX17205<I2C: I2c> {
    i2c: I2C,
    // 2 I2C addresses: read address (0x36) and write address (0x0B, shadow RAM)
    // Only using read address
    read_addr: u8,
    write_addr: u8
}

impl<I2C: I2c> MAX17205<I2C> {
    pub fn new(i2c: I2C, read_addr: u8, write_addr: u8) -> Self {
        Self {
            i2c: i2c,
            read_addr: read_addr,
            write_addr: write_addr
        }
    }
    
}