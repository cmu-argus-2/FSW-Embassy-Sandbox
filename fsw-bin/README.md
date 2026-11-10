## Building and Flashing a Program on the RP2350
```
cargo run --bin test_logging
```
replace `test_logging` with the filename you want to run.

## Logging

logging is done through the log.sh script. Usage example:
```
./log.sh ../target/thumbv8m.main-none-eabihf/debug/test_logging
```
Replace `test_drivers` with the binary you're running. A binary name resolves to the workspace's `target/thumbv8m.main-none-eabihf/debug/` directory relative to the script, regardless of your working directory. Explicit ELF paths are also supported, including release builds.

**important notes for logging**

Make sure you set up the defmt usb task. Examples are shown in `test_logging.rs` and `scan_i2c.rs`.

Run the script from `fsw-bin` with the ELF matching the flashed firmware. It requires `python3` with `pyserial` and `defmt-print`. If multiple devices are attached, supply the serial port as a second argument.

The USB backend normally waits for its buffer to fill. `test_drivers.rs` calls `defmt::flush()` every 250 ms from a concurrent loop so sparse logs can be sent without filler messages. With this backend, flushing schedules bytes for the USB task; it does not wait for the host. The backend also polls every 100 ms, so expect roughly subsecond output with a connected reader, rather than guaranteed real-time delivery. Other binaries need the same flush loop to benefit.

## RAM usage

RAM in use is static RAM plus the stack's high-water mark (there is no heap).

Static RAM is read from the ELF, no board needed:
```
./mem.sh eps
```
It prints the `.data`/`.bss` totals, each embassy task's storage (its `POOL`) and the largest statics. A binary name resolves to the release build.

The stack peak is only known at runtime. A binary that calls `fsw_board::stack::paint()` first thing in `main` can report `fsw_board::stack::peak()`; `eps` logs `BENCH` rows for both every 10 s:
```
cargo run --release --bin eps
./log.sh ../target/thumbv8m.main-none-eabihf/release/eps | grep BENCH
```
The peak only grows, so let it run through a few fuel gauge reads (every 5 s) and take the last report.

## Examples

Reference [embassy.dev](https://embassy.dev/) for documentation and examples.
