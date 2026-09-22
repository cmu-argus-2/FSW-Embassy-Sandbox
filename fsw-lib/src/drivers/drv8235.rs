use embedded_hal_async::i2c::I2c;

/*
 * DRV8235 Torque Coil (Magnetorquer) Driver
 *
 * Ported from FSW-mainboard flight/hal/drivers/drv8235.py.
 * DC motor driver controlled over I2C: bridge direction and target voltage are set
 * through registers, and motor voltage, current and fault flags are read back.
 */

mod regs {
    pub const FAULT_STATUS: u8 = 0x00;
    pub const REG_STATUS1: u8 = 0x04; // Voltage across motor
    pub const REG_STATUS2: u8 = 0x05; // Current through motor
    pub const CONFIG0: u8 = 0x09;
    pub const CONFIG3: u8 = 0x0C;
    pub const CONFIG4: u8 = 0x0D;
    pub const REG_CTRL0: u8 = 0x0E;
    pub const REG_CTRL1: u8 = 0x0F; // WSET_VSET, target motor voltage
    pub const RC_CTRL2: u8 = 0x13;
    pub const RC_CTRL3: u8 = 0x14; // INV_R
}

mod bits {
    pub const CONFIG0_EN_OUT: u8 = 1 << 7;
    pub const CONFIG0_CLR_FLT: u8 = 1 << 1;
    pub const CONFIG3_INT_VREF: u8 = 1 << 4;
    pub const CONFIG4_PMODE: u8 = 1 << 3; // PWM programming mode
    pub const CONFIG4_I2C_BC: u8 = 1 << 2; // Bridge control over I2C
    pub const CONFIG4_DIR: u8 = 0b11; // IN2 IN1
    pub const REG_CTRL0_REG_CTRL: u8 = 0b11 << 3;
    pub const RC_CTRL2_INV_R_SCALE: u8 = 0b11 << 6;

    pub const FAULT: u8 = 1 << 7;
    pub const STALL: u8 = 1 << 5;
    pub const OCP: u8 = 1 << 4;
    pub const OVP: u8 = 1 << 3;
    pub const TSD: u8 = 1 << 2;
    pub const NPOR: u8 = 1 << 1;
}

const DRV_MAX_VOLT: f32 = 38.0;
const COIL_MAX_VOLT: f32 = 6.0;
const THROTTLE_MAX: f32 = COIL_MAX_VOLT / DRV_MAX_VOLT;

pub const MAX_VOLTS: f32 = 42.7;
const VOLTS_PER_INDEX: f32 = 0.16733; // 42.67 V / 255
const INDEX_PER_VOLT: f32 = 5.9761; // 255 / 42.67 V
const AMPS_PER_INDEX: f32 = 0.01451; // 3.7 A / 255

const REG_CTRL_VOLTAGE: u8 = 0b11;
// TODO (carried over from drv8235.py): check inv_r_scale and inv_r values
const INV_R_SCALE: u8 = 0b11;
const INV_R: u8 = 82;

/// H-bridge state. Bit order: IN2 IN1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum BridgeControl {
    Coast = 0b00, // Standby/coast (Hi-Z)
    Reverse = 0b01,
    Forward = 0b10,
    Brake = 0b11,
}

impl BridgeControl {
    fn from_bits(bits: u8) -> Self {
        match bits & bits::CONFIG4_DIR {
            0b00 => Self::Coast,
            0b01 => Self::Reverse,
            0b10 => Self::Forward,
            _ => Self::Brake,
        }
    }
}

/// Stall, overcurrent and thermal shutdown disable the device until faults are cleared
/// (thermal shutdown also resumes once cool). Undervoltage resumes when voltage returns.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, defmt::Format)]
pub struct Faults {
    pub stall: bool,
    pub overcurrent: bool,
    pub overvoltage: bool,
    pub thermal_shutdown: bool,
    pub undervoltage_lockout: bool,
}

impl Faults {
    pub fn any(&self) -> bool {
        self.stall
            || self.overcurrent
            || self.overvoltage
            || self.thermal_shutdown
            || self.undervoltage_lockout
    }
}

pub struct DRV8235<I2C: I2c> {
    i2c: I2C,
    addr: u8,
}

impl<I2C: I2c> DRV8235<I2C> {
    pub fn new(i2c: I2C, addr: u8) -> Self {
        Self { i2c, addr }
    }

    /// Take bridge control over I2C in voltage regulation mode, coast at 0 V,
    /// enable the output and clear faults.
    pub async fn init(&mut self) -> Result<(), I2C::Error> {
        self.update_reg(regs::CONFIG4, bits::CONFIG4_I2C_BC, bits::CONFIG4_I2C_BC)
            .await?;
        self.update_reg(regs::CONFIG4, bits::CONFIG4_PMODE, bits::CONFIG4_PMODE)
            .await?;
        self.set_bridge(BridgeControl::Coast).await?;
        self.update_reg(
            regs::REG_CTRL0,
            bits::REG_CTRL0_REG_CTRL,
            REG_CTRL_VOLTAGE << 3,
        )
        .await?;
        self.write_reg(regs::REG_CTRL1, 0).await?;
        self.update_reg(
            regs::CONFIG3,
            bits::CONFIG3_INT_VREF,
            bits::CONFIG3_INT_VREF,
        )
        .await?;
        self.update_reg(regs::RC_CTRL2, bits::RC_CTRL2_INV_R_SCALE, INV_R_SCALE << 6)
            .await?;
        self.write_reg(regs::RC_CTRL3, INV_R).await?;
        self.update_reg(regs::CONFIG0, bits::CONFIG0_EN_OUT, bits::CONFIG0_EN_OUT)
            .await?;
        self.clear_faults().await
    }

    /// Throttle from -1.0 (full reverse) to 1.0 (full forward), scaled to the coil's
    /// 6 V limit. `None` coasts, `0.0` brakes.
    pub async fn set_throttle(&mut self, throttle: Option<f32>) -> Result<(), I2C::Error> {
        let Some(throttle) = throttle else {
            return self.coast().await;
        };
        let fraction = (throttle * THROTTLE_MAX).clamp(-THROTTLE_MAX, THROTTLE_MAX);
        self.drive(throttle, (fraction.abs() * 255.0) as u8).await
    }

    /// Throttle in volts from -42.7 to 42.7. `None` coasts, `0.0` brakes.
    pub async fn set_throttle_volts(&mut self, volts: Option<f32>) -> Result<(), I2C::Error> {
        let Some(volts) = volts else {
            return self.coast().await;
        };
        let volts = volts.clamp(-MAX_VOLTS, MAX_VOLTS);
        self.drive(volts, round_to_u8(volts.abs() * INDEX_PER_VOLT))
            .await
    }

    /// Raw WSET_VSET value from -255 to 255. `None` coasts, `0` brakes.
    pub async fn set_throttle_raw(&mut self, raw: Option<i16>) -> Result<(), I2C::Error> {
        let Some(raw) = raw else {
            return self.coast().await;
        };
        let raw = raw.clamp(-255, 255);
        self.drive(raw as f32, raw.unsigned_abs() as u8).await
    }

    /// Current throttle from -1.0 to 1.0, or `None` when coasting.
    pub async fn throttle(&mut self) -> Result<Option<f32>, I2C::Error> {
        let index = self.read_reg(regs::REG_CTRL1).await?;
        let magnitude = round_3dp(index as f32 / 255.0) / THROTTLE_MAX;
        self.signed_by_bridge(magnitude).await
    }

    /// Current target voltage from -42.7 to 42.7, or `None` when coasting.
    pub async fn throttle_volts(&mut self) -> Result<Option<f32>, I2C::Error> {
        let index = self.read_reg(regs::REG_CTRL1).await?;
        self.signed_by_bridge(index as f32 * VOLTS_PER_INDEX).await
    }

    /// Current raw WSET_VSET value from -255 to 255, or `None` when coasting.
    pub async fn throttle_raw(&mut self) -> Result<Option<i16>, I2C::Error> {
        let index = self.read_reg(regs::REG_CTRL1).await?;
        Ok(self.signed_by_bridge(index as f32).await?.map(|v| v as i16))
    }

    pub async fn bridge_control(&mut self) -> Result<BridgeControl, I2C::Error> {
        Ok(BridgeControl::from_bits(
            self.read_reg(regs::CONFIG4).await?,
        ))
    }

    /// Measured (volts, amps) across the coil.
    pub async fn read_voltage_current(&mut self) -> Result<(f32, f32), I2C::Error> {
        let voltage = self.read_reg(regs::REG_STATUS1).await? as f32 * VOLTS_PER_INDEX;
        let current = self.read_reg(regs::REG_STATUS2).await? as f32 * AMPS_PER_INDEX;
        Ok((voltage, current))
    }

    /// Read the fault flags. If any fault is reported, the flags are cleared afterwards.
    pub async fn faults(&mut self) -> Result<Faults, I2C::Error> {
        let status = self.read_reg(regs::FAULT_STATUS).await?;
        if status & bits::FAULT == 0 {
            return Ok(Faults::default());
        }
        let faults = Faults {
            stall: status & bits::STALL != 0,
            overcurrent: status & bits::OCP != 0,
            overvoltage: status & bits::OVP != 0,
            thermal_shutdown: status & bits::TSD != 0,
            undervoltage_lockout: status & bits::NPOR != 0,
        };
        self.clear_faults().await?;
        Ok(faults)
    }

    pub async fn clear_faults(&mut self) -> Result<(), I2C::Error> {
        self.update_reg(regs::CONFIG0, bits::CONFIG0_CLR_FLT, bits::CONFIG0_CLR_FLT)
            .await
    }

    async fn coast(&mut self) -> Result<(), I2C::Error> {
        self.write_reg(regs::REG_CTRL1, 0).await?;
        self.set_bridge(BridgeControl::Coast).await
    }

    /// Reverse for negative `sign`, forward for positive, brake for zero.
    async fn drive(&mut self, sign: f32, index: u8) -> Result<(), I2C::Error> {
        let (index, bridge) = if sign < 0.0 {
            (index, BridgeControl::Reverse)
        } else if sign > 0.0 {
            (index, BridgeControl::Forward)
        } else {
            (0, BridgeControl::Brake)
        };
        self.write_reg(regs::REG_CTRL1, index).await?;
        self.set_bridge(bridge).await
    }

    async fn signed_by_bridge(&mut self, magnitude: f32) -> Result<Option<f32>, I2C::Error> {
        Ok(match self.bridge_control().await? {
            BridgeControl::Coast => None,
            BridgeControl::Brake => Some(0.0),
            BridgeControl::Reverse => Some(-magnitude),
            BridgeControl::Forward => Some(magnitude),
        })
    }

    async fn set_bridge(&mut self, bridge: BridgeControl) -> Result<(), I2C::Error> {
        self.update_reg(regs::CONFIG4, bits::CONFIG4_DIR, bridge as u8)
            .await
    }

    async fn update_reg(&mut self, reg: u8, mask: u8, value: u8) -> Result<(), I2C::Error> {
        let old = self.read_reg(reg).await?;
        self.write_reg(reg, (old & !mask) | (value & mask)).await
    }

    async fn read_reg(&mut self, reg: u8) -> Result<u8, I2C::Error> {
        let mut b = [0u8; 1];
        self.i2c.write_read(self.addr, &[reg], &mut b).await?;
        Ok(b[0])
    }

    async fn write_reg(&mut self, reg: u8, val: u8) -> Result<(), I2C::Error> {
        self.i2c.write(self.addr, &[reg, val]).await
    }
}

/// Round a non-negative value to the nearest integer, saturating at 255.
fn round_to_u8(x: f32) -> u8 {
    (x + 0.5) as u8
}

/// Round a non-negative value to 3 decimal places.
fn round_3dp(x: f32) -> f32 {
    ((x * 1000.0 + 0.5) as u32) as f32 / 1000.0
}
