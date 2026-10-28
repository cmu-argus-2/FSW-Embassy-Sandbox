// Shared fakes for driver tests.
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::{Error, ErrorKind, ErrorType, I2c, Operation};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    Write(u8, u8),
    DelayMs(u32),
}

pub type Log = Rc<RefCell<Vec<Event>>>;

/// Return and clear everything recorded in `log`.
pub fn take_events(log: &Log) -> Vec<Event> {
    log.borrow_mut().drain(..).collect()
}

pub fn writes(pairs: &[(u8, u8)]) -> Vec<Event> {
    pairs
        .iter()
        .map(|&(reg, val)| Event::Write(reg, val))
        .collect()
}

#[derive(Debug)]
pub struct FakeError;

impl Error for FakeError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

/// Simulated I2C device: a 256-byte register map where a write sets the register
/// pointer, and reads and writes auto-increment it. Every register write is logged.
pub struct FakeRegs {
    pub regs: [u8; 256],
    pub log: Log,
    /// Writes to this register fail.
    pub fail_writes_to: Option<u8>,
}

impl FakeRegs {
    pub fn new(initial: &[(u8, u8)]) -> Self {
        let mut regs = [0u8; 256];
        for &(reg, val) in initial {
            regs[reg as usize] = val;
        }
        Self {
            regs,
            log: Log::default(),
            fail_writes_to: None,
        }
    }
}

impl ErrorType for FakeRegs {
    type Error = FakeError;
}

impl I2c for FakeRegs {
    async fn transaction(
        &mut self,
        _address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), FakeError> {
        let mut ptr = 0u8;
        for op in operations {
            match op {
                Operation::Write(bytes) => {
                    let Some((&reg, data)) = bytes.split_first() else {
                        continue;
                    };
                    if !data.is_empty() && self.fail_writes_to == Some(reg) {
                        return Err(FakeError);
                    }
                    ptr = reg;
                    for &val in data {
                        self.regs[ptr as usize] = val;
                        self.log.borrow_mut().push(Event::Write(ptr, val));
                        ptr = ptr.wrapping_add(1);
                    }
                }
                Operation::Read(buf) => {
                    for b in buf.iter_mut() {
                        *b = self.regs[ptr as usize];
                        ptr = ptr.wrapping_add(1);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Records delays in the same log as register writes instead of waiting.
pub struct FakeDelay {
    pub log: Log,
}

impl DelayNs for FakeDelay {
    async fn delay_ns(&mut self, ns: u32) {
        self.log.borrow_mut().push(Event::DelayMs(ns / 1_000_000));
    }

    async fn delay_ms(&mut self, ms: u32) {
        self.log.borrow_mut().push(Event::DelayMs(ms));
    }
}
