use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use embedded_hal_async::i2c::I2c;

pub struct DS3231<I2C: I2c> {
    i2c: I2C,
    addr: u8,
}

mod regs {
    pub const DATETIME_START: u8 = 0x00;
    pub const CONTROL: u8 = 0x0E;
    pub const STATUS: u8 = 0x0F;
}

mod masks {
    pub const SECONDS: u8 = 0x7F;
    pub const MINUTES: u8 = 0x7F;
    pub const HOURS_24H: u8 = 0x3F;
    pub const DAY_OF_MONTH: u8 = 0x3F;
    pub const MONTH: u8 = 0x1F;
    pub const EOSC: u8 = 0x80; // Enable Oscillator (active low), CONTROL register
    pub const OSF: u8 = 0x80; // Oscillator Stop Flag, STATUS register
}

const YEAR_OFFSET: u16 = 2000;

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

impl<I2C: I2c> DS3231<I2C> {
    pub fn new(i2c: I2C, addr: u8) -> Self {
        Self { i2c, addr }
    }

    fn bcd2dec(bcd: u8) -> u8 {
        ((bcd & 0xF0) >> 4) * 10 + (bcd & 0x0F)
    }
    fn dec2bcd(dec: u8) -> u8 {
        ((dec / 10) << 4) | (dec % 10)
    }

    fn to_naive(dt: &DateTime) -> Option<NaiveDateTime> {
        NaiveDate::from_ymd_opt(dt.year as i32, dt.month as u32, dt.day as u32)?.and_hms_opt(
            dt.hour as u32,
            dt.minute as u32,
            dt.second as u32,
        )
    }

    /// Current time as seconds since the Unix epoch (UTC).
    pub async fn unix_time(&mut self) -> Result<i64, Error<I2C::Error>> {
        let dt = self.datetime().await?;
        let naive = Self::to_naive(&dt).ok_or(Error::InvalidTime)?;
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

    pub async fn set_datetime(&mut self, dt: &DateTime) -> Result<(), Error<I2C::Error>> {
        // The chip only stores two year digits, so it can only represent 2000-2099
        if !(YEAR_OFFSET..YEAR_OFFSET + 100).contains(&dt.year) || Self::to_naive(dt).is_none() {
            return Err(Error::InvalidTime);
        }

        let buf = [
            regs::DATETIME_START,
            Self::dec2bcd(dt.second) & masks::SECONDS,
            Self::dec2bcd(dt.minute) & masks::MINUTES,
            Self::dec2bcd(dt.hour) & masks::HOURS_24H,
            0x01, // Day of week (1-7), setting to 1 as it is unused
            Self::dec2bcd(dt.day) & masks::DAY_OF_MONTH,
            Self::dec2bcd(dt.month) & masks::MONTH,
            Self::dec2bcd((dt.year % 100) as u8),
        ];
        self.i2c.write(self.addr, &buf).await.map_err(Error::I2c)?;

        // Control: clear only /EOSC (enable oscillator); keep INTCN, alarm and rate-select bits
        self.clear_bits(regs::CONTROL, masks::EOSC).await?;
        // Status: clear only OSF (oscillator stop flag); keep EN32kHz and alarm flags
        self.clear_bits(regs::STATUS, masks::OSF).await?;
        Ok(())
    }

    async fn clear_bits(&mut self, reg: u8, mask: u8) -> Result<(), Error<I2C::Error>> {
        let mut b = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[reg], &mut b)
            .await
            .map_err(Error::I2c)?;
        self.i2c
            .write(self.addr, &[reg, b[0] & !mask])
            .await
            .map_err(Error::I2c)
    }

    pub async fn lost_power(&mut self) -> Result<bool, Error<I2C::Error>> {
        let mut b = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[regs::STATUS], &mut b)
            .await
            .map_err(Error::I2c)?;
        Ok((b[0] & masks::OSF) != 0)
    }

    pub async fn datetime(&mut self) -> Result<DateTime, Error<I2C::Error>> {
        let mut b = [0u8; 7];
        self.i2c
            .write_read(self.addr, &[regs::DATETIME_START], &mut b)
            .await
            .map_err(Error::I2c)?;
        Ok(DateTime {
            second: Self::bcd2dec(b[0] & masks::SECONDS),
            minute: Self::bcd2dec(b[1] & masks::MINUTES),
            hour: Self::bcd2dec(b[2] & masks::HOURS_24H),
            day: Self::bcd2dec(b[4] & masks::DAY_OF_MONTH), // b[3] is Day of Week
            month: Self::bcd2dec(b[5] & masks::MONTH),
            year: YEAR_OFFSET + Self::bcd2dec(b[6]) as u16,
        })
    }
}
