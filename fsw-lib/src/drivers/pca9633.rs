use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;

/*
 * PCA9633 Burn Wire Driver
 *
 * Ported from FSW-mainboard flight/hal/drivers/pca9633.py.
 * - Channels 0-2 drive the burn wires, channel 3 enables the burn wire supply
 * - PWM values are inverted on the board ("Values seem negated in testing"), so callers
 *   use 0 for off and 255 for full strength and the driver writes 255 - value
 * - CAUTION: if the strength is too high the board might brown out
 */

mod regs {
    pub const MODE1: u8 = 0x00;
    pub const MODE2: u8 = 0x01;
    pub const PWM0: u8 = 0x02;
    pub const LEDOUT: u8 = 0x08;
}

mod bits {
    pub const MODE1_SLEEP: u8 = 1 << 4;
    pub const MODE2_OUTDRV: u8 = 1 << 2; // Totem-pole (1) or open-drain (0)
    pub const LEDOUT_MASK: u8 = 0b11;
    pub const LEDOUT_FULL_ON: u8 = 0b01;
    pub const LEDOUT_PWM: u8 = 0b10;
}

pub const PWM_MIN: u8 = 0;
pub const PWM_MAX: u8 = 255;
pub const ENABLE_CHANNEL: u8 = 3;

#[derive(Debug, defmt::Format)]
pub enum Error<E> {
    I2c(E),
    InvalidChannel,
}

pub struct PCA9633<I2C: I2c> {
    i2c: I2C,
    addr: u8,
}

impl<I2C: I2c> PCA9633<I2C> {
    pub fn new(i2c: I2C, addr: u8) -> Self {
        Self { i2c, addr }
    }

    /// Wake the chip, select totem-pole outputs and turn every channel off.
    pub async fn init(&mut self) -> Result<(), Error<I2C::Error>> {
        self.update_reg(regs::MODE1, bits::MODE1_SLEEP, 0).await?;
        self.update_reg(regs::MODE2, bits::MODE2_OUTDRV, bits::MODE2_OUTDRV)
            .await?;
        for channel in [ENABLE_CHANNEL, 0, 1, 2] {
            self.turn_off_pwm(channel).await?;
        }
        Ok(())
    }

    /// Put `channel` (0-3) in PWM mode at `value` (0 = off, 255 = full).
    pub async fn set_pwm(&mut self, channel: u8, value: u8) -> Result<(), Error<I2C::Error>> {
        if channel > ENABLE_CHANNEL {
            return Err(Error::InvalidChannel);
        }
        self.set_ledout(channel, bits::LEDOUT_PWM).await?;
        self.write_reg(regs::PWM0 + channel, PWM_MAX - value).await
    }

    pub async fn turn_off_pwm(&mut self, channel: u8) -> Result<(), Error<I2C::Error>> {
        self.set_pwm(channel, PWM_MIN).await?;
        self.set_ledout(channel, bits::LEDOUT_FULL_ON).await
    }

    pub async fn enable_driver(&mut self) -> Result<(), Error<I2C::Error>> {
        self.set_pwm(ENABLE_CHANNEL, PWM_MAX).await?;
        self.update_reg(regs::MODE1, bits::MODE1_SLEEP, 0).await
    }

    /// Turn off the supply and every burn wire, then put the chip to sleep.
    /// Keeps going after a failed write so as much as possible is shut off, and
    /// returns the first error.
    pub async fn disable_driver(&mut self) -> Result<(), Error<I2C::Error>> {
        let mut result = Ok(());
        for channel in [ENABLE_CHANNEL, 0, 1, 2] {
            result = result.and(self.set_pwm(channel, PWM_MIN).await);
        }
        result.and(
            self.update_reg(regs::MODE1, bits::MODE1_SLEEP, bits::MODE1_SLEEP)
                .await,
        )
    }

    /// Burn one wire (channel 0-2) at `strength` for `duration_ms`, then disable the driver.
    /// The driver is disabled even if a step fails. If this future is dropped mid-burn the
    /// wire stays on, so call `disable_driver` in that case.
    pub async fn burn<D: DelayNs>(
        &mut self,
        channel: u8,
        strength: u8,
        duration_ms: u32,
        delay: &mut D,
    ) -> Result<(), Error<I2C::Error>> {
        if channel >= ENABLE_CHANNEL {
            return Err(Error::InvalidChannel);
        }
        let burned = self.start_burn(channel, strength, duration_ms, delay).await;
        let disabled = self.disable_driver().await;
        burned.and(disabled)
    }

    async fn start_burn<D: DelayNs>(
        &mut self,
        channel: u8,
        strength: u8,
        duration_ms: u32,
        delay: &mut D,
    ) -> Result<(), Error<I2C::Error>> {
        self.set_pwm(channel, strength).await?;
        self.enable_driver().await?;
        delay.delay_ms(duration_ms).await;
        Ok(())
    }

    async fn set_ledout(&mut self, channel: u8, mode: u8) -> Result<(), Error<I2C::Error>> {
        let shift = channel * 2;
        self.update_reg(regs::LEDOUT, bits::LEDOUT_MASK << shift, mode << shift)
            .await
    }

    async fn update_reg(&mut self, reg: u8, mask: u8, value: u8) -> Result<(), Error<I2C::Error>> {
        let mut b = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[reg], &mut b)
            .await
            .map_err(Error::I2c)?;
        self.write_reg(reg, (b[0] & !mask) | (value & mask)).await
    }

    async fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), Error<I2C::Error>> {
        self.i2c
            .write(self.addr, &[reg, val])
            .await
            .map_err(Error::I2c)
    }
}
