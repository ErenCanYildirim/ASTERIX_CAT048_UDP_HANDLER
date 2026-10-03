// ASTERIX item encodings and length rules

// least significant bit of an extended octet
pub(crate) const FX: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Always exactly this many bytes.
    Fixed(usize),
    /// One or more 1-byte octets; each octet's FX bit says whether another follows.
    Extended,
    /// A 1-byte repetition count `REP`, then `REP` elements of this many bytes each.
    Repetitive(usize),
    /// A 1-byte length (counting itself), then that many bytes minus one.
    Explicit,
    /// An extended primary subfield whose bits say which subfields follow, in order.
    /// Entry `i` is the format of subfield `i + 1`; missing entries are undefined.
    Compound(&'static [Format]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LenError {
    /// The item needs at least `needed` bytes from its start, and fewer were available.
    Truncated { needed: usize },
    /// An explicit-length item declared length 0, which cannot include its own length byte.
    ZeroExplicitLength,
    /// A compound primary subfield flagged a subfield (1-based) that the format does not define.
    UndefinedSubfield { subfield: usize },
}

impl LenError {
    /// Re-express a nested error relative to an outer item that starts `by` bytes earlier.
    fn shifted(self, by: usize) -> Self {
        match self {
            Self::Truncated { needed } => Self::Truncated {
                needed: needed + by,
            },
            other => other,
        }
    }
}

/// Number of bytes the item at the start of `bytes` occupies, according to `format`
pub(crate) fn item_len(format: Format, bytes: &[u8]) -> Result<usize, LenError> {
    let len = match format {
        Format::Fixed(len) => len,
        Format::Extended => {
            return bytes
                .iter()
                .position(|octet| octet & FX == 0)
                .map(|last| last + 1)
                .ok_or(LenError::Truncated {
                    needed: bytes.len() + 1,
                });
        }
        Format::Repetitive(element_len) => {
            let Some(&rep) = bytes.first() else {
                return Err(LenError::Truncated { needed: 1 });
            };
            1 + usize::from(rep) * element_len
        }
        Format::Explicit => match bytes.first() {
            None => return Err(LenError::Truncated { needed: 1 }),
            Some(0) => return Err(LenError::ZeroExplicitLength),
            Some(&len) => usize::from(len),
        },
        Format::Compound(subfields) => return compound_len(subfields, bytes),
    };

    if bytes.len() < len {
        return Err(LenError::Truncated { needed: len });
    }
    Ok(len)
}

fn compound_len(subfields: &[Format], bytes: &[u8]) -> Result<usize, LenError> {
    let primary_len = item_len(Format::Extended, bytes)?;
    let primary = bytes.get(..primary_len).unwrap_or_default();
    let mut pos = primary_len;

    for (octet_index, &octet) in primary.iter().enumerate() {
        for bit in 0..7 {
            if octet & (0x80 >> bit) == 0 {
                continue;
            }
            let index = octet_index * 7 + bit;
            let Some(&subfield) = subfields.get(index) else {
                return Err(LenError::UndefinedSubfield {
                    subfield: index + 1,
                });
            };
            let rest = bytes.get(pos..).unwrap_or_default();
            let len = item_len(subfield, rest).map_err(|err| err.shifted(pos))?;
            pos += len;
        }
    }
    Ok(pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed() {
        assert_eq!(item_len(Format::Fixed(2), &[1, 2, 3]), Ok(2));
        assert_eq!(
            item_len(Format::Fixed(2), &[1]),
            Err(LenError::Truncated { needed: 2 })
        );
    }

    #[test]
    fn extended_stops_at_first_clear_fx() {
        assert_eq!(item_len(Format::Extended, &[0xA0, 0xFF]), Ok(1));
        assert_eq!(item_len(Format::Extended, &[0xA1, 0x41, 0x40, 0xFF]), Ok(3));
        assert_eq!(
            item_len(Format::Extended, &[0xA1, 0x41]),
            Err(LenError::Truncated { needed: 3 })
        );
        assert_eq!(
            item_len(Format::Extended, &[]),
            Err(LenError::Truncated { needed: 1 })
        );
    }

    #[test]
    fn repetitive() {
        assert_eq!(
            item_len(Format::Repetitive(3), &[2, 0, 0, 0, 0, 0, 0]),
            Ok(7)
        );
        assert_eq!(item_len(Format::Repetitive(3), &[0, 9]), Ok(1));
        assert_eq!(
            item_len(Format::Repetitive(3), &[2, 0, 0, 0]),
            Err(LenError::Truncated { needed: 7 })
        );
    }

    #[test]
    fn explicit() {
        assert_eq!(item_len(Format::Explicit, &[3, 0xAA, 0xBB, 0xCC]), Ok(3));
        assert_eq!(item_len(Format::Explicit, &[1]), Ok(1));
        assert_eq!(
            item_len(Format::Explicit, &[0]),
            Err(LenError::ZeroExplicitLength)
        );
        assert_eq!(
            item_len(Format::Explicit, &[5, 1]),
            Err(LenError::Truncated { needed: 5 })
        );
    }

    const COMPOUND: Format = Format::Compound(&[Format::Fixed(2), Format::Repetitive(1)]);

    #[test]
    fn compound_reads_flagged_subfields_in_order() {
        // Primary 0xC0: subfields 1 and 2 present.
        assert_eq!(item_len(COMPOUND, &[0xC0, 1, 2, 2, 7, 8]), Ok(6));
        // Primary 0x40: only subfield 2.
        assert_eq!(item_len(COMPOUND, &[0x40, 1, 7]), Ok(3));
    }

    #[test]
    fn compound_rejects_undefined_subfield() {
        assert_eq!(
            item_len(COMPOUND, &[0x20, 0, 0]),
            Err(LenError::UndefinedSubfield { subfield: 3 })
        );
    }

    #[test]
    fn compound_truncation_is_relative_to_item_start() {
        // Subfield 2 starts at byte 3 and declares 4 elements: needs 3 + 1 + 4 bytes.
        assert_eq!(
            item_len(COMPOUND, &[0xC0, 1, 2, 4, 7]),
            Err(LenError::Truncated { needed: 8 })
        );
    }
}
