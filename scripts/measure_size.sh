#!/usr/bin/env bash
#
# Flash and RAM usage of each firmware binary, as CSV rows for the port comparison.
#
#   ./scripts/measure_size.sh                  # every binary, release profile
#   ./scripts/measure_size.sh eps_task         # just one
#   ./scripts/measure_size.sh --profile debug  # debug instead of release
#
# Flash is text+data (what is programmed). Static RAM is data+bss (reserved before
# anything runs); stack and any runtime allocation sit on top of this.
#
# Compare release against release only: debug is roughly 4x the flash of release
# here, so a port built at a different optimisation level is not comparable.
#
# Exits non-zero if any binary fails to build, so a partial run cannot be mistaken
# for a complete one. The commit column is marked -dirty when the working tree has
# uncommitted changes, because the numbers then do not correspond to that commit.
#
# Needs: arm-none-eabi-size (binutils-arm-none-eabi) and the thumbv8m target.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET=thumbv8m.main-none-eabihf
PORT=rust-embassy
PROFILE=release
BINS=()
FAILED=0

usage() { sed -n '3,19p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
    case "$1" in
        --profile)
            [ $# -ge 2 ] || { echo "--profile needs a value" >&2; exit 2; }
            PROFILE="$2"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        -*) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
        *) BINS+=("$1"); shift ;;
    esac
done

case "$PROFILE" in
    release|debug) ;;
    *) echo "profile must be release or debug, got '$PROFILE'" >&2; exit 2 ;;
esac

command -v arm-none-eabi-size >/dev/null || {
    echo "arm-none-eabi-size not found (apt install binutils-arm-none-eabi)" >&2
    exit 1
}

rustup target list --installed 2>/dev/null | grep -qx "$TARGET" || {
    echo "target $TARGET not installed (rustup target add $TARGET)" >&2
    exit 1
}

# Default to every binary in fsw-bin
if [ "${#BINS[@]}" -eq 0 ]; then
    for f in "$REPO"/fsw-bin/src/bin/*.rs; do
        [ -e "$f" ] || { echo "no binaries found in fsw-bin/src/bin" >&2; exit 1; }
        BINS+=("$(basename "$f" .rs)")
    done
fi

commit="$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)"
if ! git -C "$REPO" diff --quiet HEAD 2>/dev/null; then
    commit="${commit}-dirty"
    echo "warning: working tree has uncommitted changes; these numbers are not reproducible from $commit" >&2
fi

build_flag=()
[ "$PROFILE" = release ] && build_flag=(--release)

echo "port,category,metric,subject,stat,value,unit,n,build,commit"
for bin in "${BINS[@]}"; do
    # The target has to be explicit: the cargo config that sets it lives in fsw-bin/,
    # so building from the repo root would otherwise target the host and fail in cortex-m
    if ! (cd "$REPO" && cargo build -p fsw-bin --bin "$bin" --target "$TARGET" "${build_flag[@]}" >/dev/null 2>&1); then
        echo "build failed: $bin" >&2
        FAILED=1
        continue
    fi

    elf="$REPO/target/$TARGET/$PROFILE/$bin"
    if [ ! -f "$elf" ]; then
        echo "no ELF produced for $bin at $elf" >&2
        FAILED=1
        continue
    fi

    read -r text data bss <<<"$(arm-none-eabi-size "$elf" | awk 'NR==2 {print $1, $2, $3}')"
    if [ -z "${bss:-}" ]; then
        echo "could not parse size output for $bin" >&2
        FAILED=1
        continue
    fi

    printf '%s,efficiency,flash,%s,total,%s,bytes,1,%s,%s\n' "$PORT" "$bin" "$((text + data))" "$PROFILE" "$commit"
    printf '%s,efficiency,ram_static,%s,total,%s,bytes,1,%s,%s\n' "$PORT" "$bin" "$((data + bss))" "$PROFILE" "$commit"
done

exit "$FAILED"
