use std::sync::OnceLock;
use std::time::Instant;

use crate::adapter::rate_limit_adapter::Millis;
use crate::controller::price_check_controller::Clock;

pub struct SystemClock;

impl SystemClock {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Millis {
        static START: OnceLock<Instant> = OnceLock::new();

        START.get_or_init(Instant::now).elapsed().as_millis() as Millis
    }

    async fn sleep(&self, millis: Millis) {
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_never_runs_backwards() {
        let clock = SystemClock::new();

        let first = clock.now();
        let second = clock.now();

        assert!(second >= first, "{second} came before {first}");
    }

    #[test]
    fn the_first_reading_is_near_zero() {
        assert!(SystemClock::new().now() < 60_000);
    }

    #[test]
    fn default_matches_new() {
        let made: SystemClock = Default::default();

        assert!(made.now() < 60_000);
    }
}
