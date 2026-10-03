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
