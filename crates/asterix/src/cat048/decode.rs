//! Typed decoding of the core CAT048 items.
//!
//! Each item type converts from a fixed-size byte array with `From` (cannot
//! fail) or `TryFrom` (can fail on an out-of-range value). `Record` gets one
//! accessor per item that returns `Ok(None)` when the item is absent.

use core::convert::Infallible;

use super::report::Instrumentation;
use super::{Item, Record};
use crate::error::DecodeError;
use crate::units::{
    AircraftAddress, Azimuth, FlightLevel, Mode3A, SlantRange, TimeOfDay, TrackNumber,
};

/// Bit 8 of the first octet in I048/070 and I048/090: 1 = code not validated.
pub(super) const V_BIT: u8 = 0x80;
/// Bit 7 of the first octet in I048/070 and I048/090: 1 = garbled code.
pub(super) const G_BIT: u8 = 0x40;
/// Bit 6 of the first octet in I048/070: 1 = not extracted during the last scan.
pub(super) const L_BIT: u8 = 0x20;
/// Low 12 bits: Mode-3/A code (I048/070) and track number (I048/161).
const LOW_12_BITS: u16 = 0x0FFF;

/// I048/010 Data Source Identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DataSource {
    /// System Area Code.
    pub sac: u8,
    /// System Identification Code.
    pub sic: u8,
}

impl From<[u8; 2]> for DataSource {
    fn from([sac, sic]: [u8; 2]) -> Self {
        Self { sac, sic }
    }
}

impl TryFrom<[u8; 3]> for TimeOfDay {
    type Error = DecodeError;

    /// I048/140: 24-bit unsigned count of 1/128 s since midnight.
    fn try_from([b0, b1, b2]: [u8; 3]) -> Result<Self, Self::Error> {
        let ticks = u32::from_be_bytes([0, b0, b1, b2]);
        Self::from_ticks(ticks).ok_or(DecodeError::TimeOutOfRange { ticks })
    }
}

/// I048/040 Measured Position in Polar Co-ordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeasuredPosition {
    pub range: SlantRange,
    pub azimuth: Azimuth,
}

impl From<[u8; 4]> for MeasuredPosition {
    fn from([r0, r1, a0, a1]: [u8; 4]) -> Self {
        Self {
            range: SlantRange(u16::from_be_bytes([r0, r1])),
            azimuth: Azimuth(u16::from_be_bytes([a0, a1])),
        }
    }
}

/// I048/070 Mode-3/A Code, with its validity flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mode3AReport {
    pub code: Mode3A,
    /// V = 0: code validated.
    pub validated: bool,
    /// G = 1: garbled code.
    pub garbled: bool,
    /// L = 0: code derived from the transponder reply in this scan (not a previous one).
    pub from_current_scan: bool,
}

impl From<[u8; 2]> for Mode3AReport {
    fn from([b0, b1]: [u8; 2]) -> Self {
        Self {
            code: Mode3A(u16::from_be_bytes([b0, b1]) & LOW_12_BITS),
            validated: b0 & V_BIT == 0,
            garbled: b0 & G_BIT != 0,
            from_current_scan: b0 & L_BIT == 0,
        }
    }
}

/// I048/090 Flight Level in Binary Representation, with its validity flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlightLevelReport {
    pub level: FlightLevel,
    /// V = 0: code validated.
    pub validated: bool,
    /// G = 1: garbled code.
    pub garbled: bool,
}

impl From<[u8; 2]> for FlightLevelReport {
    fn from([b0, b1]: [u8; 2]) -> Self {
        // FL is a 14-bit two's complement number below the V and G bits. Shift it
        // left so its sign bit becomes bit 16, reinterpret as i16, then shift right
        // arithmetically: the sign is copied back into the top two bits.
        let shifted = (u16::from_be_bytes([b0, b1]) << 2).to_be_bytes();
        let quarters = i16::from_be_bytes(shifted) >> 2;
        Self {
            level: FlightLevel(quarters),
            validated: b0 & V_BIT == 0,
            garbled: b0 & G_BIT != 0,
        }
    }
}

impl From<[u8; 2]> for TrackNumber {
    /// I048/161: 4 spare bits, then a 12-bit track number. Spare bits are ignored.
    fn from(bytes: [u8; 2]) -> Self {
        Self(u16::from_be_bytes(bytes) & LOW_12_BITS)
    }
}

impl From<[u8; 3]> for AircraftAddress {
    /// I048/220: 24-bit ICAO address.
    fn from([b0, b1, b2]: [u8; 3]) -> Self {
        Self(u32::from_be_bytes([0, b0, b1, b2]))
    }
}

impl From<Infallible> for DecodeError {
    fn from(never: Infallible) -> Self {
        match never {}
    }
}

impl Record<'_> {
    /// Decode `item` into `T` if the record contains it.
    fn decode<const N: usize, T>(&self, item: Item) -> Result<Option<T>, DecodeError>
    where
        T: TryFrom<[u8; N]>,
        DecodeError: From<T::Error>,
    {
        let Some(bytes) = self.item(item) else {
            return Ok(None);
        };
        let array = <[u8; N]>::try_from(bytes).map_err(|_| DecodeError::WrongLength {
            item,
            expected: N,
            actual: bytes.len(),
        })?;
        Ok(Some(T::try_from(array)?))
    }

    /// I048/010 Data Source Identifier.
    pub fn data_source(&self) -> Result<Option<DataSource>, DecodeError> {
        self.decode(Item::DataSourceIdentifier)
    }

    /// I048/140 Time of Day.
    pub fn time_of_day(&self) -> Result<Option<TimeOfDay>, DecodeError> {
        self.decode(Item::TimeOfDay)
    }

    /// I048/040 Measured Position in Polar Co-ordinates.
    pub fn measured_position(&self) -> Result<Option<MeasuredPosition>, DecodeError> {
        self.decode(Item::MeasuredPositionPolar)
    }

    /// I048/070 Mode-3/A Code.
    pub fn mode_3a(&self) -> Result<Option<Mode3AReport>, DecodeError> {
        self.decode(Item::Mode3ACode)
    }

    /// I048/090 Flight Level.
    pub fn flight_level(&self) -> Result<Option<FlightLevelReport>, DecodeError> {
        self.decode(Item::FlightLevel)
    }

    /// I048/161 Track Number.
    pub fn track_number(&self) -> Result<Option<TrackNumber>, DecodeError> {
        self.decode(Item::TrackNumber)
    }

    /// I048/220 Aircraft Address.
    pub fn aircraft_address(&self) -> Result<Option<AircraftAddress>, DecodeError> {
        self.decode(Item::AircraftAddress)
    }

    /// Sequence number and send timestamp carried in the SP field.
    pub fn instrumentation(&self) -> Result<Option<Instrumentation>, DecodeError> {
        self.decode(Item::SpecialPurpose)
    }
}

// Expected values are exact binary fractions, so exact float comparison is correct here.
#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::cat048::records;
    use crate::framing::data_blocks;

    #[test]
    fn data_source() {
        assert_eq!(
            DataSource::from([0x19, 0xC9]),
            DataSource { sac: 25, sic: 201 }
        );
    }

    #[test]
    fn time_of_day() {
        // 0x356D4D = 53 * 65 536 + 0x6D4D = 3 501 389 ticks = 27 354 s + 77/128 s.
        let time = TimeOfDay::try_from([0x35, 0x6D, 0x4D]).unwrap();
        assert_eq!(time.ticks(), 3_501_389);
        assert_eq!(time.seconds(), 27_354.601_562_5);
        assert_eq!(time.as_duration().as_nanos(), 27_354_601_562_500);
        assert_eq!(time.to_string(), "07:35:54.601");
    }

    #[test]
    fn time_of_day_last_tick_of_the_day() {
        // 0xA8BFFF = 86 400 * 128 - 1.
        let time = TimeOfDay::try_from([0xA8, 0xBF, 0xFF]).unwrap();
        assert_eq!(time.to_string(), "23:59:59.992");
    }

    #[test]
    fn time_of_day_out_of_range() {
        // 0xA8C000 = 86 400 * 128: exactly one day, which is not a time of day.
        assert_eq!(
            TimeOfDay::try_from([0xA8, 0xC0, 0x00]),
            Err(DecodeError::TimeOutOfRange { ticks: 11_059_200 })
        );
    }

    #[test]
    fn measured_position() {
        // RHO 0xC5AF = 50 607 / 256 NM. THETA 0xF1E0 = 61 920 * 360 / 65 536 deg.
        let pos = MeasuredPosition::from([0xC5, 0xAF, 0xF1, 0xE0]);
        assert_eq!(pos.range.nautical_miles(), 197.683_593_75);
        assert_eq!(pos.azimuth.degrees(), 340.136_718_75);
    }

    #[test]
    fn azimuth_top_of_scale() {
        // 0xFFFF is one LSB short of 360 degrees, never 360 itself.
        let pos = MeasuredPosition::from([0, 0, 0xFF, 0xFF]);
        assert_eq!(pos.azimuth.degrees(), 360.0 - 360.0 / 65_536.0);
    }

    #[test]
    fn mode_3a_hijack_code() {
        // Squawk 7500: 0o7500 = 0xF40.
        let report = Mode3AReport::from([0x0F, 0x40]);
        assert_eq!(report.code.code(), 0o7500);
        assert_eq!(report.code.to_string(), "7500");
        assert!(report.validated);
        assert!(!report.garbled);
        assert!(report.from_current_scan);
    }

    #[test]
    fn mode_3a_flags_do_not_leak_into_code() {
        // V, G and L all set, plus the spare bit, around squawk 1200.
        let report = Mode3AReport::from([0xF2, 0x80]);
        assert_eq!(report.code.to_string(), "1200");
        assert!(!report.validated);
        assert!(report.garbled);
        assert!(!report.from_current_scan);
    }

    #[test]
    fn mode_3a_leading_zero() {
        assert_eq!(Mode3AReport::from([0x00, 0x08]).code.to_string(), "0010");
    }

    #[test]
    fn flight_level_positive() {
        // 0x05C8 = 1480 quarters = FL370 = 37 000 ft.
        let report = FlightLevelReport::from([0x05, 0xC8]);
        assert_eq!(report.level.quarters(), 1480);
        assert_eq!(report.level.flight_level(), 370.0);
        assert_eq!(report.level.feet(), 37_000.0);
        assert_eq!(report.level.to_string(), "FL370");
        assert!(report.validated);
        assert!(!report.garbled);
    }

    #[test]
    fn flight_level_negative() {
        // 14-bit 0x3FFC = 16 380 = 2^14 - 4, i.e. -4 quarters = FL-1 = -100 ft.
        let report = FlightLevelReport::from([0x3F, 0xFC]);
        assert_eq!(report.level.quarters(), -4);
        assert_eq!(report.level.feet(), -100.0);
        assert_eq!(report.level.to_string(), "FL-1");
    }

    #[test]
    fn flight_level_flags_do_not_affect_sign() {
        // Same -4 quarters, but with V and G set: the flags sit above the sign bit.
        let report = FlightLevelReport::from([0xFF, 0xFC]);
        assert_eq!(report.level.quarters(), -4);
        assert!(!report.validated);
        assert!(report.garbled);

        // A positive value with V set must stay positive.
        assert_eq!(FlightLevelReport::from([0x85, 0xC8]).level.quarters(), 1480);
    }

    #[test]
    fn flight_level_extremes() {
        assert_eq!(FlightLevelReport::from([0x1F, 0xFF]).level.quarters(), 8191);
        assert_eq!(
            FlightLevelReport::from([0x20, 0x00]).level.quarters(),
            -8192
        );
        assert_eq!(
            FlightLevelReport::from([0x3F, 0xFF]).level.to_string(),
            "FL-0.25"
        );
    }

    #[test]
    fn track_number_ignores_spare_bits() {
        assert_eq!(TrackNumber::from([0x0F, 0xFF]).get(), 4095);
        assert_eq!(TrackNumber::from([0xF0, 0x2A]).get(), 42);
    }

    #[test]
    fn aircraft_address() {
        let address = AircraftAddress::from([0x3C, 0x65, 0xAC]);
        assert_eq!(address.get(), 0x3C_65AC);
        assert_eq!(address.to_string(), "3C65AC");
        assert_eq!(
            AircraftAddress::from([0x00, 0x0A, 0xBC]).to_string(),
            "000ABC"
        );
    }

    #[test]
    fn record_accessors() {
        // FSPEC: FRN 1, 2, 4, 5, 6 + FX; FRN 8, 11.
        let dg = [
            48, 0x00, 0x17, // block header, LEN 23
            0xDD, 0x90, // FSPEC
            0x19, 0xC9, // 010
            0x35, 0x6D, 0x4D, // 140
            0xC5, 0xAF, 0xF1, 0xE0, // 040
            0x0F, 0x40, // 070
            0x05, 0xC8, // 090
            0x3C, 0x65, 0xAC, // 220
            0x01, 0x2C, // 161
        ];
        let block = data_blocks(&dg).next().unwrap().unwrap();
        let record = records(&block).unwrap().next().unwrap().unwrap();

        assert_eq!(record.data_source().unwrap().unwrap().sic, 201);
        assert_eq!(record.time_of_day().unwrap().unwrap().ticks(), 3_501_389);
        assert_eq!(
            record.measured_position().unwrap().unwrap().range.raw(),
            0xC5AF
        );
        assert_eq!(record.mode_3a().unwrap().unwrap().code.to_string(), "7500");
        assert_eq!(
            record.flight_level().unwrap().unwrap().level.to_string(),
            "FL370"
        );
        assert_eq!(
            record.aircraft_address().unwrap().unwrap().to_string(),
            "3C65AC"
        );
        assert_eq!(record.track_number().unwrap().unwrap().get(), 300);
    }

    #[test]
    fn absent_item_is_ok_none() {
        let dg = [48, 0x00, 0x06, 0x80, 0x19, 0xC9];
        let block = data_blocks(&dg).next().unwrap().unwrap();
        let record = records(&block).unwrap().next().unwrap().unwrap();

        assert_eq!(record.flight_level(), Ok(None));
    }

    #[test]
    fn out_of_range_value_surfaces_through_record() {
        let dg = [48, 0x00, 0x07, 0x40, 0xA8, 0xC0, 0x00];
        let block = data_blocks(&dg).next().unwrap().unwrap();
        let record = records(&block).unwrap().next().unwrap().unwrap();

        assert_eq!(
            record.time_of_day(),
            Err(DecodeError::TimeOutOfRange { ticks: 11_059_200 })
        );
    }
}
