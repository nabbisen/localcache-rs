//! Writes `vectors.txt`: the bytes `bincode` 2.0.1 (serde mode,
//! `config::legacy()`) produces for every case of `zoo.rs`.
//!
//! This is a generator, not a test: it is **not** compiled by the workspace and
//! `bincode` is not a dependency of any manifest in this repository. Run it once,
//! in a scratch crate, as `README.md` describes. Regenerating the vectors is the
//! loud act that signals a wire-format change (RFC 008, RFC 028).

mod zoo;

use std::fmt::Write as _;

struct Bincode;

impl zoo::ZooCodec for Bincode {
    fn encode<T: serde::Serialize>(value: &T) -> Result<Vec<u8>, String> {
        bincode::serde::encode_to_vec(value, bincode::config::legacy()).map_err(|e| e.to_string())
    }

    fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
        bincode::serde::decode_from_slice(bytes, bincode::config::legacy())
            .map(|(value, _)| value)
            .map_err(|e| e.to_string())
    }
}

fn main() {
    let mut out = String::new();
    let mut names = std::collections::BTreeSet::new();
    zoo::each::<Bincode>(&mut |case| {
        assert!(names.insert(case.name), "duplicate case name {}", case.name);
        let bytes = case
            .encoded
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        // The oracle must agree with itself before its bytes become a vector.
        let decoded = (case.decode)(&bytes).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert!(
            decoded.same_value,
            "{}: bincode does not round-trip",
            case.name
        );
        assert_eq!(
            decoded.reencoded.as_deref(),
            Ok(bytes.as_slice()),
            "{}",
            case.name
        );
        let _ = write!(out, "{} =", case.name);
        if !bytes.is_empty() {
            out.push(' ');
        }
        for byte in &bytes {
            let _ = write!(out, "{byte:02x}");
        }
        out.push('\n');
    });
    print!("{out}");
}
