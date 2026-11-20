use chrono::{Datelike, Local, Timelike};
use senti_core::{Clock, LocalDateTime};

/// [`Clock`] backed by the operating system's local time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> LocalDateTime {
        let now = Local::now();
        LocalDateTime {
            year: now.year(),
            month: now.month(),
            day: now.day(),
            hour: now.hour(),
            minute: now.minute(),
            weekday: now.weekday().num_days_from_monday(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_valid_ranges() {
        let now = SystemClock.now();
        assert!((1..=12).contains(&now.month));
        assert!((1..=31).contains(&now.day));
        assert!(now.hour < 24 && now.minute < 60 && now.weekday < 7);
    }
}
