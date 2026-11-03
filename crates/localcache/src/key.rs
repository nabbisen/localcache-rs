//! The crate's own type for the encryption key (RFC 029 R5, v0.23.1).
//!
//! `Key` is the only form in which the crate holds a key for the life of an
//! engine. It is not `Copy` and not `Clone`, erases its bytes when it is
//! dropped, prints `[redacted]`, and hands its bytes out by reference only.
//!
//! What this is **not**: a promise that the key cannot leak. It does not erase
//! the caller's own copy of the key, a register, the stack frames of other
//! code, the cipher library's per-call key schedule, or the swap file. It is
//! "the crate does not keep its own copies of the key after they are dropped".

use zeroize::{Zeroize, ZeroizeOnDrop};

/// An AES-256 key, erased when dropped.
///
/// Not `Clone`: no use in the crate needs a second copy (the engine borrows
/// it for each call, and a rotation moves the new key into the engine).
pub(crate) struct Key([u8; 32]);

impl Key {
    /// Copy `bytes` into a new key, or `None` when they are not exactly 32.
    pub(crate) fn from_slice(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 32 {
            return None;
        }
        let mut key = Self([0; 32]);
        key.0.copy_from_slice(bytes);
        Some(key)
    }

    /// The key's bytes, by reference.
    pub(crate) fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Zeroize for Key {
    fn zeroize(&mut self) {
        self.0.zeroize();
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        self.zeroize();
    }
}

impl ZeroizeOnDrop for Key {}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Owns a key `Vec<u8>` taken out of the options and erases it, whole buffer
/// and length, when it is dropped: on every way out of `open`, success or error.
pub(crate) struct ErasedOnDrop(Option<Vec<u8>>);

impl ErasedOnDrop {
    pub(crate) fn new(bytes: Option<Vec<u8>>) -> Self {
        Self(bytes)
    }

    pub(crate) fn as_deref(&self) -> Option<&[u8]> {
        self.0.as_deref()
    }
}

impl Drop for ErasedOnDrop {
    fn drop(&mut self) {
        erase(&mut self.0);
    }
}

/// Erase a key `Vec<u8>` in place and leave `None`.
///
/// `Vec<u8>::zeroize` overwrites the elements and the spare capacity, then
/// clears the vector (documented behaviour of the `zeroize` crate).
pub(crate) fn erase(bytes: &mut Option<Vec<u8>>) {
    if let Some(mut vec) = bytes.take() {
        vec.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}

    const BYTES: [u8; 32] = [
        0x3a, 0x91, 0xc4, 0x07, 0xe2, 0x58, 0xb6, 0x1d, 0x7f, 0xa0, 0x4c, 0xd9, 0x26, 0x8b, 0xf3,
        0x65, 0x12, 0xce, 0x9d, 0x30, 0xab, 0x74, 0x59, 0xe8, 0x0f, 0xbd, 0x41, 0x96, 0xc7, 0x2a,
        0x83, 0x5e,
    ];

    /// RFC 029 R5: the type erases itself on drop. A compile-time assertion;
    /// the crate has no `unsafe`, so the zeros are checked through the
    /// `Zeroize` trait the `Drop` impl calls, not by reading freed memory.
    #[test]
    fn key_is_zeroize_on_drop() {
        assert_zeroize_on_drop::<Key>();
    }

    #[test]
    fn zeroizing_a_key_leaves_zeros() {
        let mut key = Key::from_slice(&BYTES).unwrap();
        assert_eq!(key.as_bytes(), &BYTES);
        key.zeroize();
        assert_eq!(key.as_bytes(), &[0_u8; 32]);
    }

    #[test]
    fn a_key_needs_exactly_32_bytes() {
        assert!(Key::from_slice(&BYTES[..31]).is_none());
        assert!(Key::from_slice(&[0; 33]).is_none());
        assert!(Key::from_slice(&BYTES).is_some());
    }

    #[test]
    fn a_key_prints_redacted_and_nothing_of_its_bytes() {
        let key = Key::from_slice(&BYTES).unwrap();
        assert_eq!(format!("{key:?}"), "[redacted]");
        assert_eq!(format!("{key:#?}"), "[redacted]");
    }

    #[test]
    fn erasing_a_key_vec_leaves_none() {
        let mut bytes = Some(BYTES.to_vec());
        erase(&mut bytes);
        assert!(bytes.is_none());
        let mut nothing: Option<Vec<u8>> = None;
        erase(&mut nothing);
        assert!(nothing.is_none());
    }

    /// The guard hands the bytes out by reference while it lives. That it
    /// erases them when dropped is `erase`, tested above (the crate has no
    /// `unsafe`, so freed memory is not read back).
    #[test]
    fn the_guard_hands_out_the_bytes() {
        let guard = ErasedOnDrop::new(Some(BYTES.to_vec()));
        assert_eq!(guard.as_deref(), Some(&BYTES[..]));
        drop(guard);
        assert!(ErasedOnDrop::new(None).as_deref().is_none());
    }
}
