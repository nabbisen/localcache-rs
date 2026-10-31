//! The decoder: a `serde::Deserializer` over a byte slice, for hostile input.
//!
//! Three rules shape every function here (RFC 028 R2):
//!
//! 1. bytes are only ever taken from the slice that exists, so a length that
//!    claims more than remains fails in [`Decoder::take`] before any allocation;
//! 2. a sequence or map count is checked against the bytes left before the first
//!    element is decoded (`Decoder::count`), and is the only number from the input
//!    that decides how many times anything loops;
//! 3. every call that can decode another value goes through `Decoder::nested`,
//!    which counts the depth.

use serde::de::{
    self, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};

use super::{DEPTH_LIMIT, Error};

/// A cursor over the payload and the depth reached so far.
pub(super) struct Decoder<'de> {
    input: &'de [u8],
    depth: usize,
    /// How many more sequence and map elements this decode may claim, in all.
    budget: usize,
}

impl<'de> Decoder<'de> {
    pub(super) fn new(input: &'de [u8]) -> Self {
        Self {
            input,
            depth: 0,
            budget: input.len(),
        }
    }

    /// The next `n` bytes, or an error if fewer remain. Nothing is allocated.
    fn take(&mut self, n: usize) -> Result<&'de [u8], Error> {
        let (head, tail) = self.input.split_at_checked(n).ok_or(Error::UnexpectedEnd {
            needed: n,
            remaining: self.input.len(),
        })?;
        self.input = tail;
        Ok(head)
    }

    /// The next `N` bytes as an array.
    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let (head, tail) = self
            .input
            .split_first_chunk::<N>()
            .ok_or(Error::UnexpectedEnd {
                needed: N,
                remaining: self.input.len(),
            })?;
        self.input = tail;
        Ok(*head)
    }

    /// A `u64` length prefix as a `usize`.
    fn length(&mut self) -> Result<usize, Error> {
        let raw = u64::from_le_bytes(self.array()?);
        usize::try_from(raw).map_err(|_| Error::LengthOverflow(raw))
    }

    /// The element count of a sequence or map.
    ///
    /// It cannot exceed the bytes left: an element that is not zero-sized takes
    /// at least one byte, and a zero-sized one cannot be told from a forged
    /// count (RFC 028 R5). And the counts of **all** the sequences and maps in
    /// one decode together cannot exceed the length of the input (Amendment 1,
    /// F2): that bounds the work for zero-sized elements, which have no bytes
    /// to be counted against, to the input's length instead of its square.
    /// Honest payloads satisfy both: every element of a type that occupies
    /// bytes takes at least one, and a container's header is itself bytes.
    fn count(&mut self) -> Result<usize, Error> {
        let raw = u64::from_le_bytes(self.array()?);
        let count = usize::try_from(raw).map_err(|_| Error::LengthOverflow(raw))?;
        if count > self.input.len() {
            return Err(Error::CountExceedsInput {
                claimed: raw,
                remaining: self.input.len(),
            });
        }
        self.budget = self
            .budget
            .checked_sub(count)
            .ok_or(Error::CountExceedsInput {
                claimed: raw,
                remaining: self.budget,
            })?;
        Ok(count)
    }

    /// A length-prefixed string, borrowed from the input.
    fn str(&mut self) -> Result<&'de str, Error> {
        let length = self.length()?;
        std::str::from_utf8(self.take(length)?).map_err(|_| Error::InvalidUtf8)
    }

    /// The fields of a tuple, struct or enum variant: exactly `len` values.
    ///
    /// The caller has already counted the level, so a variant's fields cost no
    /// level beyond the enum itself.
    fn fields<V: Visitor<'de>>(&mut self, len: usize, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_seq(Elements {
            decoder: self,
            remaining: len,
        })
    }

    /// Run `decode` one level deeper, or fail if that exceeds [`DEPTH_LIMIT`].
    fn nested<T>(
        &mut self,
        decode: impl FnOnce(&mut Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if self.depth >= DEPTH_LIMIT {
            return Err(Error::TooDeep);
        }
        self.depth += 1;
        let decoded = decode(self);
        self.depth -= 1;
        decoded
    }
}

/// How many bytes the UTF-8 sequence starting with `first` has, or 0 if `first`
/// cannot start one (a continuation byte, an overlong lead or above `U+10FFFF`).
fn utf8_width(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 0,
    }
}

/// Decode a fixed-width little-endian scalar.
macro_rules! scalar {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            visitor.$visit(<$ty>::from_le_bytes(self.array()?))
        }
    };
}

impl<'de> de::Deserializer<'de> for &mut Decoder<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::Unsupported("deserialize_any"))
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.array::<1>()?[0] {
            0 => visitor.visit_bool(false),
            1 => visitor.visit_bool(true),
            other => Err(Error::InvalidBool(other)),
        }
    }

    scalar!(deserialize_i8, visit_i8, i8);
    scalar!(deserialize_i16, visit_i16, i16);
    scalar!(deserialize_i32, visit_i32, i32);
    scalar!(deserialize_i64, visit_i64, i64);
    scalar!(deserialize_i128, visit_i128, i128);
    scalar!(deserialize_u8, visit_u8, u8);
    scalar!(deserialize_u16, visit_u16, u16);
    scalar!(deserialize_u32, visit_u32, u32);
    scalar!(deserialize_u64, visit_u64, u64);
    scalar!(deserialize_u128, visit_u128, u128);
    scalar!(deserialize_f32, visit_f32, f32);
    scalar!(deserialize_f64, visit_f64, f64);

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let [first] = self.array::<1>()?;
        let width = utf8_width(first);
        if width == 0 {
            return Err(Error::InvalidChar);
        }
        let mut buffer = [first, 0, 0, 0];
        if let Some(rest) = buffer.get_mut(1..width) {
            rest.copy_from_slice(self.take(width - 1)?);
        }
        let decoded = buffer
            .get(..width)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|text| text.chars().next())
            .ok_or(Error::InvalidChar)?;
        visitor.visit_char(decoded)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_borrowed_str(self.str()?)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_string(self.str()?.to_owned())
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let length = self.length()?;
        visitor.visit_borrowed_bytes(self.take(length)?)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let length = self.length()?;
        visitor.visit_byte_buf(self.take(length)?.to_vec())
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.array::<1>()?[0] {
            0 => visitor.visit_none(),
            1 => self.nested(|decoder| visitor.visit_some(decoder)),
            other => Err(Error::InvalidOptionTag(other)),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.nested(|decoder| visitor.visit_newtype_struct(decoder))
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let count = self.count()?;
        self.nested(|decoder| {
            visitor.visit_seq(Elements {
                decoder,
                remaining: count,
            })
        })
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        self.nested(|decoder| decoder.fields(len, visitor))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let count = self.count()?;
        self.nested(|decoder| {
            visitor.visit_map(Entries {
                decoder,
                remaining: count,
            })
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(fields.len(), visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.nested(|decoder| visitor.visit_enum(Variant { decoder }))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::Unsupported("deserialize_identifier"))
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::Unsupported("deserialize_ignored_any"))
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

/// The elements of a sequence, tuple or struct: exactly `remaining` of them.
struct Elements<'a, 'de> {
    decoder: &'a mut Decoder<'de>,
    remaining: usize,
}

impl<'de> SeqAccess<'de> for Elements<'_, 'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        seed.deserialize(&mut *self.decoder).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

/// The entries of a map: exactly `remaining` key and value pairs.
struct Entries<'a, 'de> {
    decoder: &'a mut Decoder<'de>,
    remaining: usize,
}

impl<'de> MapAccess<'de> for Entries<'_, 'de> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        seed.deserialize(&mut *self.decoder).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(&mut *self.decoder)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

/// An enum: the `u32` variant index, then the variant's fields.
struct Variant<'a, 'de> {
    decoder: &'a mut Decoder<'de>,
}

impl<'a, 'de> EnumAccess<'de> for Variant<'a, 'de> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), Error> {
        let index = u32::from_le_bytes(self.decoder.array()?);
        let index: de::value::U32Deserializer<Error> = index.into_deserializer();
        Ok((seed.deserialize(index)?, self))
    }
}

impl<'de> VariantAccess<'de> for Variant<'_, 'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(self.decoder)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        self.decoder.fields(len, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.decoder.fields(fields.len(), visitor)
    }
}
