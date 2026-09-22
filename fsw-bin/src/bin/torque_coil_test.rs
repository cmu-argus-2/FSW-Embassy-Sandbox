#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c, InterruptHandler};
use embassy_rp::peripherals::{I2C1, USB};
use embassy_rp::{Peri, bind_interrupts};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;

use fsw_lib::drivers::drv8235::DRV8235;

/*
 * Torque Coil Test (Mainboard v4)
 *
 * DRV8235 drivers on I2C1 (SDA1 = GPIO46, SCL1 = GPIO47): XP 0x30, YP 0x31, ZP 0x12.
 * The -X/-Y/-Z coils sit on I2C0 (SDA0 = GPIO24, SCL0 = GPIO25) at 0x30/0x31/0x32;
 * to test those, switch the bus and pins below and use COILS_I2C0.
 *
 * COIL_EN (GPIO1) powers the coils and PERIPH_PWR_EN (GPIO42) powers the 3.3V
 * peripherals; both must be high before the drivers answer.
 *
 * Each coil is driven forward then reverse at a low throttle, reading back the measured
 * voltage and current and any faults. Coils are left coasting between steps.
 */

const COILS_I2C1: [(&str, u8); 3] = [("XP", 0x30), ("YP", 0x31), ("ZP", 0x12)];
#[allow(dead_code)]
const COILS_I2C0: [(&str, u8); 3] = [("XM", 0x30), ("YM", 0x31), ("ZM", 0x32)];

const THROTTLE: f32 = 0.25;
const DWELL_MS: u64 = 2000;

type Bus = Mutex<NoopRawMutex, I2c<'static, I2C1, i2c::Async>>;
static BUS: StaticCell<Bus> = StaticCell::new();

bind_interrupts!(struct Irqs {
    I2C1_IRQ => InterruptHandler<I2C1>;
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let _ = spawner.spawn(defmtusb_wrapper(p.USB));
    Timer::after_secs(3).await;

    info!("--- DRV8235 Torque Coil Test (v4, I2C1) ---");

    let mut periph_pwr = Output::new(p.PIN_42, Level::High);
    periph_pwr.set_high();
    let mut coil_pwr = Output::new(p.PIN_1, Level::High);
    coil_pwr.set_high();
    Timer::after_millis(500).await;

    let i2c = I2c::new_async(p.I2C1, p.PIN_47, p.PIN_46, Irqs, i2c::Config::default());
    let bus = BUS.init(Mutex::new(i2c));

    loop {
        for (name, addr) in COILS_I2C1 {
            let mut coil = DRV8235::new(I2cDevice::new(bus), addr);

            if let Err(e) = coil.init().await {
                error!("{}: init failed at 0x{:02X}: {:?}", name, addr, e);
                continue;
            }
            info!("{}: ready at 0x{:02X}", name, addr);

            for throttle in [THROTTLE, -THROTTLE] {
                if let Err(e) = coil.set_throttle(Some(throttle)).await {
                    error!("{}: set_throttle({}) failed: {:?}", name, throttle, e);
                    break;
                }
                Timer::after_millis(DWELL_MS).await;

                match coil.read_voltage_current().await {
                    Ok((volts, amps)) => {
                        info!("{}: throttle {} -> {} V, {} A", name, throttle, volts, amps)
                    }
                    Err(e) => error!("{}: read failed: {:?}", name, e),
                }
                match coil.faults().await {
                    Ok(f) if f.any() => error!("{}: faults {:?}", name, f),
                    Ok(_) => info!("{}: no faults", name),
                    Err(e) => error!("{}: fault read failed: {:?}", name, e),
                }
            }

            // Coast before moving to the next coil
            if let Err(e) = coil.set_throttle(None).await {
                error!("{}: coast failed: {:?}", name, e);
            }
            Timer::after_millis(500).await;
        }
        info!("Sweep complete, repeating in 5s");
        Timer::after_secs(5).await;
    }
}

#[embassy_executor::task]
async fn defmtusb_wrapper(usb: Peri<'static, USB>) {
    let driver = embassy_rp::usb::Driver::new(usb, Irqs);
    let config = {
        let mut c = embassy_usb::Config::new(0x1234, 0x5678);
        c.serial_number = Some("defmt");
        c.max_packet_size_0 = 64;
        c.composite_with_iads = true;
        c.device_class = 0xEF;
        c.device_sub_class = 0x02;
        c.device_protocol = 0x01;
        c
    };
    defmt_embassy_usbserial::run(driver, config).await;
}
