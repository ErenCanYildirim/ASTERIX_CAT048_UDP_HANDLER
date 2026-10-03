//! Newtypes for ASTERIX physical quantities and codes.
//! Each type stores the raw wire value as an integer, exactly as transmitted,
//! and converts to physical units only on request. That keeps decoding lossless
//! and makes the unit part of the type: a `FlightLevel` cannot be passed where a
//! range or a height in feet is expected.

use core::fmt;
use core::time::Duration;

/// Time of day since midnight UTC, in units of 1/128 s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeOfDay(pub(crate) u32);

impl TimeOfDay {
    /// Ticks per second: the wire LSB is 1/128 s.
    pub const TICKS_PER_SECOND: u32 = 128;
    /// Exclusive upper bound: one day.
    pub const MAX_TICKS: u32 = 86_400 * Self::TICKS_PER_SECOND;
    /// 1/128 s is exactly 7 812 500 ns, so the conversion to `Duration` is lossless.
    const NANOS_PER_TICK: u64 = 7_812_500;

    /// Raw value in 1/128 s.
    #[must_use]
    pub fn ticks(self) -> u32 {
        self.0
    }

    /// Seconds since midnight.
    #[must_use]
    pub fn seconds(self) -> f64 {
        f64::from(self.0) / f64::from(Self::TICKS_PER_SECOND)
    }

    /// Exact time since midnight.
    #[must_use]
    pub fn as_duration(self) -> Duration {
        Duration::from_nanos(u64::from(self.0) * Self::NANOS_PER_TICK)
    }
}

impl fmt::Display for TimeOfDay {
    /// `HH:MM:SS.mmm`, milliseconds truncated.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let secs = self.0 / Self::TICKS_PER_SECOND;
        let millis = (self.0 % Self::TICKS_PER_SECOND) * 1000 / Self::TICKS_PER_SECOND;
        let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
        write!(f, "{h:02}:{m:02}:{s:02}.{millis:03}")
    }
}

/// Slant range from the radar, in units of 1/256 NM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlantRange(pub(crate) u16);

impl SlantRange {
    /// Raw value in 1/256 NM.
    #[must_use]
    pub fn raw(self) -> u16 {
        self.0
    }

    /// Range in nautical miles.
    #[must_use]
    pub fn nautical_miles(self) -> f64 {
        f64::from(self.0) / 256.0
    }
}

/// Azimuth clockwise from north, in units of 360/2^16 degrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Azimuth(pub(crate) u16);

impl Azimuth {
    /// Raw value in 360/2^16 degrees.
    #[must_use]
    pub fn raw(self) -> u16 {
        self.0
    }

    /// Azimuth in degrees, in `[0, 360)`.
    #[must_use]
    pub fn degrees(self) -> f64 {
        f64::from(self.0) * 360.0 / 65_536.0
    }
}

/// Barometric flight level, in units of 1/4 FL (25 ft). May be negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FlightLevel(pub(crate) i16);

impl FlightLevel {
    /// Raw value in quarter flight levels.
    #[must_use]
    pub fn quarters(self) -> i16 {
        self.0
    }

    /// Flight level (hundreds of feet), e.g. `370.0` for FL370.
    #[must_use]
    pub fn flight_level(self) -> f64 {
        f64::from(self.0) / 4.0
    }

    /// Pressure altitude in feet. Explicit, because a flight level is not a height in feet.
    #[must_use]
    pub fn feet(self) -> f64 {
        f64::from(self.0) * 25.0
    }
}

impl fmt::Display for FlightLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FL{}", self.flight_level())
    }
}

/// A Mode-3/A identity code ("squawk"): 12 bits, conventionally written as 4 octal digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mode3A(pub(crate) u16);

impl Mode3A {
    /// The 12-bit code as a number. `0o7500` for squawk 7500.
    #[must_use]
    pub fn code(self) -> u16 {
        self.0
    }
}

impl fmt::Display for Mode3A {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04o}", self.0)
    }
}

/// A 12-bit track number assigned by the radar's local tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackNumber(pub(crate) u16);

impl TrackNumber {
    #[must_use]
    pub fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for TrackNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A 24-bit ICAO aircraft address (Mode S address), conventionally written in hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AircraftAddress(pub(crate) u32);

impl AircraftAddress {
    #[must_use]
    pub fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for AircraftAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:06X}", self.0)
    }
}
