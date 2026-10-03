//! Splitting a datagram into ASTERIX data blocks.

//! A data block is `CAT (1 byte) | LEN (2 bytes, big-endian) | records...`,
//! where LEN counts the whole block including the 3 header bytes. One datagram
//! may carry several blocks back to back, possibly of different categories.

use core::iter::FusedIterator;

use crate::error::FramingError;

/// Size of the CAT + LEN header at the start of every data block.
pub const HEADER_LEN: usize = 3;

/// One data block, borrowed from the datagram it was parsed from
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataBlock<'a> {
    category: u8,
    offset: usize,
    records: &'a [u8],
}

impl<'a> DataBlock<'a> {
    // Asterix category, 48 for monoradar target reports
    #[must_use]
    pub fn category(&self) -> u8 {
        self.category
    }

    /// Byte offset of the block's header with the datagram
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The record bytes after the header. Never empty.
    #[must_use]
    pub fn records(&self) -> &'a [u8] {
        self.records
    }
}

// Iterate over data blocks in one datagram
// Yields Ok(block) for each well-formed block, on the first malformed block
// yields one `Err` then stops
#[must_use]
pub fn data_blocks(datagram: &[u8]) -> DataBlocks<'_> {
    DataBlocks {
        remaining: datagram,
        offset: 0,
        state: State::Start,
    }
}

/// Iterator returned by [`data_blocks`].
#[derive(Debug, Clone)]
pub struct DataBlocks<'a> {
    remaining: &'a [u8],
    offset: usize,
    state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Start,
    Running,
    Done,
}

impl<'a> DataBlocks<'a> {
    fn parse_block(&mut self) -> Result<DataBlock<'a>, FramingError> {
        let offset = self.offset;
        let available = self.remaining.len();

        let Some((&[category, len_hi, len_lo], after_header)) =
            self.remaining.split_first_chunk::<HEADER_LEN>()
        else {
            return Err(FramingError::TruncatedHeader { offset, available });
        };

        let len = usize::from(u16::from_be_bytes([len_hi, len_lo]));
        if len < HEADER_LEN {
            return Err(FramingError::LengthTooSmall { offset, len });
        }
        if len == HEADER_LEN {
            return Err(FramingError::EmptyBlock { offset, category });
        }

        let Some((records, rest)) = after_header.split_at_checked(len - HEADER_LEN) else {
            return Err(FramingError::TruncatedBlock {
                offset,
                declared: len,
                available,
            });
        };

        self.remaining = rest;
        self.offset += len;
        Ok(DataBlock {
            category,
            offset,
            records,
        })
    }
}

impl<'a> Iterator for DataBlocks<'a> {
    type Item = Result<DataBlock<'a>, FramingError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.state {
            State::Done => return None,
            State::Start if self.remaining.is_empty() => {
                self.state = State::Done;
                return Some(Err(FramingError::EmptyDatagram));
            }
            State::Running if self.remaining.is_empty() => {
                self.state = State::Done;
                return None;
            }
            State::Start | State::Running => {}
        }

        self.state = State::Running;
        let result = self.parse_block();
        if result.is_err() {
            self.state = State::Done;
        }
        Some(result)
    }
}

impl FusedIterator for DataBlocks<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(datagram: &[u8]) -> Vec<Result<DataBlock<'_>, FramingError>> {
        data_blocks(datagram).collect()
    }

    #[test]
    fn single_block() {
        let datagram = [48, 0x00, 0x06, 0xAA, 0xBB, 0xCC];
        let blocks = collect(&datagram);

        assert_eq!(blocks.len(), 1);
        let block = blocks[0].unwrap();
        assert_eq!(block.category(), 48);
        assert_eq!(block.offset(), 0);
        assert_eq!(block.records(), &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn two_blocks_mixed_categories() {
        let datagram = [
            34, 0x00, 0x04, 0x01, // CAT034, 1 record byte
            48, 0x00, 0x05, 0x02, 0x03, // CAT048, 2 record bytes
        ];
        let blocks: Vec<_> = data_blocks(&datagram).map(Result::unwrap).collect();

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].category(), 34);
        assert_eq!(blocks[0].offset(), 0);
        assert_eq!(blocks[0].records(), &[0x01]);
        assert_eq!(blocks[1].category(), 48);
        assert_eq!(blocks[1].offset(), 4);
        assert_eq!(blocks[1].records(), &[0x02, 0x03]);
    }

    #[test]
    fn length_uses_both_bytes_big_endian() {
        // LEN = 0x0103 = 259: a byte-order bug would read 0x0301 = 769 and fail.
        let mut datagram = vec![48, 0x01, 0x03];
        datagram.extend(std::iter::repeat_n(0xEE, 256));
        let blocks = collect(&datagram);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].unwrap().records().len(), 256);
    }

    #[test]
    fn empty_datagram() {
        assert_eq!(collect(&[]), vec![Err(FramingError::EmptyDatagram)]);
    }

    #[test]
    fn truncated_header() {
        assert_eq!(
            collect(&[48, 0x00]),
            vec![Err(FramingError::TruncatedHeader {
                offset: 0,
                available: 2
            })]
        );
    }

    #[test]
    fn trailing_bytes_after_valid_block() {
        let datagram = [48, 0x00, 0x04, 0x01, 0xFF];
        let blocks = collect(&datagram);

        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].is_ok());
        assert_eq!(
            blocks[1],
            Err(FramingError::TruncatedHeader {
                offset: 4,
                available: 1
            })
        );
    }

    #[test]
    fn length_too_small() {
        assert_eq!(
            collect(&[48, 0x00, 0x02, 0x01]),
            vec![Err(FramingError::LengthTooSmall { offset: 0, len: 2 })]
        );
    }

    #[test]
    fn length_zero() {
        assert_eq!(
            collect(&[48, 0x00, 0x00]),
            vec![Err(FramingError::LengthTooSmall { offset: 0, len: 0 })]
        );
    }

    #[test]
    fn empty_block() {
        assert_eq!(
            collect(&[48, 0x00, 0x03]),
            vec![Err(FramingError::EmptyBlock {
                offset: 0,
                category: 48
            })]
        );
    }

    #[test]
    fn length_exceeds_datagram() {
        assert_eq!(
            collect(&[48, 0x00, 0x0A, 0x01, 0x02, 0x03]),
            vec![Err(FramingError::TruncatedBlock {
                offset: 0,
                declared: 10,
                available: 6
            })]
        );
    }

    #[test]
    fn error_in_second_block_reports_its_offset() {
        let datagram = [48, 0x00, 0x04, 0x01, 48, 0x00, 0x09, 0x02];
        let blocks = collect(&datagram);

        assert!(blocks[0].is_ok());
        assert_eq!(
            blocks[1],
            Err(FramingError::TruncatedBlock {
                offset: 4,
                declared: 9,
                available: 4
            })
        );
    }

    #[test]
    fn stops_after_first_error() {
        // A bad LEN, then bytes that would parse as a valid block if we kept going.
        let datagram = [48, 0x00, 0x02, 48, 0x00, 0x04, 0x01];
        let mut iter = data_blocks(&datagram);

        assert!(matches!(
            iter.next(),
            Some(Err(FramingError::LengthTooSmall { .. }))
        ));
        assert_eq!(iter.next(), None);
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn records_borrow_from_input() {
        let datagram = [48, 0x00, 0x04, 0x7F];
        let block = data_blocks(&datagram).next().unwrap().unwrap();

        // Zero-copy: the record slice points into the original buffer.
        assert_eq!(block.records().as_ptr(), datagram[3..].as_ptr());
    }

    #[test]
    fn error_message_is_locatable() {
        let err = FramingError::TruncatedBlock {
            offset: 12,
            declared: 40,
            available: 9,
        };
        assert_eq!(
            err.to_string(),
            "truncated data block at offset 12: LEN declares 40 bytes, 9 available"
        );
    }
}
