//! The encoder: a `serde::Serializer` writing the legacy payload format.

use serde::ser::{
    self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
};

use super::{DEPTH_LIMIT, Error};

/// Bytes written so far, and the depth reached.
///
/// The depth is counted exactly as the decoder counts it, so that a value that
/// can be written can be read back: an engine never stores what it would refuse
/// to read (RFC 028 Amendment 1, F1).
pub(super) struct Encoder {
    out: Vec<u8>,
    depth: usize,
}

impl Encoder {
    pub(super) fn new() -> Self {
        Self {
            out: Vec::new(),
            depth: 0,
        }
    }

    /// Start a level, or fail if that exceeds [`DEPTH_LIMIT`]. Pair with `leave`.
    fn enter(&mut self) -> Result<(), Error> {
        if self.depth >= DEPTH_LIMIT {
            return Err(Error::TooDeep);
        }
        self.depth += 1;
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// Run `write` one level deeper.
    fn nested(&mut self, write: impl FnOnce(&mut Self) -> Result<(), Error>) -> Result<(), Error> {
        self.enter()?;
        let written = write(self);
        self.leave();
        written
    }

    fn put_u32(&mut self, value: u32) {
        self.out.extend_from_slice(&value.to_le_bytes());
    }

    pub(super) fn into_bytes(self) -> Vec<u8> {
        self.out
    }

    fn length_prefix(&mut self, length: usize) -> Result<(), Error> {
        let length = u64::try_from(length).map_err(|_| Error::LengthTooLarge)?;
        self.out.extend_from_slice(&length.to_le_bytes());
        Ok(())
    }
}

/// The elements of a sequence, map, tuple, struct or enum variant.
///
/// A sequence or map must write as many elements as it declared, or its length
/// prefix would not match its bytes and the payload could never be read back.
pub(super) struct Compound<'a> {
    encoder: &'a mut Encoder,
    /// The declared element count, for a sequence or map.
    declared: Option<usize>,
    /// Elements (a map's keys) written so far.
    written: usize,
}

impl<'a> Compound<'a> {
    fn fixed(encoder: &'a mut Encoder) -> Self {
        Self {
            encoder,
            declared: None,
            written: 0,
        }
    }

    fn counted(encoder: &'a mut Encoder, declared: usize) -> Self {
        Self {
            encoder,
            declared: Some(declared),
            written: 0,
        }
    }

    fn element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.written += 1;
        value.serialize(&mut *self.encoder)
    }

    fn finish(self) -> Result<(), Error> {
        self.encoder.leave();
        match self.declared {
            Some(declared) if declared != self.written => Err(Error::LengthMismatch {
                declared,
                written: self.written,
            }),
            _ => Ok(()),
        }
    }
}

// The primitive writers are `#[inline]` on purpose. They are not generic, so without the attribute a
// downstream crate calls each one as an exported function, once per element of a sequence; with it the
// writes are compiled into the caller. Measured on a 1 MiB `Vec<u8>`: a `set` 3.4x faster, and the
// step-wise timings (4K aliasing between the input and the output buffer) gone. Only the writers that
// run per element carry it: the sequence and map openers, the compound closers and the helpers showed no
// effect and stay as they were.
impl<'a> ser::Serializer for &'a mut Encoder {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    #[inline]
    fn serialize_bool(self, v: bool) -> Result<(), Error> {
        self.out.push(u8::from(v));
        Ok(())
    }

    #[inline]
    fn serialize_i8(self, v: i8) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_i16(self, v: i16) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_i32(self, v: i32) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_i64(self, v: i64) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_i128(self, v: i128) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_u8(self, v: u8) -> Result<(), Error> {
        self.out.push(v);
        Ok(())
    }

    #[inline]
    fn serialize_u16(self, v: u16) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_u32(self, v: u32) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_u64(self, v: u64) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_u128(self, v: u128) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_f32(self, v: f32) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_bits().to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_f64(self, v: f64) -> Result<(), Error> {
        self.out.extend_from_slice(&v.to_bits().to_le_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_char(self, v: char) -> Result<(), Error> {
        let mut buffer = [0_u8; 4];
        self.out
            .extend_from_slice(v.encode_utf8(&mut buffer).as_bytes());
        Ok(())
    }

    #[inline]
    fn serialize_str(self, v: &str) -> Result<(), Error> {
        self.serialize_bytes(v.as_bytes())
    }

    #[inline]
    fn serialize_bytes(self, v: &[u8]) -> Result<(), Error> {
        self.length_prefix(v.len())?;
        self.out.extend_from_slice(v);
        Ok(())
    }

    #[inline]
    fn serialize_none(self) -> Result<(), Error> {
        self.out.push(0);
        Ok(())
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), Error> {
        self.out.push(1);
        self.nested(|encoder| value.serialize(encoder))
    }

    #[inline]
    fn serialize_unit(self) -> Result<(), Error> {
        Ok(())
    }

    #[inline]
    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Error> {
        Ok(())
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<(), Error> {
        self.nested(|encoder| {
            encoder.put_u32(variant_index);
            Ok(())
        })
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.nested(|encoder| value.serialize(encoder))
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.nested(|encoder| {
            encoder.put_u32(variant_index);
            value.serialize(encoder)
        })
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Compound<'a>, Error> {
        let len = len.ok_or(Error::UnknownLength)?;
        self.enter()?;
        self.length_prefix(len)?;
        Ok(Compound::counted(self, len))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Compound<'a>, Error> {
        self.enter()?;
        Ok(Compound::fixed(self))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Compound<'a>, Error> {
        self.enter()?;
        Ok(Compound::fixed(self))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Compound<'a>, Error> {
        self.enter()?;
        self.put_u32(variant_index);
        Ok(Compound::fixed(self))
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Compound<'a>, Error> {
        let len = len.ok_or(Error::UnknownLength)?;
        self.enter()?;
        self.length_prefix(len)?;
        Ok(Compound::counted(self, len))
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Compound<'a>, Error> {
        self.enter()?;
        Ok(Compound::fixed(self))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Compound<'a>, Error> {
        self.enter()?;
        self.put_u32(variant_index);
        Ok(Compound::fixed(self))
    }

    #[inline]
    fn is_human_readable(&self) -> bool {
        false
    }
}

impl SerializeSeq for Compound<'_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.element(value)
    }

    fn end(self) -> Result<(), Error> {
        self.finish()
    }
}

impl SerializeTuple for Compound<'_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.element(value)
    }

    fn end(self) -> Result<(), Error> {
        self.finish()
    }
}

impl SerializeTupleStruct for Compound<'_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.element(value)
    }

    fn end(self) -> Result<(), Error> {
        self.finish()
    }
}

impl SerializeTupleVariant for Compound<'_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.element(value)
    }

    fn end(self) -> Result<(), Error> {
        self.finish()
    }
}

impl SerializeMap for Compound<'_> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        self.element(key)
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        value.serialize(&mut *self.encoder)
    }

    fn end(self) -> Result<(), Error> {
        self.finish()
    }
}

impl SerializeStruct for Compound<'_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.element(value)
    }

    fn end(self) -> Result<(), Error> {
        self.finish()
    }
}

impl SerializeStructVariant for Compound<'_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.element(value)
    }

    fn end(self) -> Result<(), Error> {
        self.finish()
    }
}
