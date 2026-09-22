#![no_std]
#![no_main]

use defmt::info;
use embassy_embedded_hal::shared_bus::asynch::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::i2c::{self, I2c, InterruptHandler};
use embassy_rp::peripherals::{I2C0, USB};
use embassy_rp::{Peri, bind_interrupts};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use panic_probe as _;
use static_cell::StaticCell;

use fsw_lib::drivers::opt4003::OPT4003;

type Bus = Mutex<NoopRawMutex, I2c<'static, I2C0, i2c::Async>>;
static BUS: StaticCell<Bus> = StaticCell::new();

bind_interrupts!(struct Irqs {
    I2C0_IRQ => InterruptHandler<I2C0>;
    USBCTRL_IRQ => embassy_rp::usb::InterruptHandler<USB>;
});

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let _ = spawner.spawn(defmtusb_wrapper(p.USB));
    Timer::after_secs(3).await;

    // PERIPH_PWR_EN: the 3.3V peripheral rail must be on before the I2C addresses are valid
    let mut periph_pwr = Output::new(p.PIN_42, Level::Low);
    periph_pwr.set_high();

    let i2c = I2c::new_async(p.I2C0, p.PIN_25, p.PIN_24, Irqs, i2c::Config::default());
    let i2c_bus = BUS.init(Mutex::new(i2c));

    let _ = spawner.spawn(i2c_task_a(i2c_bus));

    loop {
        info!("looping...");
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

#[embassy_executor::task]
async fn i2c_task_a(i2c_bus: &'static Bus) {
    let i2c_dev = I2cDevice::new(i2c_bus);
    let mut sensor = OPT4003::new(i2c_dev, 0x44);
    let _ = sensor.init().await;
    loop {
        match sensor.lux().await {
            Ok(lux) => {
                info!("lux {}", lux);
            }
            Err(e) => {
                info!("error {:?}", e);
            }
        }
        Timer::after_secs(1).await;
    }
}
