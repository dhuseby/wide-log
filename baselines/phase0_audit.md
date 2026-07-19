# Phase 0 Audit Baseline

Captured on 2026-07-19 against the `wide-log` workspace at the start of the
`0.6.0` implementation (after Phase 0 setup, before any Phase 1+ changes).

## MSRV note

`rust-version` is set to `1.88` (was originally `1.85`, bumped because the
pre-existing code uses `if let ... && let ...` chains which were stabilized
in Rust 1.88.0, not 1.85.0).

## Tools

| Tool         | Version    |
|--------------|------------|
| `cargo`      | 1.96.0     |
| `rustc`      | 1.96.0     |
| `cargo-audit`| 0.21.x     |
| `cargo-deny` | 0.20.2     |

## `cargo audit`

```
$ cargo audit
    Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 1166 security advisories
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (160 crate dependencies)
$ echo $?
0
```

**Result**: 0 vulnerabilities, 0 warnings. **Clean.**

## `cargo deny check`

```
$ cargo deny check
warning[duplicate]: found 2 duplicate entries for crate 'getrandom'
   ┌─ Cargo.lock:36:1
   ├ getrandom v0.3.4 (via ahash -> sonic-rs)
   ├ getrandom v0.4.3 (via rand -> ulid)

warning[duplicate]: found 2 duplicate entries for crate 'r-efi'
   ┌─ Cargo.lock:73:1
   ├ r-efi v5.3.0 (via getrandom 0.3 -> ahash)
   ├ r-efi v6.0.0 (via getrandom 0.4 -> rand)

advisories ok, bans ok, licenses ok, sources ok
```

**Result**: All four sections (`advisories`, `bans`, `licenses`, `sources`)
pass. The two `duplicate` warnings are pre-existing transitive dep
deduplication issues and are not blocking.

## Dependency summary

- **160 crates** total in the lockfile (direct + transitive).
- 0 known vulnerabilities at the time of this baseline.
- 2 transitive duplicate crates (`getrandom` 0.3 vs 0.4, `r-efi` 5 vs 6)
  — pulled in by `ahash` and `rand` respectively. Not a blocker; will
  revisit if a version bump resolves them.

## Pre-existing advisories

None. The baseline is clean.

## Phase 6 comparison

A final `cargo audit` and `cargo deny check` will be run at the end of
Phase 6 (just before the `0.6.0` release). Any new advisories or new
deny failures will be documented in `baselines/phase6_final.md` and
addressed before release.
