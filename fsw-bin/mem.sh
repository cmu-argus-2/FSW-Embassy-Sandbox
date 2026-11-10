#!/bin/bash
# Usage: ./mem.sh <binary-name|elf-file> [count]
#
# Static RAM of a firmware ELF, read from the ELF (no board needed), in bytes and as a percentage
# of total RAM: the .data/.bss/.uninit totals, each embassy task's storage (its POOL: everything
# the task keeps across awaits), and the [count] (default 20) largest statics.
#
# The stack is not included: it is only known at runtime. Binaries that call
# fsw_board::stack::paint() report it as a stack_peak BENCH row in their logs.
#
# A binary name resolves to the workspace's release build, since debug builds use more RAM.
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "Usage: $0 <binary-name|elf-file> [count]" >&2
  exit 1
fi

ELF=$1
COUNT=${2:-20}
case "$ELF" in
  */*) ;; # Explicit paths are used as supplied.
  *)
    SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
    ELF="$SCRIPT_DIR/../target/thumbv8m.main-none-eabihf/release/$ELF"
    ;;
esac
if [ ! -f "$ELF" ]; then
  echo "ELF file not found: $ELF (build it with cargo build --release --bin <name>)" >&2
  exit 1
fi

NM=$(command -v llvm-nm || command -v arm-none-eabi-nm || true)
SIZE=$(command -v llvm-size || command -v arm-none-eabi-size || true)
if [ -z "$NM" ] || [ -z "$SIZE" ]; then
  echo "llvm-nm and llvm-size (or arm-none-eabi-nm/size) are required" >&2
  exit 1
fi

NAME=$(basename "$ELF")

# Section sizes in bytes ("name size addr" rows from size -A).
section() {
  "$SIZE" -A "$ELF" | awk -v s="$1" '$1 == s { print $2; found = 1 } END { if (!found) print 0 }'
}
DATA=$(section .data)
BSS=$(section .bss)
UNINIT=$(section .uninit)
STATIC=$((DATA + BSS + UNINIT))

# Total RAM: from the start of static RAM to the top of the stack (cortex-m-rt linker symbols).
addr() {
  "$NM" -t d "$ELF" | awk -v s="$1" '$3 == s { print $1 + 0 }'
}
RAM_START=$(addr __sdata)
RAM_END=$(addr _stack_start)
if [ -z "$RAM_START" ] || [ -z "$RAM_END" ]; then
  echo "__sdata or _stack_start not found in $ELF" >&2
  exit 1
fi
RAM=$((RAM_END - RAM_START))

# Print "<bytes> <percent of RAM>" for each line's first field, then the rest of the line.
with_percent() {
  awk -v ram="$RAM" '{ size = $1; $1 = ""; printf "  %7d  %5.2f %%%s\n", size, size * 100 / ram, $0 }'
}

# RAM symbols (.data/.bss: types d D b B) as "size name", largest first, with the
# ::h<hash> suffix and the .llvm.<n> annotation dropped from the demangled names.
ram_symbols() {
  "$NM" -C -S --size-sort -t d "$ELF" |
    awk '$3 ~ /^[bBdD]$/ { size = $2 + 0; $1 = $2 = $3 = ""; sub(/^ +/, ""); print size, $0 }' |
    sed -E 's/::h[0-9a-f]{16}//; s/ \(\.llvm\.[0-9]+\)$//' |
    sort -rn
}

echo "$NAME: $ELF"
echo
awk -v s="$STATIC" -v ram="$RAM" -v d="$DATA" -v b="$BSS" -v u="$UNINIT" 'BEGIN {
  printf "Static RAM: %d B, %.2f %% of %d KiB (.data %d, .bss %d, .uninit %d)\n", s, s * 100 / ram, ram / 1024, d, b, u
}'
echo "The stack peak adds to this at runtime: see the stack_peak BENCH row in the firmware logs."
echo
echo "Task storage (embassy task POOLs):"
ram_symbols | awk '$2 ~ /::POOL$/ { sub(/::POOL$/, "", $2); print $1, $2 }' | with_percent
echo
echo "Largest $COUNT statics:"
ram_symbols | head -n "$COUNT" | with_percent
echo
echo "BENCH,rust-embassy,memory,static_ram,$NAME,total,$STATIC,B,1"
awk -v s="$STATIC" -v ram="$RAM" -v n="$NAME" 'BEGIN {
  printf "BENCH,rust-embassy,memory,static_ram,%s,total,%.2f,%%,1\n", n, s * 100 / ram
}'
