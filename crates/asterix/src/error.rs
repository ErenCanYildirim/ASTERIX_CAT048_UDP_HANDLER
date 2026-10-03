// Error types for the asterix crate

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
