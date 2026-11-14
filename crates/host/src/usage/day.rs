//! Where the local day starts, for the estimated spend's "today".
//!
//! "Today" is the calendar day the user is in, so it starts at local
//! midnight rather than 24 hours before now: a day a daylight-saving change
//! falls on runs 23 or 25 hours. The start is the first instant of the civil
//! day in the zone, which is also right in a zone whose clocks skip midnight.

use jiff::Timestamp;
use jiff::tz::TimeZone;

/// Where local midnight falls, for the estimate's "today".
pub trait LocalDay: Send + Sync {
    /// Epoch ms of the local midnight that starts the day `now_ms` is in,
    /// or `None` when it cannot be told.
    fn today_start_ms(&self, now_ms: u64) -> Option<u64>;
}

/// The day as it runs in one given time zone.
#[derive(Debug, Clone)]
pub struct ZonedDay {
    zone: TimeZone,
}

impl ZonedDay {
    pub fn new(zone: TimeZone) -> Self {
        Self { zone }
    }
}

impl LocalDay for ZonedDay {
    fn today_start_ms(&self, now_ms: u64) -> Option<u64> {
        start_of_day(&self.zone, now_ms)
    }
}

/// The day as it runs in the host's own time zone.
///
/// The zone is asked of the system at each reading rather than once at
/// start, so a host whose user changes zone follows it; the system's answer
/// is cached for a few minutes, which keeps a reading every few seconds
/// cheap. A host that cannot tell its zone has no "today".
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemDay;

impl LocalDay for SystemDay {
    fn today_start_ms(&self, now_ms: u64) -> Option<u64> {
        start_of_day(&TimeZone::try_system().ok()?, now_ms)
    }
}

/// Epoch ms of the first instant of the civil day `now_ms` falls on in
/// `zone`, or `None` for a zone nobody can tell or an instant out of range.
fn start_of_day(zone: &TimeZone, now_ms: u64) -> Option<u64> {
    if zone.is_unknown() {
        return None;
    }
    let now = Timestamp::from_millisecond(i64::try_from(now_ms).ok()?).ok()?;
    let start = now.to_zoned(zone.clone()).start_of_day().ok()?;
    u64::try_from(start.timestamp().as_millisecond()).ok()
}

#[cfg(test)]
mod tests {
    use jiff::tz::TimeZone;

    use super::{LocalDay, ZonedDay};

    /// US Eastern as a POSIX rule, which needs no time zone database on any
    /// OS: summer time from 02:00 on the second Sunday of March to 02:00 on
    /// the first Sunday of November.
    const EASTERN: &str = "EST5EDT,M3.2.0,M11.1.0";

    const HOUR: u64 = 3_600_000;

    fn eastern() -> ZonedDay {
        ZonedDay::new(TimeZone::posix(EASTERN).expect("a valid POSIX rule"))
    }

    #[test]
    fn today_starts_at_local_midnight_in_the_zone() {
        // 2026-07-15T14:30 EDT; that day's midnight is 04:00Z.
        let now = 1_784_140_200_000;
        let midnight = 1_784_088_000_000;
        assert_eq!(eastern().today_start_ms(now), Some(midnight));
        // Midnight is the first instant of its own day, and the instant before
        // it is the day before's.
        assert_eq!(eastern().today_start_ms(midnight), Some(midnight));
        assert_eq!(
            eastern().today_start_ms(midnight - 1),
            Some(midnight - 24 * HOUR)
        );
    }

    #[test]
    fn today_starts_at_local_midnight_on_a_day_of_23_hours() {
        // 2026-03-08, when the clocks go forward: 20:00 EDT is 19 hours after
        // that day's midnight (05:00Z), not 20 and not 24.
        let now = 1_773_014_400_000;
        let midnight = 1_772_946_000_000;
        let start = eastern().today_start_ms(now);
        assert_eq!(start, Some(midnight));
        assert_eq!(now - midnight, 19 * HOUR);
        assert_ne!(start, Some(now - 24 * HOUR));
    }

    #[test]
    fn today_starts_at_local_midnight_on_a_day_of_25_hours() {
        // 2026-11-01, when the clocks go back: 22:00 EST is 23 hours after
        // that day's midnight (04:00Z), not 22 and not 24.
        let now = 1_793_588_400_000;
        let midnight = 1_793_505_600_000;
        let start = eastern().today_start_ms(now);
        assert_eq!(start, Some(midnight));
        assert_eq!(now - midnight, 23 * HOUR);
        assert_ne!(start, Some(now - 24 * HOUR));
    }

    #[test]
    fn a_zone_nobody_can_tell_has_no_local_midnight() {
        let unknown = ZonedDay::new(TimeZone::unknown());
        assert_eq!(unknown.today_start_ms(1_784_140_200_000), None);
    }
}
