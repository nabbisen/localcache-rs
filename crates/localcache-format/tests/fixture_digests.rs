//! The codec fixtures are the commitment of the payload format (RFC 028): the
//! vectors are the oracle, and the generator that wrote them is recorded by
//! digest in `tests/fixtures/payload_codec/README.md`. A change to any of the
//! three files is a change of the format's record and must be deliberate: this
//! test fails first.

use sha2::{Digest, Sha256};

fn digest(path: &str) -> String {
    let full = format!(
        "{}/tests/fixtures/payload_codec/{path}",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&full).unwrap_or_else(|error| panic!("cannot read {full}: {error}"));
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
}

#[test]
#[cfg_attr(
    miri,
    ignore = "reads files, which Miri's isolation refuses; checked natively"
)]
fn the_vectors_and_their_generator_are_the_ones_the_readme_records() {
    assert_eq!(
        digest("vectors.txt"),
        "f3c51b8244af1c439ee980049641ef231564f6ebfc73c841b8bee3fe4a50e129"
    );
    assert_eq!(
        digest("generate.rs"),
        "b80cd75cb64ffdeaad46eb062036cf281a1afed5448eba4f321615ece3ce08d0"
    );
    // The comment in `zoo.rs` still names the file's former place
    // (`src/serialization/payload_codec/tests.rs`). It was not edited when the
    // code moved, because the digest below is the record of what generated the
    // vectors.
    assert_eq!(
        digest("zoo.rs"),
        "12d72f33cfd6c8b74a2693cc1d2024dbe3bfc469ddf029af8d9687ad5b8accf7"
    );
}
