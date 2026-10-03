//! An owned CAT048 target report and the instrumentation carried in its SP field.

use super::decode::{DataSource, FlightLevelReport, MeasuredPosition, Mode3AReport};
use super::{Item, Record};
use crate::error::DecodeError;
use crate::units::{AircraftAddress, TimeOfDay, TrackNumber};

/// Sequence number and send timestamp, carried in the SP (Special Purpose) field.
///
/// Wire layout, all big-endian, 18 bytes including the SP length byte:
///
/// | Offset | Size | Field |
/// | --- | --- | --- |
/// | 0 | 1 | LEN = 18 (explicit-length byte, counts itself) |
/// | 1 | 1 | Layout tag = 0x01 |
/// | 2 | 8 | Sequence number |
/// | 10 | 8 | Send timestamp in nanoseconds |
///
/// The tag lets a receiver tell this layout apart from any other SP content.
/// The timestamp's clock and meaning (intended vs actual send time) are defined
/// by the sender; this crate only carries the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Instrumentation {
    pub sequence: u64,
    pub send_time_ns: u64,
}

impl Instrumentation {
    /// Total SP item length on the wire, including the length byte.
    pub const WIRE_LEN: u8 = 18;
    /// Identifies this layout inside the SP field.
    pub const LAYOUT_TAG: u8 = 0x01;

    pub(crate) fn to_bytes(self) -> [u8; Self::WIRE_LEN as usize] {
        let [s0, s1, s2, s3, s4, s5, s6, s7] = self.sequence.to_be_bytes();
        let [t0, t1, t2, t3, t4, t5, t6, t7] = self.send_time_ns.to_be_bytes();
        [
            Self::WIRE_LEN,
            Self::LAYOUT_TAG,
            s0,
            s1,
            s2,
            s3,
            s4,
            s5,
            s6,
            s7,
            t0,
            t1,
            t2,
            t3,
            t4,
            t5,
            t6,
            t7,
        ]
    }
}

impl TryFrom<[u8; Instrumentation::WIRE_LEN as usize]> for Instrumentation {
    type Error = DecodeError;

    fn try_from(bytes: [u8; Self::WIRE_LEN as usize]) -> Result<Self, Self::Error> {
        // The length byte is already checked: record splitting used it to size this slice.
        let [
            _len,
            tag,
            s0,
            s1,
            s2,
            s3,
            s4,
            s5,
            s6,
            s7,
            t0,
            t1,
            t2,
            t3,
            t4,
            t5,
            t6,
            t7,
        ] = bytes;
        if tag != Self::LAYOUT_TAG {
            return Err(DecodeError::UnrecognizedSpecialPurpose { tag });
        }
        Ok(Self {
            sequence: u64::from_be_bytes([s0, s1, s2, s3, s4, s5, s6, s7]),
            send_time_ns: u64::from_be_bytes([t0, t1, t2, t3, t4, t5, t6, t7]),
        })
    }
}

/// One CAT048 target report with the items this project encodes and decodes.
///
/// I048/010 is mandatory in CAT048, so it is not optional here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Report {
    pub data_source: DataSource,
    pub time_of_day: Option<TimeOfDay>,
    pub measured_position: Option<MeasuredPosition>,
    pub mode_3a: Option<Mode3AReport>,
    pub flight_level: Option<FlightLevelReport>,
    pub aircraft_address: Option<AircraftAddress>,
    pub track_number: Option<TrackNumber>,
    pub instrumentation: Option<Instrumentation>,
}

impl TryFrom<&Record<'_>> for Report {
    type Error = DecodeError;

    /// Items outside this subset are ignored.
    fn try_from(record: &Record<'_>) -> Result<Self, Self::Error> {
        Ok(Self {
            data_source: record.data_source()?.ok_or(DecodeError::MissingItem {
                item: Item::DataSourceIdentifier,
            })?,
            time_of_day: record.time_of_day()?,
            measured_position: record.measured_position()?,
            mode_3a: record.mode_3a()?,
            flight_level: record.flight_level()?,
            aircraft_address: record.aircraft_address()?,
            track_number: record.track_number()?,
            instrumentation: record.instrumentation()?,
        })
    }
}
