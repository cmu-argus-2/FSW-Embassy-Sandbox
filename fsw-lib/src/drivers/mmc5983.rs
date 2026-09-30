use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;

/*
 * MMC5983MA 3-Axis Magnetometer Driver
 *
 * AMR magnetometer on I2C at 0x30, +/-8 gauss, 18-bit output (16384 counts/gauss)
 * with the no-field reading at 131072 counts.
 *
 * The sensing elements can be flipped by a strong external field, and their bridge
 * offset drifts with temperature, so the part provides SET and RESET coil pulses to
 * re-magnetise them. A measurement after SET reads +H + offset and one after RESET
 * reads -H + offset, so a pair cancels the offset: see field_offset_cancelled().
 */

pub const DEFAULT_ADDRESS: u8 = 0x30;
/// Value the product ID register returns on a healthy part
pub const PRODUCT_ID: u8 = 0x30;

mod regs {
    pub const XOUT0: u8 = 0x00; // 7 bytes: X/Y/Z high, mid, then the packed low bits
    pub const TOUT: u8 = 0x07;
    pub const STATUS: u8 = 0x08;
    pub const CONTROL0: u8 = 0x09;
    pub const CONTROL1: u8 = 0x0A;
    pub const PRODUCT_ID: u8 = 0x2F;
}

mod bits {
    // Status (0x08)
    pub const MEAS_M_DONE: u8 = 1 << 0;
    pub const MEAS_T_DONE: u8 = 1 << 1;

    // Internal control 0 (0x09), write only, bits self-clear
    pub const TM_M: u8 = 1 << 0; // take a magnetic measurement
    pub const TM_T: u8 = 1 << 1; // take a temperature measurement
    pub const SET: u8 = 1 << 3;
    pub const RESET: u8 = 1 << 4;
    pub const AUTO_SR_EN: u8 = 1 << 5;

    // Internal control 1 (0x0A), write only
    pub const BANDWIDTH: u8 = 0b11;
    pub const SW_RST: u8 = 1 << 7;
}

/// 18-bit mode: counts per gauss, and the reading with no field applied
const COUNTS_PER_GAUSS: f32 = 16384.0;
const NULL_FIELD: u32 = 131_072;

/// Temperature output is unsigned, 0 counts is -75 C and each count is 0.8 C
const TEMP_AT_ZERO_COUNTS: f32 = -75.0;
const TEMP_PER_COUNT: f32 = 0.8;

/// A SET or RESET coil pulse lasts 500ns; allow a whole millisecond for it to finish
const COIL_PULSE_MS: u32 = 1;
/// Software reset re-reads OTP, which the datasheet gives as a 10ms power-on time
const RESET_MS: u32 = 10;
/// Measurement poll interval, and how many polls before giving up
const POLL_MS: u32 = 1;
const POLL_LIMIT: u32 = 25;

/// Measurement time and bandwidth of the decimation filter. Faster settings are
/// noisier: 0.4 mG RMS at 100Hz against 1.2 mG at 800Hz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum Bandwidth {
    /// 8ms per measurement
    Hz100 = 0b00,
    /// 4ms per measurement
    Hz200 = 0b01,
    /// 2ms per measurement
    Hz400 = 0b10,
    /// 0.5ms per measurement
    Hz800 = 0b11,
}

#[derive(Debug, defmt::Format)]
pub enum Error<E> {
    I2c(E),
    /// Product ID register did not read 0x30, so this is not an MMC5983MA
    BadId(u8),
    /// The measurement never reported done
    Timeout,
}

/// Raw 18-bit counts, as the registers hold them (unsigned, no-field is 131072)
#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct Raw {
    pub x: u32,
    pub y: u32,
    pub z: u32,
}

/// Magnetic field in gauss
#[derive(Debug, Clone, Copy, defmt::Format)]
pub struct Field {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Raw {
    /// Counts to gauss, without removing the bridge offset
    pub fn to_field(self) -> Field {
        Field {
            x: counts_to_gauss(self.x),
            y: counts_to_gauss(self.y),
            z: counts_to_gauss(self.z),
        }
    }
}

fn counts_to_gauss(counts: u32) -> f32 {
    (counts as f32 - NULL_FIELD as f32) / COUNTS_PER_GAUSS
}

/// Field from a SET/RESET pair, with the bridge offset removed.
///
/// The SET measurement reads +H + offset and the RESET measurement -H + offset,
/// so H = (set - reset) / 2 and the offset itself is (set + reset) / 2.
pub fn compensate(set: Raw, reset: Raw) -> Field {
    let halve = |a: u32, b: u32| (a as f32 - b as f32) / (2.0 * COUNTS_PER_GAUSS);
    Field {
        x: halve(set.x, reset.x),
        y: halve(set.y, reset.y),
        z: halve(set.z, reset.z),
    }
}

/// Bridge offset in gauss from the same SET/RESET pair, which can be stored and
/// subtracted from later single measurements instead of pulsing the coil each time.
pub fn offset(set: Raw, reset: Raw) -> Field {
    let mean =
        |a: u32, b: u32| ((a as f32 + b as f32) / 2.0 - NULL_FIELD as f32) / COUNTS_PER_GAUSS;
    Field {
        x: mean(set.x, reset.x),
        y: mean(set.y, reset.y),
        z: mean(set.z, reset.z),
    }
}

pub struct MMC5983<I2C: I2c> {
    i2c: I2C,
    addr: u8,
}

impl<I2C: I2c> MMC5983<I2C> {
    pub fn new(i2c: I2C, addr: u8) -> Self {
        Self { i2c, addr }
    }

    /// Check the part answers as an MMC5983MA and set the measurement bandwidth.
    pub async fn init<D: DelayNs>(
        &mut self,
        bandwidth: Bandwidth,
        delay: &mut D,
    ) -> Result<(), Error<I2C::Error>> {
        let id = self.product_id().await?;
        if id != PRODUCT_ID {
            return Err(Error::BadId(id));
        }
        self.set_bandwidth(bandwidth).await?;
        // Start from a known magnetisation rather than whatever the last field left
        self.perform_set(delay).await
    }

    pub async fn product_id(&mut self) -> Result<u8, Error<I2C::Error>> {
        self.read_reg(regs::PRODUCT_ID).await
    }

    pub async fn set_bandwidth(&mut self, bandwidth: Bandwidth) -> Result<(), Error<I2C::Error>> {
        self.write_reg(regs::CONTROL1, bandwidth as u8 & bits::BANDWIDTH)
            .await
    }

    /// Reset the part, clearing every register and re-reading its OTP.
    pub async fn software_reset<D: DelayNs>(
        &mut self,
        delay: &mut D,
    ) -> Result<(), Error<I2C::Error>> {
        self.write_reg(regs::CONTROL1, bits::SW_RST).await?;
        delay.delay_ms(RESET_MS).await;
        Ok(())
    }

    /// Magnetise the sensing elements in the SET direction.
    pub async fn perform_set<D: DelayNs>(
        &mut self,
        delay: &mut D,
    ) -> Result<(), Error<I2C::Error>> {
        self.write_reg(regs::CONTROL0, bits::SET).await?;
        delay.delay_ms(COIL_PULSE_MS).await;
        Ok(())
    }

    /// Magnetise the sensing elements in the RESET direction, opposite to SET.
    pub async fn perform_reset<D: DelayNs>(
        &mut self,
        delay: &mut D,
    ) -> Result<(), Error<I2C::Error>> {
        self.write_reg(regs::CONTROL0, bits::RESET).await?;
        delay.delay_ms(COIL_PULSE_MS).await;
        Ok(())
    }

    /// Let the chip perform its own SET/RESET around each measurement.
    pub async fn set_auto_set_reset(&mut self, on: bool) -> Result<(), Error<I2C::Error>> {
        self.write_reg(regs::CONTROL0, if on { bits::AUTO_SR_EN } else { 0 })
            .await
    }

    /// Trigger one measurement, wait for it, and return the raw counts.
    pub async fn measure<D: DelayNs>(&mut self, delay: &mut D) -> Result<Raw, Error<I2C::Error>> {
        self.write_reg(regs::CONTROL0, bits::TM_M).await?;
        self.wait_for(bits::MEAS_M_DONE, delay).await?;

        let mut data = [0u8; 7];
        self.i2c
            .write_read(self.addr, &[regs::XOUT0], &mut data)
            .await
            .map_err(Error::I2c)?;

        // Each axis is 18 bits: [17:10] and [9:2] in their own registers, with the
        // bottom two bits packed into 0x06 as Xout[1:0] Yout[1:0] Zout[1:0] 0 0
        let axis =
            |high: u8, mid: u8, low: u8| ((high as u32) << 10) | ((mid as u32) << 2) | low as u32;
        Ok(Raw {
            x: axis(data[0], data[1], (data[6] >> 6) & 0b11),
            y: axis(data[2], data[3], (data[6] >> 4) & 0b11),
            z: axis(data[4], data[5], (data[6] >> 2) & 0b11),
        })
    }

    /// One measurement converted to gauss, with the bridge offset left in.
    pub async fn field<D: DelayNs>(&mut self, delay: &mut D) -> Result<Field, Error<I2C::Error>> {
        Ok(self.measure(delay).await?.to_field())
    }

    /// Field with the bridge offset removed, by measuring after SET and again after
    /// RESET. Costs two measurements and two coil pulses; for a cheaper read, take
    /// the offset once with `offset()` and subtract it from later `field()` calls.
    pub async fn field_offset_cancelled<D: DelayNs>(
        &mut self,
        delay: &mut D,
    ) -> Result<Field, Error<I2C::Error>> {
        self.perform_set(delay).await?;
        let set = self.measure(delay).await?;
        self.perform_reset(delay).await?;
        let reset = self.measure(delay).await?;
        Ok(compensate(set, reset))
    }

    /// Die temperature in degrees Celsius. Not usable while in continuous mode,
    /// and it cannot be measured at the same time as the field.
    pub async fn temperature<D: DelayNs>(
        &mut self,
        delay: &mut D,
    ) -> Result<f32, Error<I2C::Error>> {
        self.write_reg(regs::CONTROL0, bits::TM_T).await?;
        self.wait_for(bits::MEAS_T_DONE, delay).await?;
        let counts = self.read_reg(regs::TOUT).await?;
        Ok(TEMP_AT_ZERO_COUNTS + TEMP_PER_COUNT * counts as f32)
    }

    pub async fn status(&mut self) -> Result<u8, Error<I2C::Error>> {
        self.read_reg(regs::STATUS).await
    }

    async fn wait_for<D: DelayNs>(
        &mut self,
        flag: u8,
        delay: &mut D,
    ) -> Result<(), Error<I2C::Error>> {
        for _ in 0..POLL_LIMIT {
            if self.status().await? & flag != 0 {
                return Ok(());
            }
            delay.delay_ms(POLL_MS).await;
        }
        Err(Error::Timeout)
    }

    async fn read_reg(&mut self, reg: u8) -> Result<u8, Error<I2C::Error>> {
        let mut b = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[reg], &mut b)
            .await
            .map_err(Error::I2c)?;
        Ok(b[0])
    }

    async fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), Error<I2C::Error>> {
        self.i2c
            .write(self.addr, &[reg, val])
            .await
            .map_err(Error::I2c)
    }
}
