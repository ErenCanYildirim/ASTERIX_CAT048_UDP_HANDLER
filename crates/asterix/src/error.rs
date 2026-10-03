// Error types for the asterix crate

use crate::cat048::Item;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FramingError {
    // Datagram contained no bytes
    #[error("empty datagram: expected at least one data block")]
    EmptyDatagram,

    // Fewer than 3 bytes remained where a block header (CAT + LEN) should start
    #[error("truncated block header at offset {offset}: need 3 bytes, {available} available")]
    TruncatedHeader { offset: usize, available: usize },

    /// LEN was below 3, which is impossible because LEN includes the header itself.
    #[error("invalid block length {len} at offset {offset}: LEN must be at least 3")]
    LengthTooSmall { offset: usize, len: usize },

    /// LEN was exactly 3: a header with no records. The spec requires at least one record.
    #[error("empty data block (CAT{category:03}) at offset {offset}: contains no records")]
    EmptyBlock { offset: usize, category: u8 },

    /// LEN claimed more bytes than the datagram has left
    #[error(
        "truncated data block at offset {offset}: LEN declares {declared} bytes, {available} available"
    )]
    TruncatedBlock {
        offset: usize,
        declared: usize,
        available: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RecordError {
    /// The data block is not CAT048.
    #[error("data block at offset {offset} is CAT{category:03}, expected CAT048")]
    WrongCategory { offset: usize, category: u8 },

    /// The FSPEC's last octet had FX set but the block ended.
    #[error("truncated FSPEC at offset {offset}: FX set but no more bytes")]
    TruncatedFspec { offset: usize },

    /// The FSPEC extends past the last CAT048 FRN.
    #[error("FSPEC at offset {offset} extends beyond the CAT048 UAP")]
    FspecTooLong { offset: usize },

    /// The FSPEC flags no items at all.
    #[error("empty record at offset {offset}: FSPEC flags no items")]
    EmptyRecord { offset: usize },

    /// An item needs more bytes than remain in the block.
    #[error(
        "truncated item {item} at offset {offset}: needs {needed} bytes, {available} available"
    )]
    TruncatedItem {
        offset: usize,
        item: Item,
        needed: usize,
        available: usize,
    },

    /// An explicit-length item declared length 0.
    #[error("item {item} at offset {offset} declares explicit length 0")]
    ZeroExplicitLength { offset: usize, item: Item },

    /// A compound item flags a subfield the UAP does not define.
    #[error("item {item} at offset {offset} flags undefined subfield {subfield}")]
    UndefinedSubfield {
        offset: usize,
        item: Item,
        subfield: usize,
    },
}

/// Why a present item could not be decoded into a typed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DecodeError {
    /// The item's byte length does not match its fixed size.
    #[error("item {item} is {actual} bytes, expected {expected}")]
    WrongLength {
        item: Item,
        expected: usize,
        actual: usize,
    },

    /// I048/140 was one day or more: not a valid time of day.
    #[error("time of day {ticks}/128 s is not below 24 h")]
    TimeOutOfRange { ticks: u32 },
}
