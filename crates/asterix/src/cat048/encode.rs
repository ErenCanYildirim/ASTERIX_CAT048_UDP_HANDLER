//! Encoding CAT048 reports into data blocks.
//!
//! The encoder is the mirror of the decoder: each item type turns back into its
//! fixed-size wire array, the FSPEC is built from which items are present, and
//! records are wrapped in a data block header. Item values are validated when
//! they are constructed, so encoding a `Report` cannot fail; only the block as
//! a whole can (empty, or too large for its 16-bit LEN).

use super::decode::{
    DataSource, FlightLevelReport, G_BIT, L_BIT, MeasuredPosition, Mode3AReport, V_BIT,
};
use super::report::Report;
use super::{CATEGORY, Item, MAX_FSPEC_LEN};
use crate::error::EncodeError;
use crate::format::FX;
use crate::framing::HEADER_LEN;
use crate::units::{AircraftAddress, TimeOfDay, TrackNumber};

/// Low 14 bits of I048/090: the flight level.
const FL_BITS: u16 = 0x3FFF;

fn flag(set: bool, bit: u8) -> u8 {
    if set { bit } else { 0 }
}

impl DataSource {
    fn to_bytes(self) -> [u8; 2] {
        [self.sac, self.sic]
    }
}

impl TimeOfDay {
    fn to_bytes(self) -> [u8; 3] {
        // `from_ticks` guarantees the value is below 2^24, so the top byte is always 0.
        let [_, b0, b1, b2] = self.0.to_be_bytes();
        [b0, b1, b2]
    }
}

impl MeasuredPosition {
    fn to_bytes(self) -> [u8; 4] {
        let [r0, r1] = self.range.0.to_be_bytes();
        let [a0, a1] = self.azimuth.0.to_be_bytes();
        [r0, r1, a0, a1]
    }
}

impl Mode3AReport {
    fn to_bytes(self) -> [u8; 2] {
        let [hi, lo] = self.code.0.to_be_bytes();
        let flags = flag(!self.validated, V_BIT)
            | flag(self.garbled, G_BIT)
            | flag(!self.from_current_scan, L_BIT);
        [hi | flags, lo]
    }
}

impl FlightLevelReport {
    fn to_bytes(self) -> [u8; 2] {
        // Reinterpret the i16 bits as u16 (no `as` cast), then keep the low 14 bits:
        // two's complement truncated to 14 bits is still two's complement.
        let bits = u16::from_be_bytes(self.level.0.to_be_bytes()) & FL_BITS;
        let [hi, lo] = bits.to_be_bytes();
        [
            hi | flag(!self.validated, V_BIT) | flag(self.garbled, G_BIT),
            lo,
        ]
    }
}

impl TrackNumber {
    fn to_bytes(self) -> [u8; 2] {
        self.0.to_be_bytes()
    }
}

impl AircraftAddress {
    fn to_bytes(self) -> [u8; 3] {
        let [_, b0, b1, b2] = self.0.to_be_bytes();
        [b0, b1, b2]
    }
}

/// Write the shortest FSPEC that flags exactly `items`.
fn write_fspec(items: impl Iterator<Item = Item>, out: &mut Vec<u8>) {
    let mut octets = [0u8; MAX_FSPEC_LEN];
    for item in items {
        let index = item.frn() - 1;
        if let Some(octet) = octets.get_mut(index / 7) {
            *octet |= 0x80 >> (index % 7);
        }
    }
    // Trailing all-zero octets are dropped; every octet before the last gets FX.
    let len = octets
        .iter()
        .rposition(|&octet| octet != 0)
        .map_or(1, |last| last + 1);
    for (index, &octet) in octets.iter().take(len).enumerate() {
        out.push(if index + 1 < len { octet | FX } else { octet });
    }
}

impl Report {
    /// Items present in this report, in FRN order.
    fn present_items(&self) -> impl Iterator<Item = Item> {
        [
            Some(Item::DataSourceIdentifier),
            self.time_of_day.map(|_| Item::TimeOfDay),
            self.measured_position.map(|_| Item::MeasuredPositionPolar),
            self.mode_3a.map(|_| Item::Mode3ACode),
            self.flight_level.map(|_| Item::FlightLevel),
            self.aircraft_address.map(|_| Item::AircraftAddress),
            self.track_number.map(|_| Item::TrackNumber),
            self.instrumentation.map(|_| Item::SpecialPurpose),
        ]
        .into_iter()
        .flatten()
    }

    /// Append this report as one CAT048 record (FSPEC + items) to `out`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        write_fspec(self.present_items(), out);

        // Items in FRN order: 010, 140, 040, 070, 090, 220, 161, SP.
        out.extend(self.data_source.to_bytes());
        if let Some(time) = self.time_of_day {
            out.extend(time.to_bytes());
        }
        if let Some(position) = self.measured_position {
            out.extend(position.to_bytes());
        }
        if let Some(mode_3a) = self.mode_3a {
            out.extend(mode_3a.to_bytes());
        }
        if let Some(level) = self.flight_level {
            out.extend(level.to_bytes());
        }
        if let Some(address) = self.aircraft_address {
            out.extend(address.to_bytes());
        }
        if let Some(track) = self.track_number {
            out.extend(track.to_bytes());
        }
        if let Some(instrumentation) = self.instrumentation {
            out.extend(instrumentation.to_bytes());
        }
    }
}

/// Append one CAT048 data block containing `reports` to `out`.
///
/// On error, `out` is left exactly as it was.
pub fn encode_block(reports: &[Report], out: &mut Vec<u8>) -> Result<(), EncodeError> {
    if reports.is_empty() {
        return Err(EncodeError::EmptyBlock);
    }
    let start = out.len();
    out.extend([CATEGORY, 0, 0]); // LEN is patched once the records are written.
    for report in reports {
        report.encode(out);
    }

    let len = out.len() - start;
    let Ok(len_u16) = u16::try_from(len) else {
        out.truncate(start);
        return Err(EncodeError::BlockTooLarge { len });
    };
    if let Some(len_field) = out.get_mut(start + 1..start + HEADER_LEN) {
        for (dst, src) in len_field.iter_mut().zip(len_u16.to_be_bytes()) {
            *dst = src;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::cat048::records;
    use crate::cat048::report::Instrumentation;
    use crate::framing::data_blocks;
    use crate::units::{Azimuth, FlightLevel, Mode3A, SlantRange};

    fn minimal(sic: u8) -> Report {
        Report {
            data_source: DataSource { sac: 25, sic },
            time_of_day: None,
            measured_position: None,
            mode_3a: None,
            flight_level: None,
            aircraft_address: None,
            track_number: None,
            instrumentation: None,
        }
    }

    fn decode_all(datagram: &[u8]) -> Vec<Report> {
        let mut blocks = data_blocks(datagram);
        let block = blocks.next().unwrap().unwrap();
        assert!(blocks.next().is_none(), "expected exactly one block");
        records(&block)
            .unwrap()
            .map(|record| Report::try_from(&record.unwrap()).unwrap())
            .collect()
    }

    /// Encoder output compared byte for byte with a hand-built datagram. A round trip
    /// alone cannot catch a mistake made the same way in both encoder and decoder.
    #[test]
    fn encodes_known_bytes() {
        let report = Report {
            data_source: DataSource { sac: 25, sic: 201 },
            time_of_day: TimeOfDay::from_ticks(0x35_6D4D),
            measured_position: Some(MeasuredPosition {
                range: SlantRange::from_raw(0xC5AF),
                azimuth: Azimuth::from_raw(0xF1E0),
            }),
            mode_3a: Some(Mode3AReport {
                code: Mode3A::new(0o7500).unwrap(),
                validated: true,
                garbled: false,
                from_current_scan: true,
            }),
            flight_level: Some(FlightLevelReport {
                level: FlightLevel::from_quarters(1480).unwrap(),
                validated: true,
                garbled: false,
            }),
            aircraft_address: AircraftAddress::new(0x3C_65AC),
            track_number: TrackNumber::new(300),
            instrumentation: None,
        };
        let mut out = Vec::new();
        encode_block(&[report], &mut out).unwrap();

        // Same bytes as the decode test `record_accessors`.
        assert_eq!(
            out,
            [
                48, 0x00, 0x17, // block header, LEN 23
                0xDD, 0x90, // FSPEC
                0x19, 0xC9, // 010
                0x35, 0x6D, 0x4D, // 140
                0xC5, 0xAF, 0xF1, 0xE0, // 040
                0x0F, 0x40, // 070
                0x05, 0xC8, // 090
                0x3C, 0x65, 0xAC, // 220
                0x01, 0x2C, // 161
            ]
        );
    }

    #[test]
    fn encodes_flags_and_negative_flight_level() {
        let mut report = minimal(1);
        report.mode_3a = Some(Mode3AReport {
            code: Mode3A::new(0o1200).unwrap(),
            validated: false,
            garbled: true,
            from_current_scan: false,
        });
        report.flight_level = Some(FlightLevelReport {
            level: FlightLevel::from_quarters(-4).unwrap(),
            validated: false,
            garbled: true,
        });
        let mut out = Vec::new();
        report.encode(&mut out);

        // FSPEC 0x8C: FRN 1, 5, 6. 070: V+G+L over 0o1200. 090: V+G over 14-bit -4.
        assert_eq!(out, [0x8C, 25, 1, 0xE2, 0x80, 0xFF, 0xFC]);
    }

    #[test]
    fn encodes_instrumentation_in_sp_field() {
        let mut report = minimal(1);
        report.instrumentation = Some(Instrumentation {
            sequence: 0x0102_0304_0506_0708,
            send_time_ns: 0x1112_1314_1516_1718,
        });
        let mut out = Vec::new();
        report.encode(&mut out);

        assert_eq!(
            out,
            [
                0x81, 0x01, 0x01, 0x04, // FSPEC: FRN 1 and FRN 27 (SP)
                25, 1, // 010
                18, 0x01, // SP length, layout tag
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, // sequence
                0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, // send time
            ]
        );
    }

    #[test]
    fn fspec_is_minimal() {
        let mut out = Vec::new();
        minimal(1).encode(&mut out);
        assert_eq!(out.first(), Some(&0x80), "one octet, no FX");
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn unrecognized_sp_layout_is_an_error() {
        let mut dg = Vec::new();
        let mut report = minimal(1);
        report.instrumentation = Some(Instrumentation {
            sequence: 1,
            send_time_ns: 2,
        });
        encode_block(&[report], &mut dg).unwrap();
        // Corrupt the layout tag (block header 3 + FSPEC 4 + 010 2 + SP length 1).
        dg[10] = 0x7F;

        let block = data_blocks(&dg).next().unwrap().unwrap();
        let record = records(&block).unwrap().next().unwrap().unwrap();
        assert_eq!(
            Report::try_from(&record),
            Err(crate::DecodeError::UnrecognizedSpecialPurpose { tag: 0x7F })
        );
    }

    #[test]
    fn missing_data_source_is_an_error() {
        // A record with only I048/140.
        let dg = [48, 0x00, 0x07, 0x40, 0x00, 0x00, 0x01];
        let block = data_blocks(&dg).next().unwrap().unwrap();
        let record = records(&block).unwrap().next().unwrap().unwrap();

        assert_eq!(
            Report::try_from(&record),
            Err(crate::DecodeError::MissingItem {
                item: Item::DataSourceIdentifier
            })
        );
    }

    #[test]
    fn appends_without_touching_existing_bytes() {
        let mut out = vec![0xAA, 0xBB];
        encode_block(&[minimal(1)], &mut out).unwrap();
        assert_eq!(out.get(..2), Some(&[0xAA, 0xBB][..]));
        assert_eq!(decode_all(out.get(2..).unwrap()), [minimal(1)]);
    }

    #[test]
    fn empty_block_is_rejected() {
        let mut out = vec![0xAA];
        assert_eq!(encode_block(&[], &mut out), Err(EncodeError::EmptyBlock));
        assert_eq!(out, [0xAA]);
    }

    #[test]
    fn oversized_block_is_rejected_and_buffer_restored() {
        // 3-byte header + 21 846 records of 3 bytes = 65 541 bytes > 65 535.
        let reports = vec![minimal(1); 21_846];
        let mut out = vec![0xAA];
        assert_eq!(
            encode_block(&reports, &mut out),
            Err(EncodeError::BlockTooLarge { len: 65_541 })
        );
        assert_eq!(out, [0xAA]);
    }

    #[test]
    fn largest_block_that_fits() {
        // 3 + 21 844 * 3 = 65 535: exactly the maximum LEN.
        let reports = vec![minimal(1); 21_844];
        let mut out = Vec::new();
        encode_block(&reports, &mut out).unwrap();
        assert_eq!(out.len(), 65_535);
        assert_eq!(decode_all(&out).len(), 21_844);
    }

    #[test]
    fn constructors_reject_out_of_range_values() {
        assert!(TimeOfDay::from_ticks(TimeOfDay::MAX_TICKS).is_none());
        assert!(FlightLevel::from_quarters(8192).is_none());
        assert!(FlightLevel::from_quarters(-8193).is_none());
        assert!(Mode3A::new(0o10000).is_none());
        assert!(TrackNumber::new(4096).is_none());
        assert!(AircraftAddress::new(0x0100_0000).is_none());
    }

    fn report() -> impl Strategy<Value = Report> {
        (
            any::<(u8, u8)>(),
            proptest::option::of(0..TimeOfDay::MAX_TICKS),
            proptest::option::of(any::<(u16, u16)>()),
            proptest::option::of((0..=0o7777_u16, any::<(bool, bool, bool)>())),
            proptest::option::of((
                FlightLevel::MIN_QUARTERS..=FlightLevel::MAX_QUARTERS,
                any::<(bool, bool)>(),
            )),
            proptest::option::of(0..=0x00FF_FFFF_u32),
            proptest::option::of(0..=0x0FFF_u16),
            proptest::option::of(any::<(u64, u64)>()),
        )
            .prop_map(
                |(source, time, pos, m3a, fl, address, track, instr)| Report {
                    data_source: DataSource {
                        sac: source.0,
                        sic: source.1,
                    },
                    time_of_day: time.map(|t| TimeOfDay::from_ticks(t).unwrap()),
                    measured_position: pos.map(|(range, azimuth)| MeasuredPosition {
                        range: SlantRange::from_raw(range),
                        azimuth: Azimuth::from_raw(azimuth),
                    }),
                    mode_3a: m3a.map(|(code, (validated, garbled, current))| Mode3AReport {
                        code: Mode3A::new(code).unwrap(),
                        validated,
                        garbled,
                        from_current_scan: current,
                    }),
                    flight_level: fl.map(|(quarters, (validated, garbled))| FlightLevelReport {
                        level: FlightLevel::from_quarters(quarters).unwrap(),
                        validated,
                        garbled,
                    }),
                    aircraft_address: address.map(|a| AircraftAddress::new(a).unwrap()),
                    track_number: track.map(|t| TrackNumber::new(t).unwrap()),
                    instrumentation: instr.map(|(sequence, send_time_ns)| Instrumentation {
                        sequence,
                        send_time_ns,
                    }),
                },
            )
    }

    proptest! {
        /// Any valid set of reports survives encode -> frame -> split -> decode unchanged.
        #[test]
        fn round_trip(reports in prop::collection::vec(report(), 1..64)) {
            let mut out = Vec::new();
            encode_block(&reports, &mut out).unwrap();
            prop_assert_eq!(decode_all(&out), reports);
        }
    }
}
