# [junco-policy](https://github.com/junco-org/junco-policy) experiments

| Folder | Takeaway |
| --- | --- |
| [01-batch-hog](01-batch-hog/) | **Distinct batch events recompile the same rules, reaching ~415 MiB RSS.** |
| [02-intern-decide-hog](02-intern-decide-hog/) | **Denied candidates drive RSS to ~39 MiB without any history appends.** |
| [03-cursor-thrash](03-cursor-thrash/) | **One extra resource ID makes decisions ~4,000x slower.** |
| [04-match-hog](04-match-hog/) | **A rule compiles, then takes ~31 ms per decision to return an error.** |
| [05-compile-hog](05-compile-hog/) | **A single policy takes ~6 seconds to compile**, despite per-rule graph budgets. |
| [06-successor-hog](06-successor-hog/) | **257 event classes make the same policy ~5x slower than 256.** |
| [07-slow-policy](07-slow-policy/) | **One extra caller makes 32 rules ~2,070x slower.** |
| [08-state-cache-hog](08-state-cache-hog/) | **Redundant rule syntax drives process RSS to ~115 MiB despite graph budgets.** |

## Dev Environment

Linux 7.0.0-43-generic, x86_64; AMD Ryzen 9 8945HX, 16 logical CPUs, 23 GiB RAM.<br>
Rust/Cargo 1.98.1; local `junco-policy/` checkout at `11d73da` (clean).

Run the command blocks from the repository root, with rustup installed and the
local `junco-policy/` checkout present.

### 01-batch-hog

Audit mode denies and appends all events. RSS is sampled inside the final
observer while the compiled judges are still alive.

```sh
(cd 01-batch-hog && cargo run --release)
```

```text
batch_len=1000; audit mode, all decisions denied and appended
rules    distinct   check_ms  RSS before KiB  RSS during KiB
    1           1       0.35            2536            2872
    1        1000      21.78            2776           39248
    4           1       0.37           38900           39164
    4        1000      76.67           25776          159488
    8           1       0.30          159364          159444
    8        1000     148.17          159444          424560
```

Captured with an 8 GiB virtual-memory ceiling and a 60 CPU-second limit.
A 2 GiB virtual ceiling caused allocation errors despite much lower RSS.

### 02-intern-decide-hog

Principal and resource IDs are each 256 bytes; every row runs in a fresh process.
The final RSS sample is taken after dropping the policy and history, with the
store still alive.

```sh
(cd 02-intern-decide-hog && cargo run --release)
```

```text
empty history; 256-byte principal and resource IDs; fresh process per row
decisions rules verdict unique    loop_ms before_MiB  after_MiB store_live_MiB
    50000     1   grant  false      27.26       2.40       2.47           2.47
    50000     1   grant   true      78.91       2.35      39.41          39.41
    50000     1    deny  false      27.64       2.39       2.46           2.47
    50000     1    deny   true      83.46       2.34      39.40          39.40
    50000    32   grant  false     809.09       3.42       3.49           2.67
    50000    32   grant   true     917.57       3.38      40.48          40.48
    50000    32    deny  false      27.74       3.40       3.47           2.65
    50000    32    deny   true      80.77       3.37      40.48          40.48
```

### 03-cursor-thrash

**16 resource IDs: cached. 17: full-history scans.** The rule doesn't depend on
the candidate's resource ID, yet changing it defeats cached progress.

```sh
(cd 03-cursor-thrash && cargo run --release)
```

```text
cursor cache: 16 entries keyed by (history, candidate bindings), evicts its last entry
per-decision cost: 16 candidate values fit the cache, 17 do not
per arm: 272 unmeasured warmup decisions, 1088 measured decisions
all timings are ns/decision medians; halves contain complete candidate cycles

L = 1000 initial events, equivalent fresh append-only histories, one rule
  values    median ns   first half    last half  role
       1          111          111          111  baseline
      16          331          331          331  fits
      17        23083        19767        26189  thrash
  thrash / fits           70x
  thrash / baseline      208x

L = 5000 initial events, equivalent fresh append-only histories, one rule
  values    median ns   first half    last half  role
       1          120          111          120  baseline
      16          321          321          321  fits
      17        74279        70632        76683  thrash
  thrash / fits          231x
  thrash / baseline      619x

L = 20000 initial events, equivalent fresh append-only histories, one rule
  values    median ns   first half    last half  role
       1          111          111          111  baseline
      16          340          341          331  fits
      17       273441       265977       278480  thrash
  thrash / fits          804x
  thrash / baseline     2463x

L = 100000 initial events, equivalent fresh append-only histories, one rule
  values    median ns   first half    last half  role
       1          111          110          111  baseline
      16          340          341          331  fits
      17      1324615      1312703      1331949  thrash
  thrash / fits         3896x
  thrash / baseline    11933x
```

Headline case only: `cargo run --release -- 100000` from `03-cursor-thrash/`.
At that length, **340 ns becomes ~1.32 ms per decision**: ~4,000x versus
16 IDs, or ~12,000x versus the single-ID baseline.

### 04-match-hog

**The tail accepts only empty history.** The simple epsilon
form grants; the wide form compiles but repeatedly fails with a graph-budget error.

```sh
(cd 04-match-hog && cargo run --release)
```

```text
rule: Forbid p,Edit,r   tail = INTERSECT_{i<k} STAR(resource == res_i)
all tested tails are equivalent to epsilon (empty history only)
history: 5000 events   decisions: 10, same matching candidate each time
expected verdict: Grant; first/repeat timings exclude outcome checks

  tail/k compile ms   grants   errors    first us   repeat us
 epsilon       0.03       10        0        1.60        0.13
     256       1.56       10        0        5.10        0.12
     512       4.91        0       10     5024.96     4683.98
         first error: GraphBudget { limit: 102400 }
    1024      31.22        0       10    31237.62    31202.70
         first error: GraphBudget { limit: 102400 }
```

### 05-compile-hog

```sh
(cd 05-compile-hog && cargo run --release)
```

```text
rule: Forbid p,Edit,r   tail = INTERSECT_{i<k} STAR(resource == res_i)

     k   compile outcome
    32       0ms Ok
    64       0ms Ok
   128       0ms Ok
   256       1ms Ok
   384       2ms Ok
   512       4ms Ok
   640      12ms Ok
   768      17ms Ok
   896      23ms Ok
  1024      31ms Ok
  1152      38ms Ok
  1280         - Runtime(GraphBudget

largest swept k that compiles: 1152

 rules    compile   ms/rule  RSS before  RSS after
     1      38ms      38.0         3 MiB        3 MiB
    20     786ms      39.3         3 MiB       19 MiB
    60    2369ms      39.5        19 MiB       58 MiB
   150    5941ms      39.6        58 MiB      144 MiB
```

### 06-successor-hog

The union width stays fixed at 512 tests. Fresh decision timings exclude
construction; warm timings reuse a cursor.

```sh
(cd 06-successor-hog && cargo run --release)
```

```text
history_len=10000 decisions=3 successor_capacity=256
rules     k classes missing compile_ms   warm_ns     fresh_us  fresh/warm
    1   512     128   false      18.38       201     17136.68       85257x
    1   512     256   false      18.32       230     17975.81       78156x
    1   512     257   false      18.30       210     89008.94      423852x
    1   512     512   false      18.14       200     89139.54      445698x
    4   512     512    true      98.58       391    391118.07     1000302x
```

### 07-slow-policy

Only caller identity varies; the tails use constants and history stays fixed.
Each arm uses a fresh policy with 272 excluded warmup decisions.

```sh
(cd 07-slow-policy && cargo run --release)
```

```text
history_len=20000
callers: 1 / 16 / 17; warmup=272, measured=272 per arm
rules   hits_us   fits_us   thrash_us   thrash/fits  ns per rule-event
    1       0.1       0.1       231.6        1768x             11.58
    4       0.4       0.5       949.1        2059x             11.86
    8       0.7       1.0      1888.2        1943x             11.80
   16       1.5       1.9      3738.8        1995x             11.68
   32       2.9       3.6      7528.9        2070x             11.76
thrash/fits = 2070x at 32 rules over 20000 events   (wall 8.1 s)
```

### 08-state-cache-hog

Repeated identical stars accept the same language as one star. Input and compiled
RSS are process endpoints, not isolated cache allocations. The graph budget
covers charged payload, not process RSS.

```sh
(cd 08-state-cache-hog && cargo run --release)
```

```text
atoms=2; repeated stars are equivalent to one star
 rules  repeats   build_ms compile_ms before_MiB  input_MiB compiled_MiB
     1        1       0.00       0.05       2.30       2.38         2.43
     1    20000       2.14      25.20       2.43       6.66        13.55
    16    20000      34.82     336.65       9.89      70.76       115.11
```

Previously captured output from this environment; timings and memory usage vary
between runs.
