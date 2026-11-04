use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use embedded_hal_async::i2c::I2c;

/*
 * RV-3032-C7 Real-Time Clock Driver
 *
 * Temperature compensated RTC on I2C at 0x51, replacing the DS3231. Time is held in
 * binary-coded decimal like the DS3231, so the same epoch conversion applies, but the
 * flags differ: this part reports a power-on reset (PORF) and a low voltage event
 * (VLF) instead of an oscillator stop flag, and it has no oscillator-enable bit to
 * clear, so setting the time is just the clock registers plus clearing those flags.
 *
 * Configuration lives in EEPROM mirrored into RAM at 0xC0-0xCA. This driver only
 * touches the RAM clock registers, so it never writes EEPROM and never needs the
 * refresh sequence.
 */

pub const DEFAULT_ADDRESS: u8 = 0x51;

mod regs {
    pub const SECONDS: u8 = 0x01; // through YEAR at 0x07, auto-incrementing
    pub const STATUS: u8 = 0x0D;
    pub const TEMP_LSB: u8 = 0x0E; // TEMP[3:0] in bits 7:4, flags in bits 3:0
}

mod masks {
    pub const SECONDS: u8 = 0x7F;
    pub const MINUTES: u8 = 0x7F;
    pub const HOURS_24H: u8 = 0x3F;
    pub const DATE: u8 = 0x3F;
    pub const MONTH: u8 = 0x1F;
    pub const PORF: u8 = 1 << 1; // power-on reset: contents are not valid
    pub const VLF: u8 = 1 << 0; // voltage dropped below ~1.2V: contents may not be valid
}

const YEAR_OFFSET: u16 = 2000;
/// Temperature is 12-bit two's complement, one count is 1/16 of a degree
const TEMP_PER_COUNT: f32 = 0.0625;

#[derive(Debug, defmt::Format)]
pub enum Error<E> {
    I2c(E),
    InvalidTime,
}

#[derive(Debug, Clone, Copy, defmt::Format)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

pub struct RV3032<I2C: I2c> {
    i2c: I2C,
    addr: u8,
}

impl<I2C: I2c> RV3032<I2C> {
    pub fn new(i2c: I2C, addr: u8) -> Self {
        Self { i2c, addr }
    }

    /// The chip stores each field as binary-coded decimal: one decimal digit per
    /// nibble, so the value 25 is stored as 0x25 rather than 0x19.
    fn bcd2dec(bcd: u8) -> u8 {
        ((bcd & 0xF0) >> 4) * 10 + (bcd & 0x0F)
    }

    fn dec2bcd(dec: u8) -> u8 {
        ((dec / 10) << 4) | (dec % 10)
    }

    /// Converts our `DateTime` into chrono's `NaiveDateTime` ("naive" meaning it carries
    /// no time zone; the chip keeps UTC by convention). Returns `None` if the fields are
    /// not a real date, which is how a blank or corrupted chip is detected.
    fn to_chrono_datetime(dt: &DateTime) -> Option<NaiveDateTime> {
        NaiveDate::from_ymd_opt(dt.year as i32, dt.month as u32, dt.day as u32)?.and_hms_opt(
            dt.hour as u32,
            dt.minute as u32,
            dt.second as u32,
        )
    }

    /// Current time as seconds since the Unix epoch (UTC).
    pub async fn unix_time(&mut self) -> Result<i64, Error<I2C::Error>> {
        let dt = self.datetime().await?;
        let naive = Self::to_chrono_datetime(&dt).ok_or(Error::InvalidTime)?;
        Ok(naive.and_utc().timestamp())
    }

    pub async fn set_unix_time(&mut self, ts: i64) -> Result<(), Error<I2C::Error>> {
        let dt = chrono::DateTime::from_timestamp(ts, 0).ok_or(Error::InvalidTime)?;
        self.set_datetime(&DateTime {
            year: dt.year() as u16,
            month: dt.month() as u8,
            day: dt.day() as u8,
            hour: dt.hour() as u8,
            minute: dt.minute() as u8,
            second: dt.second() as u8,
        })
        .await
    }

    pub async fn datetime(&mut self) -> Result<DateTime, Error<I2C::Error>> {
        let mut b = [0u8; 7];
        self.i2c
            .write_read(self.addr, &[regs::SECONDS], &mut b)
            .await
            .map_err(Error::I2c)?;
        Ok(DateTime {
            second: Self::bcd2dec(b[0] & masks::SECONDS),
            minute: Self::bcd2dec(b[1] & masks::MINUTES),
            hour: Self::bcd2dec(b[2] & masks::HOURS_24H),
            // b[3] is the weekday, which we do not track
            day: Self::bcd2dec(b[4] & masks::DATE),
            month: Self::bcd2dec(b[5] & masks::MONTH),
            year: YEAR_OFFSET + Self::bcd2dec(b[6]) as u16,
        })
    }

    /// Writing the seconds register also resets the hundredths counter, so the new time
    /// starts exactly on the second.
    pub async fn set_datetime(&mut self, dt: &DateTime) -> Result<(), Error<I2C::Error>> {
        // The chip only stores two year digits, so it can only represent 2000-2099
        if !(YEAR_OFFSET..YEAR_OFFSET + 100).contains(&dt.year)
            || Self::to_chrono_datetime(dt).is_none()
        {
            return Err(Error::InvalidTime);
        }

        let buf = [
            regs::SECONDS,
            Self::dec2bcd(dt.second) & masks::SECONDS,
            Self::dec2bcd(dt.minute) & masks::MINUTES,
            Self::dec2bcd(dt.hour) & masks::HOURS_24H,
            0x01, // weekday (1-7), unused, set to 1
            Self::dec2bcd(dt.day) & masks::DATE,
            Self::dec2bcd(dt.month) & masks::MONTH,
            Self::dec2bcd((dt.year % 100) as u8),
        ];
        self.i2c.write(self.addr, &buf).await.map_err(Error::I2c)?;

        self.clear_power_flags().await
    }

    /// True if the time cannot be trusted: either the supply restarted from below the
    /// power-on threshold (PORF) or it dipped below about 1.2V (VLF).
    pub async fn lost_power(&mut self) -> Result<bool, Error<I2C::Error>> {
        Ok(self.status().await? & (masks::PORF | masks::VLF) != 0)
    }

    /// Clear PORF and VLF, leaving the other flags in the status register alone.
    ///
    /// Note the chip also resets its temperature-threshold flags (THF, TLF) on any write
    /// to the status register, which cannot be avoided.
    pub async fn clear_power_flags(&mut self) -> Result<(), Error<I2C::Error>> {
        let status = self.status().await?;
        self.i2c
            .write(
                self.addr,
                &[regs::STATUS, status & !(masks::PORF | masks::VLF)],
            )
            .await
            .map_err(Error::I2c)
    }

    pub async fn status(&mut self) -> Result<u8, Error<I2C::Error>> {
        let mut b = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[regs::STATUS], &mut b)
            .await
            .map_err(Error::I2c)?;
        Ok(b[0])
    }

    /// Die temperature in degrees Celsius, which the chip measures every second and uses
    /// for its own frequency compensation. The first value appears a second after power-up.
    pub async fn temperature(&mut self) -> Result<f32, Error<I2C::Error>> {
        let mut b = [0u8; 2];
        self.i2c
            .write_read(self.addr, &[regs::TEMP_LSB], &mut b)
            .await
            .map_err(Error::I2c)?;

        // TEMP[3:0] sits in the top nibble of 0x0E, TEMP[11:4] in 0x0F
        let raw = ((b[1] as u16) << 4) | (b[0] >> 4) as u16;
        let signed = if raw & 0x800 != 0 {
            raw as i16 - 0x1000
        } else {
            raw as i16
        };
        Ok(signed as f32 * TEMP_PER_COUNT)
    }
}
