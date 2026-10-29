use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;

/*
 * BMX160 IMU Driver (gyroscope + BMM150 magnetometer)
 *
 * Supports soft reset, chip ID check, the init sequence (magnetometer setup, including reading
 * the BMM150 trim, then gyroscope setup), and gyro and magnetometer readings. The magnetometer
 * reading is compensated with the BMM150 factory trim. Accelerometer readings are not implemented
 * yet; the accelerometer is left in its post-reset suspend mode.
 */

mod regs {
    pub const CHIP_ID: u8 = 0x00;
    pub const MAG_DATA: u8 = 0x04; // mag x/y/z + rhall, 8 bytes
    pub const GYRO_DATA: u8 = 0x0C;
    pub const STATUS: u8 = 0x1B;
    pub const GYRO_CONFIG: u8 = 0x42;
    pub const GYRO_RANGE: u8 = 0x43;
    pub const MAG_ODR: u8 = 0x44;
    pub const MAG_IF_0: u8 = 0x4C;
    pub const MAG_IF_1: u8 = 0x4D;
    pub const MAG_IF_2: u8 = 0x4E;
    pub const MAG_IF_3: u8 = 0x4F;
    pub const COMMAND: u8 = 0x7E;
}

mod cmd {
    pub const SOFT_RESET: u8 = 0xB6;
    pub const GYRO_NORMAL_MODE: u8 = 0x15;
    pub const MAG_NORMAL_MODE: u8 = 0x19;
    pub const MAG_LOWPOWER_MODE: u8 = 0x1A;
}

const CHIP_ID: u8 = 0xD8;
const STATUS_MAG_MAN_OP: u8 = 1 << 2;

const MAG_ODR_12_5HZ: u8 = 0x05;
/// Gyro output data rate 50 Hz, OSR4 filter.
const GYRO_ODR_50HZ: u8 = 0x07;

/// Polls of `mag_man_op` before a BMM150 indirect access is declared stuck (about 1 ms).
const MAG_IF_POLLS: u32 = 10;
const MAG_IF_POLL_US: u32 = 100;

const DEG_TO_RAD: f32 = core::f32::consts::PI / 180.0;

/// BMM150 raw values that mean the axis overflowed.
const MAG_OVERFLOW_XY: i16 = -4096;
const MAG_OVERFLOW_Z: i16 = -16384;
/// µT per LSB, used only if the trim could not be read.
const MAG_UNTRIMMED_SCALE: f32 = 1.0 / 16.0;

/// Gyroscope full-scale range. Discriminant is the GYRO_RANGE register value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum GyroRange {
    Dps2000 = 0,
    Dps1000 = 1,
    Dps500 = 2,
    Dps250 = 3,
    Dps125 = 4,
}

impl GyroRange {
    fn dps(self) -> f32 {
        match self {
            Self::Dps2000 => 2000.0,
            Self::Dps1000 => 1000.0,
            Self::Dps500 => 500.0,
            Self::Dps250 => 250.0,
            Self::Dps125 => 125.0,
        }
    }

    /// rad/s per LSB.
    fn scale(self) -> f32 {
        self.dps() / 32768.0 * DEG_TO_RAD
    }
}

#[derive(Debug, defmt::Format)]
pub enum Error<E> {
    I2c(E),
    BadId(u8),
    /// A BMM150 indirect access did not finish.
    MagInterfaceTimeout,
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Self::I2c(e)
    }
}

/// BMM150 factory trim coefficients (datasheet Appendix 1).
#[derive(Debug, Clone, Copy, Default, PartialEq, defmt::Format)]
pub struct MagTrim {
    pub x1: i8,
    pub y1: i8,
    pub x2: i8,
    pub y2: i8,
    pub z1: u16,
    pub z2: i16,
    pub z3: i16,
    pub z4: i16,
    pub xy1: u8,
    pub xy2: i8,
    pub xyz1: u16,
}

pub struct BMX160<I2C: I2c> {
    i2c: I2C,
    addr: u8,
    gyro_scale: f32,
    trim: MagTrim,
}

impl<I2C: I2c> BMX160<I2C> {
    pub fn new(i2c: I2C, addr: u8) -> Self {
        Self {
            i2c,
            addr,
            gyro_scale: GyroRange::Dps500.scale(),
            trim: MagTrim::default(),
        }
    }

    /// Soft reset, check the chip ID, then set up the magnetometer and the gyroscope (500 dps,
    /// 50 Hz).
    pub async fn init<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), Error<I2C::Error>> {
        self.write_reg(regs::COMMAND, cmd::SOFT_RESET).await?;
        // Datasheet says ~1 ms; 3 ms is needed in practice.
        delay.delay_ms(3).await;

        let id = self.read_reg(regs::CHIP_ID).await?;
        if id != CHIP_ID {
            return Err(Error::BadId(id));
        }

        self.init_mag(delay).await?;
        self.init_gyro(delay).await
    }

    async fn init_mag<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), Error<I2C::Error>> {
        self.write_reg(regs::COMMAND, cmd::MAG_NORMAL_MODE).await?;
        delay.delay_us(650).await;
        self.write_reg(regs::MAG_IF_0, 0x80).await?;
        self.bmm150_write(0x4B, 0x01, delay).await?; // sleep mode
        self.trim = self.read_trim(delay).await?;
        self.write_reg(regs::MAG_IF_0, 0x80).await?; // manual mode, burst 1
        self.bmm150_write(0x51, 0x17, delay).await?; // rep_xy = 23 (high accuracy)
        self.bmm150_write(0x52, 0x52, delay).await?; // rep_z = 82 (high accuracy)
        self.bmm150_write(0x4C, 0x02, delay).await?; // forced mode
        self.write_reg(regs::MAG_IF_1, 0x42).await?;
        self.write_reg(regs::MAG_ODR, MAG_ODR_12_5HZ).await?;
        // Data mode with burst 8 so RHALL is included in every auto-poll.
        self.write_reg(regs::MAG_IF_0, 0x03).await?;
        self.write_reg(regs::COMMAND, cmd::MAG_LOWPOWER_MODE)
            .await?;
        delay.delay_ms(100).await; // warm-up, found empirically
        Ok(())
    }

    async fn init_gyro<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), Error<I2C::Error>> {
        self.set_gyro_range(GyroRange::Dps500).await?;
        self.write_reg(regs::GYRO_CONFIG, GYRO_ODR_50HZ).await?;
        self.write_reg(regs::COMMAND, cmd::GYRO_NORMAL_MODE).await?;
        delay.delay_us(8100).await;
        Ok(())
    }

    /// Set the gyroscope range. The scale used by [`Self::gyro`] follows the value read back.
    pub async fn set_gyro_range(&mut self, range: GyroRange) -> Result<(), Error<I2C::Error>> {
        self.write_reg(regs::GYRO_RANGE, range as u8).await?;
        let readback = self.read_reg(regs::GYRO_RANGE).await?;
        let applied = match readback {
            0 => GyroRange::Dps2000,
            1 => GyroRange::Dps1000,
            2 => GyroRange::Dps500,
            3 => GyroRange::Dps250,
            4 => GyroRange::Dps125,
            _ => return Ok(()), // unknown value: keep the current scale
        };
        self.gyro_scale = applied.scale();
        Ok(())
    }

    /// Angular rate in rad/s, sensor axes.
    pub async fn gyro(&mut self) -> Result<[f32; 3], Error<I2C::Error>> {
        let mut buf = [0u8; 6];
        self.read_regs(regs::GYRO_DATA, &mut buf).await?;
        let raw = le_i16x3(&buf);
        Ok(raw.map(|v| v as f32 * self.gyro_scale))
    }

    /// Magnetic field in µT, sensor axes, compensated with the BMM150 factory trim. An axis that
    /// overflowed reads 0.0.
    pub async fn mag(&mut self) -> Result<[f32; 3], Error<I2C::Error>> {
        let mut buf = [0u8; 8];
        self.read_regs(regs::MAG_DATA, &mut buf).await?;
        let raw = le_i16x3(&buf[..6]);
        let rhall = u16::from_le_bytes([buf[6], buf[7]]);
        Ok(compensate_mag(raw, rhall, &self.trim))
    }

    pub fn trim(&self) -> &MagTrim {
        &self.trim
    }

    async fn read_trim<D: DelayNs>(&mut self, delay: &mut D) -> Result<MagTrim, Error<I2C::Error>> {
        let mut t = MagTrim::default();
        let mut d = [0u8; 8];

        self.bmm150_read(0x5D, &mut d[..2], delay).await?;
        t.x1 = d[0] as i8;
        t.y1 = d[1] as i8;

        self.bmm150_read(0x62, &mut d[..2], delay).await?;
        t.z4 = i16::from_le_bytes([d[0], d[1]]);

        self.bmm150_read(0x64, &mut d[..2], delay).await?;
        t.x2 = d[0] as i8;
        t.y2 = d[1] as i8;

        self.bmm150_read(0x68, &mut d, delay).await?;
        t.z2 = i16::from_le_bytes([d[0], d[1]]);
        t.z1 = u16::from_le_bytes([d[2], d[3]]);
        t.xyz1 = u16::from_le_bytes([d[4], d[5]]) & 0x7FFF;
        t.z3 = i16::from_le_bytes([d[6], d[7]]);

        self.bmm150_read(0x70, &mut d[..2], delay).await?;
        t.xy2 = d[0] as i8;
        t.xy1 = d[1];

        Ok(t)
    }

    /// Indirect write to the BMM150 through the BMX160 secondary interface.
    async fn bmm150_write<D: DelayNs>(
        &mut self,
        reg: u8,
        val: u8,
        delay: &mut D,
    ) -> Result<(), Error<I2C::Error>> {
        self.write_reg(regs::MAG_IF_3, val).await?;
        self.write_reg(regs::MAG_IF_2, reg).await?;
        self.wait_mag_if(delay).await
    }

    /// Indirect read from the BMM150. `buf.len()` must be 1, 2, 6 or 8 (burst sizes).
    async fn bmm150_read<D: DelayNs>(
        &mut self,
        reg: u8,
        buf: &mut [u8],
        delay: &mut D,
    ) -> Result<(), Error<I2C::Error>> {
        let burst = match buf.len() {
            1 => 0x00,
            2 => 0x01,
            6 => 0x02,
            _ => 0x03,
        };
        self.write_reg(regs::MAG_IF_0, 0x80 | burst).await?;
        self.write_reg(regs::MAG_IF_1, reg).await?;
        self.wait_mag_if(delay).await?;
        self.read_regs(regs::MAG_DATA, buf).await?;
        Ok(())
    }

    async fn wait_mag_if<D: DelayNs>(&mut self, delay: &mut D) -> Result<(), Error<I2C::Error>> {
        for _ in 0..MAG_IF_POLLS {
            if self.read_reg(regs::STATUS).await? & STATUS_MAG_MAN_OP == 0 {
                return Ok(());
            }
            delay.delay_us(MAG_IF_POLL_US).await;
        }
        Err(Error::MagInterfaceTimeout)
    }

    async fn read_reg(&mut self, reg: u8) -> Result<u8, I2C::Error> {
        let mut buf = [0u8];
        self.i2c.write_read(self.addr, &[reg], &mut buf).await?;
        Ok(buf[0])
    }

    async fn read_regs(&mut self, reg: u8, buf: &mut [u8]) -> Result<(), I2C::Error> {
        self.i2c.write_read(self.addr, &[reg], buf).await
    }

    async fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), I2C::Error> {
        self.i2c.write(self.addr, &[reg, val]).await
    }
}

fn le_i16x3(b: &[u8]) -> [i16; 3] {
    [
        i16::from_le_bytes([b[0], b[1]]),
        i16::from_le_bytes([b[2], b[3]]),
        i16::from_le_bytes([b[4], b[5]]),
    ]
}

/// Turn the raw BMM150 registers into µT with the factory trim (BMM150 datasheet, floating-point
/// compensation). `raw` is the x/y/z data registers as read, `rhall` the hall resistance register.
///
/// If the trim was never read (`xyz1 == 0`), falls back to a flat 1/16 µT per LSB.
pub fn compensate_mag(raw: [i16; 3], rhall: u16, trim: &MagTrim) -> [f32; 3] {
    // The data registers are left-aligned: x/y are 13-bit, z 15-bit, rhall 14-bit.
    let x = raw[0] >> 3;
    let y = raw[1] >> 3;
    let z = raw[2] >> 1;
    let rhall = rhall >> 2;

    if trim.xyz1 == 0 {
        return [x, y, z].map(|v| v as f32 * MAG_UNTRIMMED_SCALE);
    }
    [
        compensate_xy(x, rhall, trim.x1, trim.x2, trim),
        compensate_xy(y, rhall, trim.y1, trim.y2, trim),
        compensate_z(z, rhall, trim),
    ]
}

/// X or Y axis: `d1`/`d2` are that axis's `x1`/`x2` or `y1`/`y2` trim.
fn compensate_xy(adc: i16, rhall: u16, d1: i8, d2: i8, t: &MagTrim) -> f32 {
    if adc == MAG_OVERFLOW_XY || rhall == 0 {
        return 0.0;
    }
    let p0 = t.xyz1 as f32 * 16384.0 / rhall as f32 - 16384.0;
    let p1 = t.xy2 as f32 * (p0 * p0 / 268_435_456.0);
    let p2 = p1 + p0 * t.xy1 as f32 / 16384.0;
    let p4 = adc as f32 * ((p2 + 256.0) * (d2 as f32 + 160.0));
    (p4 / 8192.0 + d1 as f32 * 8.0) / 16.0
}

fn compensate_z(adc: i16, rhall: u16, t: &MagTrim) -> f32 {
    if adc == MAG_OVERFLOW_Z || t.z1 == 0 || t.z2 == 0 || rhall == 0 {
        return 0.0;
    }
    let p0 = adc as f32 - t.z4 as f32;
    let p1 = rhall as f32 - t.xyz1 as f32;
    let p2 = t.z3 as f32 * p1;
    let p3 = t.z2 as f32 + t.z1 as f32 * rhall as f32 / 32768.0;
    if p3 == 0.0 {
        return 0.0;
    }
    (p0 * 131_072.0 - p2) / (p3 * 64.0)
}
