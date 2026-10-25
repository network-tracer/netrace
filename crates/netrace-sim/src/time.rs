use std::fmt;
use std::ops::{Add, Sub};
use std::time::Duration;

/// A point on the simulation's virtual timeline, in nanoseconds since start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SimTime(u64);

impl SimTime {
    pub const ZERO: SimTime = SimTime(0);
    pub const MAX: SimTime = SimTime(u64::MAX);

    pub const fn from_nanos(nanos: u64) -> Self {
        SimTime(nanos)
    }

    pub const fn from_millis(millis: u64) -> Self {
        SimTime(millis * 1_000_000)
    }

    pub const fn from_secs(secs: u64) -> Self {
        SimTime(secs * 1_000_000_000)
    }

    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    pub const fn as_millis(self) -> u64 {
        self.0 / 1_000_000
    }

    /// Time elapsed since `earlier`, or zero if `earlier` is later than `self`.
    pub fn saturating_since(self, earlier: SimTime) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }
}

impl Add<Duration> for SimTime {
    type Output = SimTime;

    /// Saturates at [`SimTime::MAX`] rather than overflowing.
    fn add(self, rhs: Duration) -> SimTime {
        let nanos = u64::try_from(rhs.as_nanos()).unwrap_or(u64::MAX);
        SimTime(self.0.saturating_add(nanos))
    }
}

impl Sub for SimTime {
    type Output = Duration;

    fn sub(self, rhs: SimTime) -> Duration {
        self.saturating_since(rhs)
    }
}

impl fmt::Display for SimTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let millis = self.0 / 1_000_000;
        let micros = (self.0 / 1_000) % 1_000;
        write!(f, "{millis}.{micros:03} ms")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_conversions() {
        assert_eq!(SimTime::from_secs(2), SimTime::from_millis(2_000));
        assert_eq!(SimTime::from_millis(3).as_nanos(), 3_000_000);
        assert_eq!(SimTime::from_nanos(7_500_000).as_millis(), 7);
    }

    #[test]
    fn adding_duration_saturates() {
        assert_eq!(SimTime::MAX + Duration::from_secs(1), SimTime::MAX);
        assert_eq!(
            SimTime::from_millis(1) + Duration::from_millis(2),
            SimTime::from_millis(3)
        );
    }

    #[test]
    fn subtraction_never_underflows() {
        let a = SimTime::from_millis(5);
        let b = SimTime::from_millis(8);
        assert_eq!(b - a, Duration::from_millis(3));
        assert_eq!(a - b, Duration::ZERO);
    }

    #[test]
    fn display_is_millis_with_micros() {
        assert_eq!(SimTime::from_nanos(12_345_678).to_string(), "12.345 ms");
    }
}
