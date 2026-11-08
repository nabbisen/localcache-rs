//! The cipher erases its key schedule when it is dropped (Q30).
//!
//! `aes-gcm` is built with its `zeroize` feature in this workspace; with it, dropping an `Aes256Gcm`
//! overwrites the round keys and the GHASH key it holds. This test proves the wiring: it fails if the
//! feature is removed from the manifest. It does not prove that no other copy of the key exists in the
//! process (the 32-byte array the caller passed, registers, stack temporaries): see `docs/src/security.md`.
//!
//! The test lives here, not in the library, because the library is `#![forbid(unsafe_code)]`; an
//! integration test is a separate crate.
//!
//! Soundness. The object is built in storage the test owns (a `MaybeUninit<Aes256Gcm>`), dropped in place
//! with `drop_in_place`, and its bytes are read afterwards through a pointer taken from that same storage.
//! The storage stays allocated and is never used as an `Aes256Gcm` again. Reading the bytes is sound only
//! if every byte of the object was written by the drop; `zeroize` does write every byte (it zeroes
//! `size_of::<T>()` bytes of the object, padding included), which is exactly the property under test. If
//! the feature were off, some bytes could be uninitialized padding and the read would be undefined behaviour
//! in the strict sense; that configuration is only used as the negative control of this test (it fails),
//! not shipped.
#![cfg(feature = "encryption")]

use std::mem::{MaybeUninit, size_of};

use aes_gcm::{Aes256Gcm, KeyInit};

const KEY: [u8; 32] = [0x42; 32];

#[test]
fn dropping_the_cipher_overwrites_its_whole_object_with_zeros() {
    let size = size_of::<Aes256Gcm>();
    // The AES-256 round keys alone are 240 bytes: a smaller object would mean the test measures a
    // different type than the one the library uses.
    assert!(
        size >= 240,
        "unexpectedly small cipher object: {size} bytes"
    );

    let cipher = Aes256Gcm::new_from_slice(&KEY).expect("a 32-byte key");

    let mut slot = MaybeUninit::<Aes256Gcm>::uninit();
    // SAFETY: `slot` is `size` bytes of writable, properly aligned storage owned by this test; writing
    // `cipher` into it initializes it as an `Aes256Gcm` (the moved-from local is not dropped again).
    unsafe { slot.as_mut_ptr().write(cipher) };

    // SAFETY: `slot` holds an initialized `Aes256Gcm`; it is dropped exactly once, here, in place, and
    // never used as an `Aes256Gcm` afterwards (`MaybeUninit` does not drop it again).
    unsafe { std::ptr::drop_in_place(slot.as_mut_ptr()) };

    // SAFETY: the storage is still allocated and `size` bytes long; every byte of it was written by the
    // drop (see the module comment), so reading them as `u8` reads initialized memory.
    let bytes = unsafe { std::slice::from_raw_parts(slot.as_ptr().cast::<u8>(), size) };
    let nonzero = bytes.iter().filter(|byte| **byte != 0).count();
    assert_eq!(
        nonzero, 0,
        "{nonzero} of {size} bytes of the dropped cipher are not zero: its key schedule was not erased \
         (is the `zeroize` feature of `aes-gcm` enabled in the workspace manifest?)"
    );
}
