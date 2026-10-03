//! CATO048 record splitting, FSPEC parsing
//!
//! A CAT048 block holds one or more records back to back, with no length field per record
//! Each record starts with an FSPEC, followed by items in UAP order
//!
use core::fmt;
use core::iter::FusedIterator;

use crate::error::RecordError;
use crate::format::{FX, Format, LenError, item_len};
use crate::framing::{DataBlock, HEADER_LEN};

/// The ASTERIX category handled by this module.
pub const CATEGORY: u8 = 48;

/// Number of items in the CAT048 UAP (FRN 1 to 28).
pub const UAP_LEN: usize = 28;

/// Maximum FSPEC length: 7 item bits per octet, so 4 octets cover 28 FRNs.
pub const MAX_FSPEC_LEN: usize = UAP_LEN.div_ceil(7);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    DataSourceIdentifier,
    TimeOfDay,
    TargetReportDescriptor,
    MeasuredPositionPolar,
    Mode3ACode,
    FlightLevel,
    RadarPlotCharacteristics,
    AircraftAddress,
    AircraftIdentification,
    BdsRegisterData,
    TrackNumber,
    CalculatedPositionCartesian,
    CalculatedTrackVelocity,
    TrackStatus,
    TrackQuality,
    WarningErrorConditions,
    Mode3ACodeConfidence,
    ModeCCodeConfidence,
    Height3dRadar,
    RadialDopplerSpeed,
    AcasCapabilityFlightStatus,
    AcasResolutionAdvisory,
    Mode1Code,
    Mode2Code,
    Mode1CodeConfidence,
    Mode2CodeConfidence,
    SpecialPurpose,
    ReservedExpansion,
}

impl Item {
    /// Field Reference Number: the item's 1-based position in the FSPEC.
    #[must_use]
    pub fn frn(self) -> usize {
        self as usize + 1
    }

    /// The item's identifier in the Eurocontrol specification.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::DataSourceIdentifier => "I048/010",
            Self::TimeOfDay => "I048/140",
            Self::TargetReportDescriptor => "I048/020",
            Self::MeasuredPositionPolar => "I048/040",
            Self::Mode3ACode => "I048/070",
            Self::FlightLevel => "I048/090",
            Self::RadarPlotCharacteristics => "I048/130",
            Self::AircraftAddress => "I048/220",
            Self::AircraftIdentification => "I048/240",
            Self::BdsRegisterData => "I048/250",
            Self::TrackNumber => "I048/161",
            Self::CalculatedPositionCartesian => "I048/042",
            Self::CalculatedTrackVelocity => "I048/200",
            Self::TrackStatus => "I048/170",
            Self::TrackQuality => "I048/210",
            Self::WarningErrorConditions => "I048/030",
            Self::Mode3ACodeConfidence => "I048/080",
            Self::ModeCCodeConfidence => "I048/100",
            Self::Height3dRadar => "I048/110",
            Self::RadialDopplerSpeed => "I048/120",
            Self::AcasCapabilityFlightStatus => "I048/230",
            Self::AcasResolutionAdvisory => "I048/260",
            Self::Mode1Code => "I048/055",
            Self::Mode2Code => "I048/050",
            Self::Mode1CodeConfidence => "I048/065",
            Self::Mode2CodeConfidence => "I048/060",
            Self::SpecialPurpose => "I048/SP",
            Self::ReservedExpansion => "I048/RE",
        }
    }
}

impl fmt::Display for Item {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (FRN {})", self.id(), self.frn())
    }
}

/// One UAP row: which item an FRN refers to and how it is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UapEntry {
    pub item: Item,
    pub format: Format,
}

const fn entry(item: Item, format: Format) -> UapEntry {
    UapEntry { item, format }
}

/// The CAT048 User Application Profile, in FRN order (index = FRN - 1).
pub const UAP: [UapEntry; UAP_LEN] = [
    entry(Item::DataSourceIdentifier, Format::Fixed(2)),
    entry(Item::TimeOfDay, Format::Fixed(3)),
    entry(Item::TargetReportDescriptor, Format::Extended),
    entry(Item::MeasuredPositionPolar, Format::Fixed(4)),
    entry(Item::Mode3ACode, Format::Fixed(2)),
    entry(Item::FlightLevel, Format::Fixed(2)),
    // SRL, SRR, SAM, PRL, PAM, RPD, APD: one byte each.
    entry(
        Item::RadarPlotCharacteristics,
        Format::Compound(&[Format::Fixed(1); 7]),
    ),
    entry(Item::AircraftAddress, Format::Fixed(3)),
    entry(Item::AircraftIdentification, Format::Fixed(6)),
    // REP x (56-bit MB data + BDS1/BDS2).
    entry(Item::BdsRegisterData, Format::Repetitive(8)),
    entry(Item::TrackNumber, Format::Fixed(2)),
    entry(Item::CalculatedPositionCartesian, Format::Fixed(4)),
    entry(Item::CalculatedTrackVelocity, Format::Fixed(4)),
    entry(Item::TrackStatus, Format::Extended),
    entry(Item::TrackQuality, Format::Fixed(4)),
    entry(Item::WarningErrorConditions, Format::Extended),
    entry(Item::Mode3ACodeConfidence, Format::Fixed(2)),
    entry(Item::ModeCCodeConfidence, Format::Fixed(4)),
    entry(Item::Height3dRadar, Format::Fixed(2)),
    // Subfield 1: calculated Doppler speed. Subfield 2: REP x raw Doppler speed.
    entry(
        Item::RadialDopplerSpeed,
        Format::Compound(&[Format::Fixed(2), Format::Repetitive(6)]),
    ),
    entry(Item::AcasCapabilityFlightStatus, Format::Fixed(2)),
    entry(Item::AcasResolutionAdvisory, Format::Fixed(7)),
    entry(Item::Mode1Code, Format::Fixed(1)),
    entry(Item::Mode2Code, Format::Fixed(2)),
    entry(Item::Mode1CodeConfidence, Format::Fixed(1)),
    entry(Item::Mode2CodeConfidence, Format::Fixed(2)),
    entry(Item::SpecialPurpose, Format::Explicit),
    entry(Item::ReservedExpansion, Format::Explicit),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record<'a> {
    offset: usize,
    bytes: &'a [u8],
    items: [Option<&'a [u8]>; UAP_LEN],
}

impl<'a> Record<'a> {
    /// Byte offset of this record (its FSPEC) within the datagram.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The whole record, FSPEC included.
    #[must_use]
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// The raw bytes of `item`, if the record contains it.
    #[must_use]
    pub fn item(&self, item: Item) -> Option<&'a [u8]> {
        self.items.get(item as usize).copied().flatten()
    }

    /// All items present in the record, in FRN order.
    pub fn items(&self) -> impl Iterator<Item = (Item, &'a [u8])> + '_ {
        UAP.iter()
            .zip(self.items.iter())
            .filter_map(|(entry, slot)| slot.map(|bytes| (entry.item, bytes)))
    }
}

/// Iterate over the records in a CAT048 data block
/// Fails up front if the block is not CAT048, like block framing, iteration yields one `Err` on the first malformed record and then stops
pub fn records<'a>(block: &DataBlock<'a>) -> Result<Records<'a>, RecordError> {
    if block.category() != CATEGORY {
        return Err(RecordError::WrongCategory {
            offset: block.offset(),
            category: block.category(),
        });
    }
    Ok(Records {
        remaining: block.records(),
        offset: block.offset() + HEADER_LEN,
        done: false,
    })
}

/// Iterator returned by [`records`].
#[derive(Debug, Clone)]
pub struct Records<'a> {
    remaining: &'a [u8],
    offset: usize,
    done: bool,
}

impl<'a> Records<'a> {
    fn parse_record(&mut self) -> Result<Record<'a>, RecordError> {
        let offset = self.offset;
        let bytes = self.remaining;

        let fspec_len = fspec_len(bytes, offset)?;
        let (fspec, mut rest) = bytes
            .split_at_checked(fspec_len)
            .ok_or(RecordError::TruncatedFspec { offset })?;
        if fspec.iter().all(|octet| octet & !FX == 0) {
            return Err(RecordError::EmptyRecord { offset });
        }

        let mut items = [None; UAP_LEN];
        for (index, (uap, slot)) in UAP.iter().zip(items.iter_mut()).enumerate() {
            if !fspec_has(fspec, index) {
                continue;
            }
            let item_offset = offset + (bytes.len() - rest.len());
            let len_error = |err| record_error(err, item_offset, uap.item, rest.len());
            let len = item_len(uap.format, rest).map_err(len_error)?;
            let (item, after) = rest
                .split_at_checked(len)
                .ok_or_else(|| len_error(LenError::Truncated { needed: len }))?;
            *slot = Some(item);
            rest = after;
        }

        // `rest` is always a suffix of `bytes`, so this split cannot fail.
        let consumed = bytes.len() - rest.len();
        let record = bytes.get(..consumed).unwrap_or_default();
        self.remaining = rest;
        self.offset += consumed;
        Ok(Record {
            offset,
            bytes: record,
            items,
        })
    }
}

impl<'a> Iterator for Records<'a> {
    type Item = Result<Record<'a>, RecordError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.remaining.is_empty() {
            self.done = true;
            return None;
        }
        let result = self.parse_record();
        if result.is_err() {
            self.done = true;
        }
        Some(result)
    }
}

impl FusedIterator for Records<'_> {}

fn fspec_len(bytes: &[u8], offset: usize) -> Result<usize, RecordError> {
    for (index, octet) in bytes.iter().take(MAX_FSPEC_LEN).enumerate() {
        if octet & FX == 0 {
            return Ok(index + 1);
        }
        if index + 1 == MAX_FSPEC_LEN {
            return Err(RecordError::FspecTooLong { offset });
        }
    }
    Err(RecordError::TruncatedFspec { offset })
}

fn fspec_has(fspec: &[u8], index: usize) -> bool {
    fspec
        .get(index / 7)
        .is_some_and(|octet| octet & (0x80 >> (index % 7)) != 0)
}

fn record_error(err: LenError, offset: usize, item: Item, available: usize) -> RecordError {
    match err {
        LenError::Truncated { needed } => RecordError::TruncatedItem {
            offset,
            item,
            needed,
            available,
        },
        LenError::ZeroExplicitLength => RecordError::ZeroExplicitLength { offset, item },
        LenError::UndefinedSubfield { subfield } => RecordError::UndefinedSubfield {
            offset,
            item,
            subfield,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::data_blocks;

    /// Wrap record bytes in a CAT048 data block header.
    fn datagram(records: &[u8]) -> Vec<u8> {
        let len = u16::try_from(records.len() + HEADER_LEN).unwrap();
        let mut out = vec![CATEGORY];
        out.extend(len.to_be_bytes());
        out.extend(records);
        out
    }

    fn parse(datagram: &[u8]) -> Vec<Result<Record<'_>, RecordError>> {
        let block = data_blocks(datagram).next().unwrap().unwrap();
        records(&block).unwrap().collect()
    }

    fn single(datagram: &[u8]) -> Record<'_> {
        let mut records = parse(datagram);
        assert_eq!(records.len(), 1, "expected exactly one record");
        records.remove(0).unwrap()
    }

    #[test]
    fn uap_is_in_frn_order() {
        for (index, entry) in UAP.iter().enumerate() {
            assert_eq!(entry.item as usize, index, "{} is out of order", entry.item);
        }
    }

    #[test]
    fn record_with_every_item() {
        // Byte length each item takes in this record, in FRN order. Extended items
        // use one octet, I048/250 has REP = 1, compounds flag one subfield, SP has LEN = 2.
        const LENS: [usize; UAP_LEN] = [
            2, 3, 1, 4, 2, 2, 2, // FRN 1-7
            3, 6, 9, 2, 4, 4, 1, // FRN 8-14
            4, 1, 2, 4, 2, 3, 2, // FRN 15-21
            7, 1, 2, 1, 2, 2, 1, // FRN 22-28
        ];
        let mut bytes = vec![0xFF, 0xFF, 0xFF, 0xFE]; // all 28 FRNs
        for (entry, &len) in UAP.iter().zip(&LENS) {
            let mut item = vec![0; len];
            match entry.item {
                Item::BdsRegisterData | Item::ReservedExpansion => item[0] = 1,
                Item::RadarPlotCharacteristics | Item::RadialDopplerSpeed => item[0] = 0x80,
                Item::SpecialPurpose => item[0] = 2,
                _ => {}
            }
            bytes.extend(item);
        }
        let dg = datagram(&bytes);
        let record = single(&dg);

        for (entry, &len) in UAP.iter().zip(&LENS) {
            assert_eq!(
                record.item(entry.item).map(<[u8]>::len),
                Some(len),
                "{}",
                entry.item
            );
        }
        assert_eq!(record.items().count(), UAP_LEN);
    }

    #[test]
    fn fixed_and_extended_items() {
        // FSPEC 0xF0: FRN 1-4 (I048/010, /140, /020, /040).
        let dg = datagram(&[
            0xF0, // FSPEC
            0x19, 0xC9, // 010
            0x35, 0x6D, 0x4D, // 140
            0xA1, 0x40, // 020: two octets, FX set on the first
            0xC5, 0xAF, 0xF1, 0xE0, // 040
        ]);
        let record = single(&dg);

        assert_eq!(record.offset(), 3);
        assert_eq!(record.bytes().len(), 12);
        assert_eq!(
            record.item(Item::DataSourceIdentifier),
            Some(&[0x19, 0xC9][..])
        );
        assert_eq!(record.item(Item::TimeOfDay), Some(&[0x35, 0x6D, 0x4D][..]));
        assert_eq!(
            record.item(Item::TargetReportDescriptor),
            Some(&[0xA1, 0x40][..])
        );
        assert_eq!(
            record.item(Item::MeasuredPositionPolar),
            Some(&[0xC5, 0xAF, 0xF1, 0xE0][..])
        );
        assert_eq!(record.item(Item::Mode3ACode), None);
    }

    #[test]
    fn multi_octet_fspec() {
        // Octet 1: FRN 1 + FX. Octet 2: 0x10 = FRN 11 (I048/161 track number).
        let dg = datagram(&[0x81, 0x10, 0x01, 0x02, 0x0F, 0xFF]);
        let record = single(&dg);

        let present: Vec<_> = record.items().map(|(item, _)| item).collect();
        assert_eq!(present, [Item::DataSourceIdentifier, Item::TrackNumber]);
        assert_eq!(record.item(Item::TrackNumber), Some(&[0x0F, 0xFF][..]));
    }

    #[test]
    fn repetitive_item() {
        // FRN 10 (I048/250) is bit 0x20 of FSPEC octet 2. REP = 2, 8 bytes each.
        let mut bytes = vec![0x01, 0x20, 2];
        bytes.extend([0xAB; 16]);
        let dg = datagram(&bytes);

        assert_eq!(single(&dg).item(Item::BdsRegisterData).unwrap().len(), 17);
    }

    #[test]
    fn compound_radar_plot_characteristics() {
        // FRN 7 (I048/130). Primary 0x82: subfields 1 (SRL) and 7 (APD).
        let dg = datagram(&[0x02, 0x82, 0x11, 0x22]);

        assert_eq!(
            single(&dg).item(Item::RadarPlotCharacteristics),
            Some(&[0x82, 0x11, 0x22][..])
        );
    }

    #[test]
    fn compound_radial_doppler_speed() {
        // FRN 20 (I048/120) is bit 0x04 of FSPEC octet 3.
        // Primary 0xC0: CAL (2 bytes) and RDS (REP = 1, 6 bytes).
        let dg = datagram(&[
            0x01, 0x01, 0x04, // FSPEC
            0xC0, 0x00, 0x10, 0x01, 1, 2, 3, 4, 5, 6,
        ]);

        assert_eq!(
            single(&dg).item(Item::RadialDopplerSpeed).unwrap().len(),
            10
        );
    }

    #[test]
    fn explicit_special_purpose_field() {
        // FRN 27 (SP) is bit 0x04 of FSPEC octet 4. LEN = 4 includes itself.
        let dg = datagram(&[0x01, 0x01, 0x01, 0x04, 0x04, 0xAA, 0xBB, 0xCC]);

        assert_eq!(
            single(&dg).item(Item::SpecialPurpose),
            Some(&[0x04, 0xAA, 0xBB, 0xCC][..])
        );
    }

    #[test]
    fn two_records_in_one_block() {
        let dg = datagram(&[
            0x80, 0x01, 0x02, // record 1: I048/010
            0x88, 0x03, 0x04, 0x05, 0x06, // record 2: I048/010 + /070
        ]);
        let records: Vec<_> = parse(&dg).into_iter().map(Result::unwrap).collect();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].offset(), 3);
        assert_eq!(records[1].offset(), 6);
        assert_eq!(records[1].item(Item::Mode3ACode), Some(&[0x05, 0x06][..]));
    }

    #[test]
    fn wrong_category() {
        let dg = [34, 0x00, 0x04, 0x80];
        let block = data_blocks(&dg).next().unwrap().unwrap();

        assert_eq!(
            records(&block).unwrap_err(),
            RecordError::WrongCategory {
                offset: 0,
                category: 34
            }
        );
    }

    #[test]
    fn truncated_fspec() {
        assert_eq!(
            parse(&datagram(&[0x81])),
            vec![Err(RecordError::TruncatedFspec { offset: 3 })]
        );
    }

    #[test]
    fn fspec_too_long() {
        // FX set in octet 4: there is no FRN 29.
        assert_eq!(
            parse(&datagram(&[0x01, 0x01, 0x01, 0x01, 0x80])),
            vec![Err(RecordError::FspecTooLong { offset: 3 })]
        );
    }

    #[test]
    fn empty_record() {
        assert_eq!(
            parse(&datagram(&[0x01, 0x00])),
            vec![Err(RecordError::EmptyRecord { offset: 3 })]
        );
    }

    #[test]
    fn truncated_fixed_item() {
        assert_eq!(
            parse(&datagram(&[0x80, 0x19])),
            vec![Err(RecordError::TruncatedItem {
                offset: 4,
                item: Item::DataSourceIdentifier,
                needed: 2,
                available: 1
            })]
        );
    }

    #[test]
    fn truncated_extended_item() {
        // I048/020 with FX set and no extension octet.
        assert_eq!(
            parse(&datagram(&[0x20, 0xA1])),
            vec![Err(RecordError::TruncatedItem {
                offset: 4,
                item: Item::TargetReportDescriptor,
                needed: 2,
                available: 1
            })]
        );
    }

    #[test]
    fn truncated_repetitive_item() {
        // REP = 2 needs 17 bytes; only 9 present.
        let mut bytes = vec![0x01, 0x20, 2];
        bytes.extend([0; 8]);
        assert_eq!(
            parse(&datagram(&bytes)),
            vec![Err(RecordError::TruncatedItem {
                offset: 5,
                item: Item::BdsRegisterData,
                needed: 17,
                available: 9
            })]
        );
    }

    #[test]
    fn undefined_compound_subfield() {
        // I048/120 primary 0x20 flags subfield 3, which CAT048 does not define.
        assert_eq!(
            parse(&datagram(&[0x01, 0x01, 0x04, 0x20])),
            vec![Err(RecordError::UndefinedSubfield {
                offset: 6,
                item: Item::RadialDopplerSpeed,
                subfield: 3
            })]
        );
    }

    #[test]
    fn zero_length_special_purpose_field() {
        assert_eq!(
            parse(&datagram(&[0x01, 0x01, 0x01, 0x04, 0x00])),
            vec![Err(RecordError::ZeroExplicitLength {
                offset: 7,
                item: Item::SpecialPurpose
            })]
        );
    }

    #[test]
    fn error_in_second_record_reports_its_offset() {
        let dg = datagram(&[0x80, 0x01, 0x02, 0x80, 0x03]);
        let records = parse(&dg);

        assert!(records[0].is_ok());
        assert_eq!(
            records[1],
            Err(RecordError::TruncatedItem {
                offset: 7,
                item: Item::DataSourceIdentifier,
                needed: 2,
                available: 1
            })
        );
    }

    #[test]
    fn stops_after_first_error() {
        // Bad record (empty FSPEC), then bytes that would parse as a valid record.
        let dg = datagram(&[0x00, 0x80, 0x01, 0x02]);
        let block = data_blocks(&dg).next().unwrap().unwrap();
        let mut iter = records(&block).unwrap();

        assert!(matches!(
            iter.next(),
            Some(Err(RecordError::EmptyRecord { .. }))
        ));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn error_message_names_the_item() {
        let err = RecordError::TruncatedItem {
            offset: 20,
            item: Item::TrackNumber,
            needed: 2,
            available: 1,
        };
        assert_eq!(
            err.to_string(),
            "truncated item I048/161 (FRN 11) at offset 20: needs 2 bytes, 1 available"
        );
    }
}
