//! SANS-IO ASTERIX CAT048 decoding and encoding
//!
//! Turn bytes into typed records and back, never touches a socket, a clock or an async runtime
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::unreachable,
        clippy::todo,
        clippy::unimplemented
    )
)]

pub mod cat048;
pub mod error;
pub mod format;
pub mod framing;
pub mod units;

pub use error::{DecodeError, EncodeError, FramingError, RecordError};
pub use framing::{DataBlock, DataBlocks, data_blocks};
