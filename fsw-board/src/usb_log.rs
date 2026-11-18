//! defmt logging over USB serial, for bench and bring-up binaries (`usb-log` feature).
//!
//! Spawn [`usb_logger`] early and give the host a few seconds to open the port before logging
//! anything important.

use embassy_time::Timer;

use crate::{Irqs, UsbPins};

#[embassy_executor::task]
pub async fn usb_logger(r: UsbPins) {
    let driver = embassy_rp::usb::Driver::new(r.usb, Irqs);
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
    // This backend otherwise waits for a full buffer before transmitting.
    // Its USB writer polls every 100 ms; flush less often to let it drain.
    let flush_logs = async {
        loop {
            Timer::after_millis(250).await;
            defmt::flush();
        }
    };
    embassy_futures::join::join(defmt_embassy_usbserial::run(driver, config), flush_logs).await;
}
