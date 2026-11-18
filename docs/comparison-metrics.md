# Port comparison: method

Three ports of the Argus flight software — Rust/Embassy, C/Zephyr, F Prime — compared on
performance, efficiency, stability, maintainability and correctness.

This file is the agreement between the three teams. It is deliberately strict about method,
because the obvious way to run this comparison produces a number nobody can defend.

**Status of the Rust port's harness:** `Stats` (count, min, max, mean, histogram percentiles)
is merged. CSV rows and `Stats::report` come next, then `ExecTimer`, `measure_overhead` and
`schedule_step`. Sections below that name those describe the agreed interface, not code that
exists yet.

## What this comparison can and cannot claim

Three ports means three programs, written by three different people, with three toolchains.
Any difference we measure is the combined effect of language, framework, toolchain and
author. Those cannot be separated by this design.

So the supportable claim has this shape:

> Under workload W, on board v4, with configuration C, port X had p99 start lateness of
> N µs against port Y's M µs, across R runs with a spread of S. The mechanism appears to
> be Z.

And this one is not supportable, however the bars look:

> Rust is faster than Zephyr.

This is not pedantry. *It's Not Easy Being Green* (ASE 2025) exists because the widely cited
27-language energy ranking confused languages with their implementations and let benchmark
code drive the result; controlling for those, the authors found language implementation has
no significant energy effect beyond execution time. A three-port comparison with
unacknowledged confounds repeats that mistake exactly.

## 1. One workload, implemented three times

The unit of comparison is a **workload spec**, not "whatever each port happens to run".

Precedent: *Towards Understanding the Runtime Performance of Rust* hand-wrote Rust/C pairs
and manually validated that each pair implements identical functionality using the same
algorithm. Biscuit (OSDI '18) compared nearly identical syscall, page-fault and context-switch
paths. The GPU portability study (ICS '25) kept code structure as close as possible across
seven ports and left kernel parameters untuned, reporting out-of-the-box behaviour.

**Workload EPS-1** (first milestone):

| Property | Value |
|---|---|
| Rate | 1 Hz (matches `TASK_CONFIG` in the Python flight software) |
| Per iteration | Read the four rail power monitors (board, GPS, radio, Jetson: V, I) and check each against its power limit; every 5th iteration also read the fuel gauge and update the power flag |
| I²C traffic | Fixed register reads, same addresses, same order, same bus speed |
| State machine | SOC hysteresis: LOW_POWER at or below 30%, back to NOMINAL at or above 40% |
| Not yet included | Battery heater decision. The Python task has it; add it to EPS-1 once every port implements it, not before, or the ports are not doing the same work |
| Duration | 10 minutes per run |
| Inputs | Replayed from a fixed trace so every port sees identical sensor values |

Real bus traffic and replayed inputs only fit together if the replay hooks in at one agreed
place: **between the driver and the task**. Each driver performs its real I²C transaction, so
the bus timing being measured is genuine; the value it returns to the task is then replaced
by the next trace entry. Every port injects at that point and nowhere else. Replaying lower
down, at the register map, would skip the bus; replaying higher up, inside the state machine,
would skip the driver.

Before any measurement counts, one person who did not write either port reads all three
implementations side by side and confirms they do the same work in the same order. Record who
checked and when.

## 2. Pin everything that is not the variable

The GPU study pinned compilers, dependency versions and flags in one environment file per
system, used identical flags across applications, and excluded warmup from timing. Do the same:

Each port commits a `comparison-config.md` recording:

- Toolchain and exact versions (rustc/cargo, Zephyr SDK, F′ toolchain)
- Optimisation level, and the argument that it is equivalent across ports
- CPU clock and timer tick rate (this port: 1 MHz, `tick-hz-1_000_000`)
- I²C bus speed, SPI speed
- What else runs during the measurement (other tasks, logging, USB)
- Whether logging is on — if it is, measure with it off too, since logging perturbs timing

Anything not pinned here is a confound, and a reviewer will find it.

## 3. Scenarios: a task alone on the CPU proves nothing

Measure each port under three loads. Ports diverge under pressure, not at idle.

| Scenario | Setup |
|---|---|
| `idle` | EPS-1 alone. Establishes the floor. |
| `nominal` | EPS-1 plus the other planned tasks at their real rates (ADCS 1 Hz, COMMAND 2 Hz, watchdog 1 Hz) |
| `overload` | Nominal plus the interference task below, to expose scheduling behaviour under contention |

The `overload` scenario is where an async executor and an RTOS should actually differ, and
where priority inversion or a blocking call shows up.

Because of that, the interference task decides the result unless it is pinned exactly. A
task that computes without yielding stalls a cooperative executor completely and is simply
preempted by an RTOS, so a loosely specified one picks the winner before anything is
measured. Every port implements this one:

| Property | Value |
|---|---|
| Period | 100 ms (10 Hz) |
| Work per iteration | 150 ms of busy computation, so it always overruns |
| Yield points | Yields to the scheduler after every 1 ms of work (`.await` on Embassy, `k_yield()` on Zephyr, the rate-group equivalent on F′) |
| Priority | Below every flight task, on ports that have priorities |
| On overrun | Starts the next iteration immediately; never skips |

A variant with **no yield points** is worth running too, but report it as its own scenario,
`overload-noyield`. It does not measure contention; it measures whether the scheduler can
preempt at all. On Embassy that depends on the executor layout, not the language: a task on
the thread-mode executor cannot be preempted by its neighbours, but a task on an interrupt
executor (the IMU service in the `adcs` binary runs on one) preempts thread mode. Record
which executor every task runs on in `comparison-config.md`, because it changes this result.

## 4. Run protocol

- **At least 5 runs** per (port, scenario), each a fresh boot.
- Discard the first 30 s of each run as warmup; the GPU study excluded warmup explicitly.
- Report **median across runs**, and the **spread** (min–max or IQR) as its own row.
- **If the spread overlaps the difference between ports, there is no result.** Say so. The GPU
  study could claim small differences only because it reported run-to-run variability of 0.1%
  typical, 3.3% worst.

## 5. Metrics

Ordered by how much they discriminate between a cooperative async executor and a preemptive
RTOS. Flash size is last on purpose: it is the easiest to measure and the least interesting.

| Metric | Unit | Why it discriminates |
|---|---|---|
| `start_lateness` (p50/p99/max, full histogram) | µs | Scheduler delay and contention. The headline number. |
| `missed_periods`, `late_iterations` | count | Behaviour under overload: skipping versus drifting |
| `irq_to_task` | µs | Interrupt to handling task. Directly exposes executor versus RTOS wake paths |
| `wake_latency` | µs | Signal/semaphore to task resumption |
| `exec_time` (p50/p99/max) | µs | Work itself; mostly a control, since all three do the same I²C traffic |
| `cpu_headroom` | % | Idle time at nominal load. Determines how much the board can still take on |
| `task_ram` per task | bytes | Everything reserved to hold one task's state: its stack high-water mark plus any static task storage. An RTOS reserves a stack per thread; Embassy keeps each task's future in static memory and shares one stack. Counting stacks alone would leave Embassy's futures out, so both are counted. Expect a real difference |
| `ram_static` | bytes | data + bss for the same workload |
| `flash` | bytes | text + data for the same workload |
| `power` | mW | External meter on the board's supply, same scenario. The board's own monitor is part of the workload, so reading power from it measures a system that is busy measuring itself; use it only if no meter is available, and say so in the results |

## 6. Attribution is required, not optional

A number without a mechanism is not a finding.

Biscuit did not stop at "Go is 5–15% slower": it attributed the cost to garbage collection and
stack-expansion checks (up to 13% of kernel CPU time) and reported the GC pause tail
(115 µs worst single pause, 600 µs summed for one request). Zhang et al. went further and
*ablated*: disabling runtime checks made Rust indistinguishable from C, proving the mechanism
rather than asserting it.

For every difference we report, name the mechanism and, where possible, ablate it:

- Rust jitter → which `.await` point, which executor wake path; try the same loop with the
  logging task disabled
- Zephyr jitter → thread priority, tick granularity, preemption settings
- F′ jitter → rate group scheduling, port call overhead

If we cannot name a mechanism, the finding is "we observed a difference we cannot explain",
which is honest and still useful.

## 7. Composite score, defined before the data

Defined now so nobody argues about weighting after seeing results. Following the GPU study's
use of Pennycook's performance-portability metric:

For each metric m and port p, **efficiency** is `best_observed(m) / value(p, m)` for
lower-is-better metrics (inverted for higher-is-better), where `best_observed` is the best
value any port achieved on that metric. A port that cannot run a scenario scores 0 for it.

Count metrics (`missed_periods`, `late_iterations`) use `(1 + best) / (1 + value)` instead.
The best port will often have a count of 0, and the plain ratio would then give every other
port 0, or divide by zero. With the offset, a port with no misses scores 1, a port with 1
miss against a best of 0 scores 0.5, and the harmonic mean still falls hard on a port that
misses often.

The port's score for a category is the **harmonic mean** of its efficiencies within that
category. Harmonic mean punishes a single bad result, which is the behaviour we want from
flight software: a port that is excellent on four metrics and fails one is not a good port.

Report per-category scores. Do not combine them into one grand number: performance and
maintainability are not commensurable.

## 8. Correctness gate

No performance number counts until the port passes correctness. Every port must produce the
same outputs as the Python flight software for the same inputs — the method already used for
drivers here: run the CircuitPython implementation against a simulated register map, record
its register writes, and require the port to match. The same replay trace that drives EPS-1
feeds this.

## Output format

Every measurement, from every port, is one CSV row:

```
port,scenario,run,category,metric,subject,stat,value,unit,n,build,commit
```

| Field | Meaning |
|---|---|
| `port` | `rust-embassy`, `c-zephyr`, `fprime` |
| `scenario` | `idle`, `nominal`, `overload` |
| `run` | run index, 1-based; `agg` for a cross-run statistic |
| `category` | `performance`, `efficiency`, `stability`, `correctness` |
| `metric` | from the table in §5 |
| `subject` | `eps_task`, `adcs_task`, … |
| `stat` | `min`, `median`, `p50`, `p99`, `max`, `spread`, `total`, `resolution` |
| `value` | integer |
| `unit` | `us`, `bytes`, `count`, `mW`, `pct` |
| `n` | samples behind the value |
| `build` | `release` or `debug` |
| `commit` | short commit, `-dirty` if the tree was not clean |

Firmware prints these over the log prefixed `BENCH,`; the size script writes them to stdout.
Collect with `grep '^BENCH,' run.log | cut -d, -f2-`.

### Which figures are exact

`count`, `min`, `max` and `mean` are exact. Percentiles come from a 64-bucket histogram and are
reported as the bucket's upper bound, clamped so they never exceed the largest sample. Every
`Stats::report` emits a `resolution` row giving the bucket width, so a reader can see the error
bar on `p50` and `p99` without asking.

Samples past the last bucket are counted, and a percentile that lands among them reports the
exact `max`. That keeps the worst case visible, but if many samples overflow, even `p50` reads
as the maximum. Choose the bucket width per metric so normal values stay in range: 64 buckets
of width `w` cover `0..64w`, so 50 µs buckets suit `start_lateness` up to 3.2 ms. If `p50`
equals `max` in a result, the bucket width was wrong for that run; fix it and rerun rather
than report it.

### Measuring the measurement

`bench::measure_overhead(n)` returns the cost of one `ExecTimer` start/elapsed pair. A task body
shorter than that cannot be measured meaningfully. Report it once per run; each port reports its
own equivalent. Timebase for this port is 1 MHz, verified with:

```bash
cargo tree -p fsw-bin -i embassy-time-driver --target thumbv8m.main-none-eabihf -e features
```

A 32 kHz tick would quantise every measurement to 31 µs.

## Maintainability

No number, and no pretence of one. A written assessment from the team against fixed anchors:
lines of code for the same workload, how much is boilerplate, what adding a new sensor takes,
the build-and-flash loop, and what class of mistake the compiler catches. Collected from all
three teams using the same questions.

## Running it

```bash
./scripts/measure_size.sh                  # efficiency rows for every binary
./scripts/measure_size.sh eps_task         # one binary
```

Timing rows come from the firmware; see `fsw-lib/src/bench.rs`. The host-side logic (`Stats`,
`schedule_step`) is unit tested: `cd fsw-test && cargo test --test bench`.

## References

Method borrowed from, in order of influence:

- Cutler, Kaashoek, Morris. *The Benefits and Costs of Writing a POSIX Kernel in a High-Level
  Language*. OSDI '18 — matched code paths, attribution to specific language features.
- van Kempen, Berger et al. *It's Not Easy Being Green: On the Energy Efficiency of Programming
  Languages*. ASE 2025 — what goes wrong when confounds are not controlled.
- Davis, Bhatele et al. *Taking GPU Programming Models to Task for Performance Portability*.
  ICS '25 — pinned environments, warmup exclusion, runs and reported variability, a composite
  metric defined up front.
- Zhang, Zhang, Portokalidis, Xu. *Towards Understanding the Runtime Performance of Rust* —
  hand-validated equivalent implementations, ablation to prove mechanism.
