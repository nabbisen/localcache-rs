#!/bin/sh
set -eu

# RFC009_RC_ELIGIBLE=1 below is this wrapper's exclusive attestation that a
# genuine canonical run is in progress. `scripts/release.py` treats
# `rc_eligible` as external, not self-asserted (RFC 009 M6c item 5): it never
# sets this variable itself, and it is required in addition to — not instead
# of — the existing RFC009_PRODUCER_IMAGE/platform/locale/base-component
# checks. Do not set RFC009_RC_ELIGIBLE outside this file.

IMAGE='docker.io/library/rust@sha256:389c1ae98c20fbcadca68a685482749267cec3c90893ae4671c5a37cc894c416'
PLATFORM='linux/amd64'
MDBOOK_URL='https://github.com/rust-lang/mdBook/releases/download/v0.5.4/mdbook-v0.5.4-x86_64-unknown-linux-gnu.tar.gz'
MDBOOK_ARCHIVE_SHA256='3f28de05dafca9d0f2eab99c662116b0e37b89b1d96a08f8f430b9eeae958cd7'
MDBOOK_BINARY_SHA256='dc9020903f60cf632c0cc01bf8c12b57237649c2ab64c7a85a60e12103e356ce'

ROOT=$(git rev-parse --show-toplevel)
OUTPUT=${1:-"$ROOT/.git-exclude/release-output"}
TOOLS="$ROOT/.git-exclude/canonical-tools"
CARGO_CACHE="$ROOT/.git-exclude/canonical-cargo"
MDBOOK_ARCHIVE="$TOOLS/mdbook-v0.5.4-x86_64-unknown-linux-gnu.tar.gz"
MDBOOK="$TOOLS/mdbook"

mkdir -p "$TOOLS" "$CARGO_CACHE" "$OUTPUT"
if [ -n "$(find "$OUTPUT" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
    echo "canonical-producer: output directory is not empty: $OUTPUT" >&2
    exit 1
fi

if [ ! -f "$MDBOOK_ARCHIVE" ]; then
    curl -fL --retry 3 "$MDBOOK_URL" -o "$MDBOOK_ARCHIVE"
fi
printf '%s  %s\n' "$MDBOOK_ARCHIVE_SHA256" "$MDBOOK_ARCHIVE" | sha256sum -c -

if [ ! -x "$MDBOOK" ]; then
    tar -xzf "$MDBOOK_ARCHIVE" -C "$TOOLS" mdbook
    chmod 0755 "$MDBOOK"
fi
printf '%s  %s\n' "$MDBOOK_BINARY_SHA256" "$MDBOOK" | sha256sum -c -

docker pull --platform "$PLATFORM" "$IMAGE"
OBSERVED_PLATFORM=$(docker image inspect \
    --format '{{.Os}}/{{.Architecture}}' "$IMAGE")
if [ "$OBSERVED_PLATFORM" != "$PLATFORM" ]; then
    echo "canonical-producer: expected $PLATFORM, found $OBSERVED_PLATFORM" >&2
    exit 1
fi

docker run --rm --read-only --platform "$PLATFORM" \
    --user "$(id -u):$(id -g)" \
    --tmpfs /tmp:rw,nosuid,nodev \
    --mount "type=bind,src=$ROOT,dst=/workspace,readonly" \
    --mount "type=bind,src=$OUTPUT,dst=/output" \
    --mount "type=bind,src=$CARGO_CACHE,dst=/cargo-cache" \
    --mount "type=bind,src=$MDBOOK,dst=/usr/local/bin/mdbook,readonly" \
    --env CARGO_HOME=/cargo-cache \
    --env RUSTUP_HOME=/usr/local/rustup \
    --env HOME=/tmp/producer-home \
    --env LC_ALL=C.UTF-8 \
    --env TZ=UTC \
    --env RFC009_PRODUCER_IMAGE="$IMAGE" \
    --env RFC009_RC_ELIGIBLE=1 \
    --env GIT_CONFIG_COUNT=1 \
    --env GIT_CONFIG_KEY_0=safe.directory \
    --env GIT_CONFIG_VALUE_0=/workspace \
    --workdir /workspace \
    "$IMAGE" \
    python3 scripts/release.py source --output-dir /output
