#!/usr/bin/env bash
#
# Argus FSW test runner.
#
#   ./test_all.sh                      host tests + build, then flatsat_check if a board is connected
#   ./test_all.sh --no-board           host tests + build only, never touch the board
#   ./test_all.sh --board rtc_only     run one specific binary on the board
#   ./test_all.sh --board --all        run every safe board test, one after another
#   ./test_all.sh --list               show which binaries can be run and which are blocked
#
# Options:
#   --duration SECONDS   how long to capture logs for (per test; default is per-test below)
#   --port /dev/ttyACM0  serial port (autodetected otherwise)
#   --out DIR            where to write logs (default: test-logs/<timestamp>)
#   --fire               allow burnwire_pca9633, which really does fire a burn wire
#   --force              allow the binaries that are wrong for Mainboard v4
#   --skip-host          skip host tests and the build, go straight to the board
#
# Needs: cargo; and for --board: picotool, defmt-print, python3 with pyserial.

set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET_DIR="$REPO/target/thumbv8m.main-none-eabihf/debug"
HOST_TARGET="x86_64-unknown-linux-gnu"

# name:seconds:category   safe = runs on request, fire = needs --fire, unsafe = needs --force
BOARD_TESTS=(
    "flatsat_check:90:safe:full checkout of every device on both buses"
    "scan_i2c:20:safe:I2C0 bus scan"
    "rtc_only:20:safe:DS3231 clock readout"
    "rtc_test:20:safe:DS3231 with build-time sync"
    "lux_only:25:safe:OPT4003 on I2C0, with address search"
    "lux_test:20:safe:OPT4003 on I2C1"
    "test_sensors:20:safe:OPT4003 via shared bus"
    "test_drivers:20:safe:ADM1176 power monitor"
    "sdcard_test:30:safe:SD card read/write"
    "torque_coil_test:60:safe:DRV8235 coils, drives each briefly"
    "burnwire_pca9633:40:fire:FIRES a burn wire on channel 0"
    "burnwire_test:20:unsafe:wrong chip and bus for v4, drives the SD clock pin"
    "stepper_test:20:unsafe:no such hardware on v4, drives the watchdog enable"
)

RUN_BOARD=0
RUN_ALL_SAFE=0
BOARD_TEST=""
DURATION=""
PORT=""
OUT_DIR=""
ALLOW_FIRE=0
ALLOW_UNSAFE=0
SKIP_HOST=0
NO_BOARD=0

PASSED=(); FAILED=(); SKIPPED=()
BOARD_PASSED=(); BOARD_FAILED=(); BOARD_SKIPPED=()
# "host" until the board tests start; decides which tally a result lands in
PHASE="host"

red()   { printf '\033[31m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
yellow(){ printf '\033[33m%s\033[0m\n' "$*"; }
bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
rule()  { printf '%s\n' "------------------------------------------------------------"; }

pass() { green  "  PASS  $1"; [ "$PHASE" = board ] && BOARD_PASSED+=("$1")  || PASSED+=("$1"); }
fail() { red    "  FAIL  $1"; [ "$PHASE" = board ] && BOARD_FAILED+=("$1")  || FAILED+=("$1"); }
skip() { yellow "  SKIP  $1 ($2)"; [ "$PHASE" = board ] && BOARD_SKIPPED+=("$1") || SKIPPED+=("$1"); }

usage() { awk 'NR > 1 { if (!/^#/) exit; sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"; }

list_tests() {
    local entry name secs category desc
    bold "Board tests"
    rule
    for entry in "${BOARD_TESTS[@]}"; do
        IFS=: read -r name secs category desc <<<"$entry"
        case "$category" in
            safe)   printf '  %-20s %3ss  %s\n' "$name" "$secs" "$desc" ;;
            fire)   printf '  %-20s %3ss  [--fire]  %s\n' "$name" "$secs" "$desc" ;;
            unsafe) printf '  %-20s %3ss  [--force] %s\n' "$name" "$secs" "$desc" ;;
        esac
    done
}

lookup() { # lookup <name> <field 2=secs 3=category 4=desc>
    local entry name secs category desc
    for entry in "${BOARD_TESTS[@]}"; do
        IFS=: read -r name secs category desc <<<"$entry"
        if [ "$name" = "$1" ]; then
            case "$2" in
                2) echo "$secs" ;; 3) echo "$category" ;; 4) echo "$desc" ;;
            esac
            return 0
        fi
    done
    return 1
}

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        red "Missing required tool: $1${2:+ ($2)}"
        return 1
    fi
}

find_port() {
    local p
    for p in /dev/tty.usbmodem* /dev/ttyACM*; do
        [ -e "$p" ] && { echo "$p"; return 0; }
    done
    return 1
}

# Wait for the USB serial port to come back after a reset
wait_for_port() {
    local waited=0 p
    while [ "$waited" -lt 20 ]; do
        if p="$(find_port)"; then
            echo "$p"
            return 0
        fi
        sleep 1
        waited=$((waited + 1))
    done
    return 1
}

# First python on the system that actually has pyserial (the default python3 may not)
find_python() {
    local p
    for p in python3 /usr/bin/python3 python; do
        command -v "$p" >/dev/null 2>&1 || continue
        "$p" -c "import serial" >/dev/null 2>&1 && { command -v "$p"; return 0; }
    done
    return 1
}

# A board counts as present in BOOTSEL too, where it has no serial port yet
board_present() {
    find_port >/dev/null && return 0
    command -v picotool >/dev/null 2>&1 && picotool info >/dev/null 2>&1 && return 0
    return 1
}

run_host_tests() {
    bold "[1/2] Host tests - run on this laptop against simulated hardware, NOT the board"
    rule
    if (cd "$REPO/fsw-test" && cargo test --target "$HOST_TARGET" --color always 2>&1 | tee "$OUT_DIR/host-tests.log" | grep -E "^test |test result|^error|panicked"); then
        if grep -qE "test result: FAILED|^error" "$OUT_DIR/host-tests.log"; then
            fail "host tests"
        else
            pass "host tests"
        fi
    else
        fail "host tests"
    fi
    echo
}

build_firmware() {
    bold "[2/2] Firmware build - compiles only, nothing is run on the board"
    rule
    if (cd "$REPO" && cargo build -p fsw-bin --color always 2>&1 | tee "$OUT_DIR/build.log" | grep -E "^error|^warning: unused|Finished|Compiling fsw"); then
        if grep -qE "^error" "$OUT_DIR/build.log"; then
            fail "firmware build"
        else
            pass "firmware build"
        fi
    else
        fail "firmware build"
    fi
    echo
}

# Flash one binary and capture its log until DURATION expires.
run_on_board() {
    local name="$1"
    local secs elf logfile port reader_pid
    secs="${DURATION:-$(lookup "$name" 2)}"
    elf="$TARGET_DIR/$name"
    logfile="$OUT_DIR/$name.log"

    bold "Board test: $name  ($(lookup "$name" 4))"
    rule

    if ! (cd "$REPO" && cargo build -p fsw-bin --bin "$name" >>"$OUT_DIR/build.log" 2>&1); then
        fail "$name (build failed, see $OUT_DIR/build.log)"
        return
    fi

    echo "Flashing (put the board in BOOTSEL if picotool cannot reboot it)..."
    if ! (cd "$REPO" && picotool load -u -v -x -t elf "$elf" >>"$logfile" 2>&1); then
        fail "$name (flashing failed, see $logfile)"
        return
    fi

    echo "Waiting for the USB serial port to come back after the reset..."
    if ! port="${PORT:-$(wait_for_port)}"; then
        skip "$name" "no serial port appeared - is the defmt USB task running in this binary?"
        return
    fi
    echo "Capturing $secs s of output from $port ..."

    local fifo
    fifo="$(mktemp -u)"; mkfifo "$fifo"
    # The USB CDC device can drop out mid-run, which used to kill capture and look like a
    # board hang. Reconnect until the capture window is over, and keep the raw bytes so a
    # run can be decoded again later.
    "$PYTHON" -u -c "
import os, serial, time
PORT = '$port'
deadline = time.time() + $secs + 5
GIVE_UP_AFTER = 20          # seconds with the port gone before we stop waiting
raw = open('${logfile%.log}.raw', 'wb', buffering=0)
gone_since = None
last_msg = 0
with open('$fifo', 'wb', buffering=0) as f:
    while time.time() < deadline:
        if not os.path.exists(PORT):
            gone_since = gone_since or time.time()
            waited = time.time() - gone_since
            if waited > GIVE_UP_AFTER:
                print('[reader] %s has been gone for %ds - the board left the USB bus '
                      '(unplugged, reset, or it crashed). Giving up.' % (PORT, waited), flush=True)
                break
            if time.time() - last_msg > 5:
                print('[reader] waiting for %s to come back...' % PORT, flush=True)
                last_msg = time.time()
            time.sleep(0.5)
            continue
        try:
            s = serial.Serial(PORT, 115200, dsrdtr=True, rtscts=False, timeout=0.2)
            s.dtr = True
            s.rts = False
            gone_since = None
            while time.time() < deadline:
                data = s.read(4096)
                if data:
                    f.write(data); f.flush()
                    raw.write(data)
        except Exception as e:
            if time.time() - last_msg > 5:
                print('[reader] serial dropped (%s), reconnecting' % e.__class__.__name__, flush=True)
                last_msg = time.time()
            time.sleep(0.5)
" 2>>"$logfile" &
    reader_pid=$!

    # <> rather than < so this never blocks waiting for a writer if the reader died
    timeout "$secs" defmt-print -e "$elf" <>"$fifo" 2>&1 | tee -a "$logfile"
    kill "$reader_pid" 2>/dev/null; wait "$reader_pid" 2>/dev/null
    rm -f "$fifo"

    # defmt lines can arrive out of order; sort by the timestamp they carry
    local sorted="${logfile%.log}.sorted.log"
    sort -s -g -k1,1 "$logfile" >"$sorted" 2>/dev/null || cp "$logfile" "$sorted"

    judge "$name" "$logfile"
    report "$sorted"
    echo
}

# Everything the board reported, in order, in one place
report() {
    local sorted="$1"
    echo
    bold "  Devices found"
    grep -E "device\(s\) answered|^[0-9.]+ (INFO|WARN) +0x[0-9A-F]{2}" "$sorted" \
        | sed -E 's/^[0-9.]+ (INFO|WARN) +/    /' | sed 's/^/  /' || true
    echo
    bold "  Readings"
    grep -E "lux|V, | V,|MB|epoch|advanced|LEDOUT|modified|bytes" "$sorted" \
        | sed -E 's/^[0-9.]+ (INFO|WARN|ERROR) +/    /' | sed 's/^/  /' || true
    echo
    bold "  Checks"
    grep -E "PASS|FAIL|SKIP" "$sorted" \
        | sed -E 's/^[0-9.]+ (INFO|WARN|ERROR) +/    /' | sed 's/^/  /' || true
    echo
    echo "  Full log (time-ordered): $sorted"
}

# Decide pass/fail from what the board printed.
judge() {
    local name="$1" logfile="$2"
    if [ ! -s "$logfile" ] || ! grep -qE "INFO|WARN|ERROR" "$logfile"; then
        fail "$name (no log output - is the defmt USB task running, and the right port used?)"
        return
    fi

    # flatsat_check prints its own verdict; without it the capture is incomplete
    if grep -q "FLAT SAT CHECKOUT" "$logfile"; then
        if grep -q "ALL CHECKS PASSED" "$logfile"; then
            green "  $(grep -o 'SUMMARY:.*' "$logfile" | tail -1)"
            pass "$name"
        elif grep -q "SUMMARY:" "$logfile"; then
            red "  $(grep -o 'SUMMARY:.*' "$logfile" | tail -1)"
            grep -o 'failed: .*' "$logfile" | sed 's/^/    /'
            fail "$name"
        else
            red "  The run never reached its summary - output was cut off or the board hung."
            red "  Only part of the checkout is in the log, so this is not a pass."
            fail "$name (incomplete)"
        fi
        return
    fi

    local errors
    errors="$(grep -c "ERROR" "$logfile")"
    if [ "$errors" -gt 0 ]; then
        red "  $errors ERROR line(s):"
        grep "ERROR" "$logfile" | head -5 | sed 's/^/    /'
        fail "$name"
    else
        pass "$name"
    fi
}

while [ $# -gt 0 ]; do
    case "$1" in
        --board)      RUN_BOARD=1
                      case "${2-}" in ""|--*) ;; *) BOARD_TEST="$2"; shift ;; esac ;;
        --no-board)   NO_BOARD=1 ;;
        --all)        RUN_ALL_SAFE=1 ;;
        --list)       list_tests; exit 0 ;;
        --duration)   DURATION="$2"; shift ;;
        --port)       PORT="$2"; shift ;;
        --out)        OUT_DIR="$2"; shift ;;
        --fire)       ALLOW_FIRE=1 ;;
        --force)      ALLOW_UNSAFE=1 ;;
        --skip-host)  SKIP_HOST=1 ;;
        -h|--help)    usage; exit 0 ;;
        *)            red "Unknown option: $1"; echo; usage; exit 2 ;;
    esac
    shift
done

OUT_DIR="${OUT_DIR:-$REPO/test-logs/$(date +%Y%m%d-%H%M%S)}"
mkdir -p "$OUT_DIR"

bold "Argus FSW test run - logs in $OUT_DIR"
echo

need_cmd cargo || exit 1

# A connected board is tested by default; --no-board opts out
if [ "$RUN_BOARD" -eq 0 ] && [ "$NO_BOARD" -eq 0 ] && board_present; then
    RUN_BOARD=1
    if p="$(find_port)"; then
        green "Board detected on $p - board tests will run (use --no-board to skip)"
    else
        green "Board detected in BOOTSEL mode - board tests will run (use --no-board to skip)"
    fi
    echo
fi

if [ "$SKIP_HOST" -eq 0 ]; then
    run_host_tests
    build_firmware
fi

if [ "$RUN_BOARD" -eq 1 ]; then
    to_run=()
    if [ "$RUN_ALL_SAFE" -eq 1 ]; then
        for entry in "${BOARD_TESTS[@]}"; do
            IFS=: read -r name _ category _ <<<"$entry"
            [ "$category" = "safe" ] && to_run+=("$name")
        done
    else
        to_run=("${BOARD_TEST:-flatsat_check}")
    fi

    # Catch a mistyped name before complaining about missing tools
    for name in "${to_run[@]}"; do
        if ! lookup "$name" 3 >/dev/null; then
            red "Not a known board test: $name"
            echo
            list_tests
            exit 2
        fi
    done

    missing=0
    need_cmd picotool "flashing" || missing=1
    need_cmd defmt-print "cargo install defmt-print" || missing=1
    [ "$missing" -eq 1 ] && exit 1

    if ! PYTHON="$(find_python)"; then
        red "No python with pyserial found. Install it with one of:"
        red "    pip install pyserial"
        red "    sudo apt install python3-serial"
        exit 1
    fi
    [ "$PYTHON" = "$(command -v python3)" ] || echo "Using $PYTHON (it has pyserial)"

    # ModemManager probes /dev/ttyACM* and can steal bytes from the board's log
    if systemctl is-active ModemManager >/dev/null 2>&1; then
        yellow "ModemManager is running. It probes USB serial devices and can corrupt or"
        yellow "interrupt the board's log. To stop it interfering with this board only:"
        yellow "    sudo cp $REPO/99-argus-no-modemmanager.rules /etc/udev/rules.d/ && sudo udevadm control --reload"
        yellow "Or just for now:  sudo systemctl stop ModemManager"
        echo
    fi

    PHASE="board"
    for name in "${to_run[@]}"; do
        category="$(lookup "$name" 3)"
        case "$category" in
            fire)
                if [ "$ALLOW_FIRE" -eq 0 ]; then
                    skip "$name" "fires a burn wire; pass --fire to allow it"
                    continue
                fi
                yellow "!! $name FIRES A BURN WIRE. Disconnect the wires unless you mean to."
                read -r -p "Type FIRE to continue: " confirm
                [ "$confirm" = "FIRE" ] || { skip "$name" "not confirmed"; continue; }
                ;;
            unsafe)
                if [ "$ALLOW_UNSAFE" -eq 0 ]; then
                    skip "$name" "wrong pins for Mainboard v4; pass --force to allow it"
                    continue
                fi
                yellow "!! $name uses pins that are wrong for Mainboard v4."
                ;;
        esac
        run_on_board "$name"
    done
fi

board_ran=$(( ${#BOARD_PASSED[@]} + ${#BOARD_FAILED[@]} ))
total_failed=$(( ${#FAILED[@]} + ${#BOARD_FAILED[@]} ))

rule
bold "SUMMARY"
if [ "$SKIP_HOST" -eq 1 ]; then
    echo   "  Laptop checks:  skipped (--skip-host)"
else
    echo   "  Laptop checks:  ${#PASSED[@]} passed, ${#FAILED[@]} failed  (compile + simulated hardware only)"
fi

if [ "$RUN_BOARD" -eq 0 ]; then
    yellow "  BOARD CHECKS:   NOT RUN - nothing touched real hardware"
    if [ "$NO_BOARD" -eq 1 ]; then
        yellow "                  (--no-board was given)"
    else
        yellow "                  No board detected. Plug it in, or hold BOOTSEL and tap RESET,"
        yellow "                  then run again. Force with: ./test_all.sh --board"
    fi
elif [ "$board_ran" -eq 0 ]; then
    red    "  BOARD CHECKS:   NONE RAN - no board test produced any output"
    red    "                  The board was not detected, so nothing was verified on hardware"
else
    echo   "  Board checks:   ${#BOARD_PASSED[@]} passed, ${#BOARD_FAILED[@]} failed, ${#BOARD_SKIPPED[@]} skipped  (on the real board)"
fi

for n in "${FAILED[@]}";        do red    "    failed:  $n"; done
for n in "${BOARD_FAILED[@]}";  do red    "    failed:  $n (board)"; done
for n in "${SKIPPED[@]}";       do yellow "    skipped: $n"; done
for n in "${BOARD_SKIPPED[@]}"; do yellow "    skipped: $n (board)"; done
echo "  Logs: $OUT_DIR"
rule

if [ "$total_failed" -gt 0 ]; then
    red "FAILED"
    exit 1
fi
if [ "$RUN_BOARD" -eq 0 ]; then
    yellow "Laptop checks passed. THE BOARD WAS NOT TESTED."
    exit 0
fi
if [ "$board_ran" -eq 0 ]; then
    red "Nothing ran on the board - not a pass."
    exit 1
fi
green "ALL PASSED, including $board_ran check(s) on the board"
