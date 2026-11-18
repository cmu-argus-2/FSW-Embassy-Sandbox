//! Watchdog task loop.
//!
//! Each cycle services the external hardware watchdog, then connects its reset output if it is not
//! connected yet, as `tasks/watchdog.py` does. The order matters: the watchdog runs whenever the
//! board is powered, and the enable line only connects its reset output to the MCU, so enabling it
//! before the first kick could let a timeout that started at power-up reset the board.
//!
//! The Python flight software gives this task its highest priority (shared only with ADCS), because
//! the HAL monitor once held it off long enough on boot to cause watchdog resets. A binary should run
//! it on the high-priority executor for the same reason.
//!
//! The task is generic over the `fsw-hal` traits and a delay, so it has no idea which board or chip
//! it runs on. Embassy `#[task]` functions cannot be generic, so a binary wraps it in a small task
//! with concrete types.

use fsw_hal::DelayNs;
use fsw_hal::watchdog::Watchdog;

/// The watchdog is serviced at 1 Hz (`TASK_CONFIG` in the Python flight software).
pub const PERIOD_MS: u32 = 1000;

pub struct WatchdogTask<W, D> {
    watchdog: W,
    delay: D,
}

impl<W: Watchdog, D: DelayNs> WatchdogTask<W, D> {
    pub fn new(watchdog: W, delay: D) -> Self {
        Self { watchdog, delay }
    }

    /// Kick the watchdog, then connect it if it is not connected. Returns `true` on the step that
    /// connected it.
    pub fn step(&mut self) -> bool {
        self.watchdog.kick();
        if self.watchdog.is_enabled() {
            false
        } else {
            self.watchdog.set_enabled(true);
            true
        }
    }

    /// Run forever: one [`Self::step`] per period, calling `on_enabled` (logging) on the step that
    /// connects the watchdog. The period is a plain delay after each step, so it drifts by the step
    /// time, which is a pin toggle.
    pub async fn run(&mut self, mut on_enabled: impl FnMut()) -> ! {
        loop {
            if self.step() {
                on_enabled();
            }
            self.delay.delay_ms(PERIOD_MS).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Event {
        Kick,
        Enable(bool),
    }

    /// Records every call in order, up to a fixed number of events.
    struct FakeWatchdog {
        enabled: bool,
        events: [Option<Event>; 8],
        count: usize,
    }

    impl FakeWatchdog {
        fn new(enabled: bool) -> Self {
            Self {
                enabled,
                events: [None; 8],
                count: 0,
            }
        }

        fn record(&mut self, event: Event) {
            self.events[self.count] = Some(event);
            self.count += 1;
        }

        fn events(&self) -> &[Option<Event>] {
            &self.events[..self.count]
        }
    }

    impl Watchdog for FakeWatchdog {
        fn kick(&mut self) {
            self.record(Event::Kick);
        }

        fn set_enabled(&mut self, enabled: bool) {
            self.enabled = enabled;
            self.record(Event::Enable(enabled));
        }

        fn is_enabled(&self) -> bool {
            self.enabled
        }
    }

    struct NoDelay;

    impl DelayNs for NoDelay {
        async fn delay_ns(&mut self, _ns: u32) {}
    }

    #[test]
    fn kicks_before_enabling() {
        let mut task = WatchdogTask::new(FakeWatchdog::new(false), NoDelay);

        assert!(
            task.step(),
            "first step should report that it enabled the watchdog"
        );
        assert_eq!(
            task.watchdog.events(),
            [Some(Event::Kick), Some(Event::Enable(true))]
        );
    }

    #[test]
    fn later_steps_only_kick() {
        let mut task = WatchdogTask::new(FakeWatchdog::new(false), NoDelay);
        task.step();

        assert!(!task.step());
        assert!(!task.step());
        assert_eq!(
            task.watchdog.events(),
            [
                Some(Event::Kick),
                Some(Event::Enable(true)),
                Some(Event::Kick),
                Some(Event::Kick),
            ]
        );
    }

    #[test]
    fn an_already_enabled_watchdog_is_only_kicked() {
        let mut task = WatchdogTask::new(FakeWatchdog::new(true), NoDelay);

        assert!(!task.step());
        assert_eq!(task.watchdog.events(), [Some(Event::Kick)]);
    }

    #[test]
    fn re_enables_a_watchdog_that_was_disconnected() {
        let mut task = WatchdogTask::new(FakeWatchdog::new(false), NoDelay);
        task.step();
        task.watchdog.set_enabled(false);

        assert!(task.step(), "a disconnected watchdog should be reconnected");
        assert!(task.watchdog.is_enabled());
    }
}
