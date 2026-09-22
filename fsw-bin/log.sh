#!/bin/bash
# Usage: ./log.sh <binary-name|elf-file> [serial-port]
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "Usage: $0 <binary-name|elf-file> [serial-port]" >&2
  exit 1
fi

ELF=$1
case "$ELF" in
  */*) ;; # Explicit paths are used as supplied.
  *)
    SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
    ELF="$SCRIPT_DIR/../target/thumbv8m.main-none-eabihf/debug/$ELF"
    ;;
esac
if [ ! -f "$ELF" ]; then
  echo "ELF file not found: $ELF" >&2
  exit 1
fi
command -v defmt-print >/dev/null || { echo "defmt-print is required" >&2; exit 1; }

PORT=${2:-}
if [ -z "$PORT" ]; then
  shopt -s nullglob
  ports=(/dev/cu.usbmodem* /dev/ttyACM*)
  if [ "${#ports[@]}" -ne 1 ]; then
    echo "Expected one USB serial device; specify its port as the second argument." >&2
    exit 1
  fi
  PORT=${ports[0]}
fi

# Block for one byte, then drain available bytes without waiting to fill a
# large read. No FIFO or separate background reader is needed.
python3 -u - "$PORT" <<'PY' | defmt-print -e "$ELF"
import sys
import serial

try:
  with serial.Serial(sys.argv[1], 115200, timeout=None,
                     dsrdtr=True, rtscts=False) as port:
    port.dtr = True
    port.rts = False
    while True:
        data = port.read(1)
        pending = port.in_waiting
        if pending:
            data += port.read(pending)
        sys.stdout.buffer.write(data)
except (KeyboardInterrupt, BrokenPipeError):
    pass
except serial.SerialException as exc:
    print(f"Serial error: {exc}", file=sys.stderr)
    sys.exit(1)
PY
