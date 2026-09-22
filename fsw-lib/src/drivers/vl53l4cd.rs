use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;
pub use vl53l4cd_ulp::Error;
use vl53l4cd_ulp::VL53L4cd;

pub struct VL53L4CD<I2C, D> {
    sensor: VL53L4cd<I2C, D>,
    addr: u8,
}

impl<I2C: I2c, D: DelayNs> VL53L4CD<I2C, D> {
    /// Constructs the driver without accessing hardware. Call `init` before reading.
    /// The sensor must initially use its default address (0x29).
    pub fn new(i2c: I2C, addr: u8, delay: D) -> Self {
        Self {
            sensor: VL53L4cd::new(i2c, delay),
            addr,
        }
    }

    /// Sets the I2C address, initializes the sensor, and starts ranging.
    /// The caller must enable sensor power and allow it to settle first.
    pub async fn init(&mut self) -> Result<(), Error<I2C::Error>> {
        self.sensor.set_i2c_address(self.addr).await?;
        if let Err(e) = self.sensor.sensor_init().await {
            defmt::error!("VL53L4CD sensor_init failed");
            return Err(e);
        }
        if let Err(e) = self.sensor.start_ranging().await {
            defmt::error!("VL53L4CD start_ranging failed");
            return Err(e);
        }
        Ok(())
    }

    /// Returns an estimated distance in millimeters, or `None` if not ready.
    /// Acknowledges each successfully read measurement before returning it.
    pub async fn read_distance(&mut self) -> Result<Option<u16>, Error<I2C::Error>> {
        if !self.sensor.check_for_data_ready().await? {
            return Ok(None);
        }
        let measurement = self.sensor.get_estimated_measurement().await?;
        self.sensor.clear_interrupt().await?;
        Ok(Some(measurement.estimated_distance_mm))
    }
}
