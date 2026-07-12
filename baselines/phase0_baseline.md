# Phase 0 Baseline Measurements

Baseline performance of wide-log v0.2.0 prior to optimization.

Captured with:
- `cargo bench --bench core -- --warm-up-time 1 --measurement-time 3 --sample-size 50`
- Rust 1.96.0, release profile
- criterion 0.8.2

All times are median (point estimate) from the criterion output.

## guard_create_drop

| Benchmark         | Time       |
|-------------------|------------|
| noop_emit         | 273.77 ns  |
| capture_emit      | 448.09 ns  |

## wl_set_single

| Benchmark | Time       |
|-----------|------------|
| string    | 304.02 ns  |
| u64       | 303.33 ns  |
| bool      | 300.64 ns  |

## wl_set_nested

| Benchmark              | Time       |
|------------------------|------------|
| two_segment            | 304.29 ns  |
| two_segment_http       | 364.41 ns  |
| update_existing_nested | 312.34 ns  |

## wl_inc_dec

| Benchmark    | Time       |
|--------------|------------|
| inc_absent   | 293.44 ns  |
| inc_existing | 312.86 ns  |
| dec_absent   | 298.25 ns  |
| add_n        | 306.80 ns  |

## log_macros

| Benchmark     | Time       |
|---------------|------------|
| info_literal  | 296.82 ns  |
| info_format   | 306.63 ns  |
| info_multiple | 332.60 ns  |

## to_json

| Benchmark | Time       |
|-----------|------------|
| small     | 233.65 ns  |
| medium    | 642.39 ns  |
| large     | 1.3213 µs  |

## full_lifecycle

| Benchmark                 | Time       | Throughput      |
|---------------------------|------------|-----------------|
| noop_emit                 | 456.26 ns  | —               |
| capture_emit              | 883.74 ns  | —               |
| capture_emit_throughput   | 899.95 ns  | 1.1112 Melem/s  |

## current_access

| Benchmark      | Time       |
|----------------|------------|
| with_guard     | 295.80 ns  |
| without_guard  | 264.30 ps  |

## wl_set_repeat

| Benchmark               | Time       |
|-------------------------|------------|
| distinct_keys/1         | 286.79 ns  |
| distinct_keys/5         | 429.51 ns  |
| distinct_keys/10        | 469.08 ns  |
| distinct_keys/20        | 520.37 ns  |
| same_key_update/1       | 305.38 ns  |
| same_key_update/5       | 318.01 ns  |
| same_key_update/10      | 345.84 ns  |
| same_key_update/20      | 404.97 ns  |

## Key observations for optimization phases

1. **Guard overhead dominates**: Every wl_set!/inc!/info! call is ~300 ns, of which
   ~273 ns is just guard create+drop overhead. The actual operation (add/inc/append) is
   only ~30 ns on top of guard overhead. This is because `iter()` includes guard
   create+drop in every iteration.

2. **Linear scan visible in wl_set_repeat**: distinct_keys goes from 287 ns (1 key) to
   520 ns (20 keys) — a 233 ns increase for 19 additional linear scans. This confirms
   the O(n) scan cost that Phase 2 (indexed storage) will eliminate.

3. **Serialization cost scales**: to_json goes from 234 ns (2 fields) to 1.32 µs
   (9 fields + 20 log entries). Phase 4 (direct serializer) targets this path.

4. **Full lifecycle**: 456 ns (noop) vs 884 ns (capture) — serialization adds ~428 ns
   on top of accumulation for a moderate event.

5. **current() without guard**: 264 ps — the TLS access itself is sub-nanosecond when
   the pointer is null. With guard, the 296 ns is dominated by guard create/drop, not
   current() access. Phase 6 optimizations will have modest impact.