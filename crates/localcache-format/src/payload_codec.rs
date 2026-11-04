//! The codec for the legacy payload wire format (RFC 028).
//!
//! Every payload written with [`Codec::Bincode`](crate::Codec::Bincode) is in
//! the format `bincode` 1 defined and RFC 008 froze, which `bincode` 2 writes
//! with `config::legacy()`. This module reads and writes exactly those bytes. It
//! replaces the `bincode` crate, whose decoder reserved memory for a *claimed*
//! length before checking that the bytes exist (a forged length prefix aborted
//! the process) and applied no limit to nesting.
//!
//! # The format (RFC 028 R1)
//!
//! Little-endian, fixed-width. The serde data model maps to bytes as follows.
//!
//! | Serde type | Bytes |
//! |---|---|
//! | `bool` | one byte, `0` or `1` |
//! | `u8`/`i8` … `u64`/`i64`, `u128`/`i128` | 1, 2, 4, 8 or 16 bytes |
//! | `usize`/`isize` | as `u64`/`i64` (serde presents them so) |
//! | `f32`/`f64` | the IEEE-754 bits, 4 or 8 bytes |
//! | `char` | UTF-8, one to four bytes |
//! | `str`, `String`, bytes | `u64` length, then that many bytes |
//! | `Option<T>` | one byte `0` or `1`, then the value if `1` |
//! | unit, unit struct | no bytes |
//! | newtype struct | the inner value |
//! | sequence, map | `u64` count, then the elements (a map's entries as key, value) |
//! | tuple, tuple struct, struct | the fields in order, no prefix |
//! | enum | `u32` variant index, then the variant's fields |
//!
//! The format is not self-describing, so `deserialize_any` (and with it
//! `flatten`, `untagged`, internally and adjacently tagged enums and
//! `serde_json::Value`), `deserialize_identifier` and `deserialize_ignored_any`
//! are errors, as is serializing a sequence or map whose length is unknown.
//!
//! # What the decoder does with hostile bytes (RFC 028 R2)
//!
//! A stored payload is untrusted input: anyone who can write the database file
//! can write any bytes.
//!
//! * Every length is checked against the bytes that remain **before** anything
//!   is allocated or looped over. A string or byte-buffer length that exceeds
//!   them, or a sequence or map count that exceeds them, is an error; and the
//!   counts of all the sequences and maps in one decode together may not exceed
//!   the input's length, which bounds the work for zero-sized elements
//!   (`Vec<Vec<()>>`) to the input's length instead of its square.
//! * No allocation is made from a claimed length. A string or byte buffer is
//!   allocated at its verified length. A sequence or map reports its verified
//!   count as its size hint, as `bincode`'s decoder did, and serde's collections
//!   reserve room for at most that many elements and never more than 1 MiB
//!   (`Vec::with_capacity(min(hint, 1 MiB / size))`) per open nesting level, so
//!   at most 128 MiB of address space however large the claim; they then grow as
//!   elements are decoded. This code never allocates from a count itself, and it
//!   never allocates more than `bincode` did on the same input.
//! * Nesting is limited to [`DEPTH_LIMIT`] levels, so a payload cannot overflow
//!   the stack however deeply it claims to nest. The encoder counts the same
//!   levels and refuses the same depth, so a value that can be written can be
//!   read back (an engine never stores what it would refuse to read).
//! * A bad `bool` or `Option` tag, an invalid `char`, invalid UTF-8, a variant
//!   index the type does not have and a truncated value are errors.
//! * Bytes after a complete value are ignored, as they always were.
//!
//! The one place the count check refuses something `bincode` accepted is a
//! sequence or map of zero-byte elements (`Vec<()>`): its count cannot be told
//! from a forged one, so a count above the bytes left is refused (RFC 028 R5).
//!
//! There is no `unsafe` code here, and none can be added without removing the
//! `forbid` below.

#![forbid(unsafe_code)]

mod decoder;
mod encoder;
#[cfg(test)]
mod tests;

use serde::{Serialize, de::DeserializeOwned};

/// How many levels of nested values the codec accepts (RFC 028 R2.2).
///
/// A level is one `Option` that holds a value, newtype, sequence, tuple, map,
/// struct or enum being decoded (or encoded) inside another; an enum variant's
/// fields add no level. It is the limit `serde_json` applies to documents, and it
/// bounds the decoder's stack use to tens of kilobytes (measured: at most 30 KiB
/// in a release build and 146 KiB in a debug build, at the limit) whatever the
/// input claims. The encoder counts the same levels (Amendment 1, F1).
pub(super) const DEPTH_LIMIT: usize = 128;

/// The longest message kept from a `serde` error, in bytes. Messages built by
/// other crates' `Deserialize` implementations can quote the input; this keeps
/// an error from carrying a large piece of it.
const MESSAGE_LIMIT: usize = 200;

/// Why a payload could not be written or read.
#[derive(Debug)]
pub(super) enum Error {
    /// A message from a `Serialize` or `Deserialize` implementation.
    Message(String),
    /// The input ended inside a value, or a length claims more than remains.
    UnexpectedEnd { needed: usize, remaining: usize },
    /// A sequence or map claims more elements than there are bytes left, or the
    /// counts claimed in this decode together exceed the input's length.
    CountExceedsInput { claimed: u64, remaining: usize },
    /// A length that does not fit this platform's `usize`.
    LengthOverflow(u64),
    /// A `bool` byte that is neither `0` nor `1`.
    InvalidBool(u8),
    /// An `Option` tag that is neither `0` nor `1`.
    InvalidOptionTag(u8),
    /// Bytes that are not one UTF-8 encoded `char`.
    InvalidChar,
    /// A string whose bytes are not UTF-8.
    InvalidUtf8,
    /// More than [`DEPTH_LIMIT`] nested values.
    TooDeep,
    /// A call that needs a self-describing format.
    Unsupported(&'static str),
    /// A sequence or map whose length is not known when it starts.
    UnknownLength,
    /// A sequence or map that wrote a different number of elements than declared.
    LengthMismatch { declared: usize, written: usize },
    /// A length that does not fit in 64 bits.
    LengthTooLarge,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Message(message) => f.write_str(message),
            Self::UnexpectedEnd { needed, remaining } => write!(
                f,
                "unexpected end of input: {needed} byte(s) needed, {remaining} left"
            ),
            Self::CountExceedsInput { claimed, remaining } => write!(
                f,
                "a count of {claimed} elements exceeds the {remaining} byte(s) left"
            ),
            Self::LengthOverflow(length) => write!(f, "a length of {length} does not fit in usize"),
            Self::InvalidBool(value) => write!(f, "invalid bool value {value}"),
            Self::InvalidOptionTag(value) => write!(f, "invalid Option tag {value}"),
            Self::InvalidChar => f.write_str("invalid char encoding"),
            Self::InvalidUtf8 => f.write_str("invalid UTF-8 in a string"),
            Self::TooDeep => write!(f, "nesting is deeper than {DEPTH_LIMIT} levels"),
            Self::Unsupported(what) => write!(
                f,
                "{what} is not supported: the payload format is not self-describing"
            ),
            Self::UnknownLength => f.write_str("a sequence or map must have a known length"),
            Self::LengthMismatch { declared, written } => write!(
                f,
                "a sequence or map declared {declared} element(s) but wrote {written}"
            ),
            Self::LengthTooLarge => f.write_str("a length does not fit in 64 bits"),
        }
    }
}

impl std::error::Error for Error {}

/// Keep at most [`MESSAGE_LIMIT`] bytes of `text`, on a character boundary.
fn bounded(mut text: String) -> String {
    if text.len() > MESSAGE_LIMIT {
        let mut end = MESSAGE_LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

impl serde::ser::Error for Error {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self::Message(bounded(message.to_string()))
    }
}

impl serde::de::Error for Error {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self::Message(bounded(message.to_string()))
    }
}

/// Encode `value` in the legacy payload format.
pub(super) fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    let mut encoder = encoder::Encoder::new();
    value.serialize(&mut encoder)?;
    Ok(encoder.into_bytes())
}

/// Decode a value from the start of `bytes`; bytes after it are ignored.
pub(super) fn from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    T::deserialize(&mut decoder::Decoder::new(bytes))
}
