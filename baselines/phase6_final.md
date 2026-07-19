# Phase 6 Final Audit

Captured on 2026-07-19 against the `wide-log` workspace at the end of
the `0.6.0` implementation (after Phase 6, before the release).

## Comparison to Phase 0 baseline (`baselines/phase0_audit.md`)

| Metric | Phase 0 (2026-07-19) | Phase 6 (2026-07-19) | Delta |
|--------|----------------------|----------------------|-------|
| `cargo` | 1.96.0 | 1.96.0 | — |
| `rustc` | 1.96.0 | 1.96.0 | — |
| `cargo-audit` | 0.21.x | 0.21.x | — |
| `cargo-deny` | 0.20.2 | 0.20.2 | — |
| Total crates in `Cargo.lock` | 160 | 163 | +3 |
| Known vulnerabilities | 0 | 0 | — |
| `cargo deny` advisories | ok | ok | — |
| `cargo deny` bans | ok | ok | — |
| `cargo deny` licenses | ok | ok | — |
| `cargo deny` sources | ok | ok | — |
| Duplicate crate warnings | 2 (`getrandom`, `r-efi`) | 2 (`getrandom`, `r-efi`) | — |

## Crate additions for Phase 6

The 3 new crates are dev-deps only and are NOT published with
`wide-log 0.6.0`. They are not in the published crate's dep graph.

- `loom 0.7.2` — exhaustive multi-threaded model checker for
  `ContextCell` invariants. Pulled in only when
  `RUSTFLAGS="--cfg loom"` is set, via the
  `[target.'cfg(loom)'.dev-dependencies]` table.
- `generator 0.8.9` — transitive of `loom`.
- `scoped-tls 1.0.1` — transitive of `loom`.

## `cargo audit` output

```
$ cargo audit
    Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 1166 security advisories
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (163 crate dependencies)
$ echo $?
0
```

**Result**: 0 vulnerabilities, 0 warnings. **Clean.**

## `cargo deny check` output

```
$ cargo deny check
advisories ok, bans ok, licenses ok, sources ok
$ echo $?
0
```

**Result**: All four sections (`advisories`, `bans`, `licenses`,
`sources`) pass with no warnings. Two transitive dep duplicates
(`getrandom 0.3.4` via `ahash 0.8` → `sonic-rs 0.5.8`; `getrandom
0.4.3` via `rand 0.10` → `ulid 3.0`; and the corresponding
`r-efi 5` / `r-efi 6`) are listed in `.cargo/deny.toml` under
`[bans].skip = ["getrandom", "r-efi"]` with a comment explaining
that unifying them would require changing either the pinned
`sonic-rs = "=0.5.8"` or the `ulid` dep, both out of scope for
0.6.0. The `cargo-deny` exit status is 0.

## Deltas from Phase 0

- **3 new dev-deps** (loom + 2 transitive), all gated on `--cfg loom`
  so they don't affect normal builds.
- **0 new vulnerabilities**.
- **0 new license concerns** (loom is MIT, same as the rest).
- **No new transitive dedup issues** — the same 2 pre-existing
  duplicate warnings remain.

## Conclusion

The Phase 0 baseline was clean (0 vulnerabilities, 0 license issues)
and the Phase 6 final state is also clean. No new advisories, no
new denials, no deltas that would block the `0.6.0` release.
