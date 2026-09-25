use embedded_hal_async::i2c::I2c;

const VCELL_ADDR: u8 = 0x09; // Lowest cell voltage of a pack
const REPSOC_ADDR: u8 = 0x06; // Reported state of charge
const REPCAP_ADDR: u8 = 0x05; // Reported remaining capacity
const CURRENT_ADDR: u8 = 0x0A; // Battery current
const TTE_ADDR: u8 = 0x11; // Time to empty
const TTF_ADDR: u8 = 0x20; // Time to full
const CAPACITY_ADDR: u8 = 0x10; // Full capacity estimation
const VBAT_ADDR: u8 = 0xDA; // Battery pack voltage
const AVCELL_ADDR: u8 = 0x17; // Battery cycles
const TIMERH_ADDR: u8 = 0xBE; // Time since power up
const TEMP_ADDR: u8 = 0x08; // Temp register

const CONFIG2_ADDR: u8 = 0xBB; // Command register

// Addresses in shadow RAM (I2C address 0x0B)
const TEMP1_ADDR: u8 = 0x34; // AIN1 thermistor temperature
const TEMP2_ADDR: u8 = 0x3B; // AIN2 thermistor temperature
const INTTEMP_ADDR: u8 = 0x35; // Internal die temperature

fn unpack_signed_short_int(buf: [u8; 2]) -> i16 {
    let val = ((buf[1] as u16) << 8) | (buf[0] as u16);
    val as i16
}

#[derive(Debug, defmt::Format)]

pub struct MAX17205<I2C: I2c> {
    pub i2c: I2C,
    // 2 I2C addresses: read address (0x36) and write address (0x0B, shadow RAM)
    // Only using read address
    read_addr: u8,
    write_addr: u8,
}

impl<I2C: I2c> MAX17205<I2C> {
    pub fn new(i2c: I2C, read_addr: u8, write_addr: u8) -> Self {
        Self {
            i2c: i2c,
            read_addr: read_addr,
            write_addr: write_addr,
        }
    }

    async fn read_reg(&mut self, addr: u8, reg: u8) -> Result<[u8; 2], I2C::Error> {
        let mut buf = [0u8; 2];
        self.i2c.write(addr, &[reg]).await?;
        self.i2c.read(addr, &mut buf).await?;
        Ok(buf)
    }

    /// Reads SoC from the battery pack.
    ///
    /// Returns SoC as a percentage.
    pub async fn read_soc(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.read_addr, REPSOC_ADDR).await?;
        let raw = u16::from_le_bytes(buf);
        Ok(raw as f32 / 256.0)
    }

    /// Reads capacity from the battery pack.
    ///
    /// Returns capacity in mAh.
    pub async fn read_capacity(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.read_addr, REPCAP_ADDR).await?;
        let raw = u16::from_le_bytes(buf);
        Ok(raw as f32 * 0.5)
    }

    /// Reads the current from the battery pack.
    ///
    /// Returns current in mA.
    pub async fn read_current(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.read_addr, CURRENT_ADDR).await?;
        let raw = unpack_signed_short_int(buf);
        Ok(raw as f32 * 0.0015625 / 0.01)
    }

    /// Reads the voltage for the battery pack.
    ///
    /// Returns voltage in mV.
    pub async fn read_voltage(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.read_addr, VBAT_ADDR).await?;
        let raw = u16::from_le_bytes(buf);
        Ok(raw as f32 * 1.25)
    }

    /// Reads the midpoint voltage for the battery pack.
    ///
    /// Returns voltage in mV.
    pub async fn read_midvoltage(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.read_addr, VCELL_ADDR).await?;
        let raw = u16::from_le_bytes(buf);
        Ok(raw as f32 * 0.078125)
    }

    /// Reads the battery cycles for the battery pack.
    pub async fn read_cycles(&mut self) -> Result<u16, I2C::Error> {
        let buf = self.read_reg(self.read_addr, AVCELL_ADDR).await?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Reads the time-to-empty for the battery pack.
    ///
    /// Returns time-to-empty in seconds.
    pub async fn read_tte(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.read_addr, TTE_ADDR).await?;
        let raw = u16::from_le_bytes(buf);
        Ok(raw as f32 * 5.625)
    }

    /// Reads the time-to-full for the battery pack.
    ///
    /// Returns time-to-full in seconds.
    pub async fn read_ttf(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.read_addr, TTF_ADDR).await?;
        let raw = u16::from_le_bytes(buf);
        Ok(raw as f32 * 5.625)
    }

    /// Reads the time since power up for the battery pack.
    ///
    /// Returns time since power up in seconds.
    pub async fn read_time_pwrup(&mut self) -> Result<u16, I2C::Error> {
        let buf = self.read_reg(self.read_addr, TIMERH_ADDR).await?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Reads the temperature of the battery pack.
    ///
    /// Returns temperature in centi-Celsius.
    pub async fn read_temperature(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.read_addr, TEMP_ADDR).await?;
        let raw = u16::from_le_bytes(buf);
        Ok(raw as f32 * 0.390625)
    }

    /// Reads the temperature of thermistor set 1.
    ///
    /// Returns temperature in centi-Celsius.
    pub async fn read_temperature_ain1(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.write_addr, TEMP1_ADDR).await?;
        let raw = u16::from_le_bytes(buf);
        Ok((raw as f32 - 2731.0) * 10.0)
    }

    /// Reads the temperature of thermistor set 2.
    ///
    /// Returns temperature in centi-Celsius.
    pub async fn read_temperature_ain2(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.write_addr, TEMP2_ADDR).await?;
        let raw = u16::from_le_bytes(buf);
        Ok((raw as f32 - 2731.0) * 10.0)
    }

    /// Reads the temperature of the MAX17205 die.
    ///
    /// Returns temperature in centi-Celsius.
    pub async fn read_temperature_die(&mut self) -> Result<f32, I2C::Error> {
        let buf = self.read_reg(self.write_addr, INTTEMP_ADDR).await?;
        let raw = u16::from_le_bytes(buf);
        Ok((raw as f32 - 2731.0) * 10.0)
    }

    /// Resets the fuel gauge IC.
    pub async fn reset(&mut self) -> Result<(), I2C::Error> {
        self.i2c
            .write(self.read_addr, &[CONFIG2_ADDR, 0x01, 0x00])
            .await
    }
}
