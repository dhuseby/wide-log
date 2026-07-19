# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.6.x   | :white_check_mark: |
| < 0.6.0 | :x:                |

`wide-log` is a pre-1.0 crate. As a matter of policy, only the most
recent minor version receives security fixes. Users are encouraged
to track `0.6.x` once it ships.

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for suspected security
vulnerabilities.

Report privately via one of the following channels:

- **GitHub Security Advisories**: use the
  [private vulnerability reporting](https://github.com/dhuseby/wide-log/security/advisories/new)
  feature on the repository.
- **Email**: `dwg@linuxprogrammer.org` (PGP key on request).

Please include:

1. A description of the vulnerability and its impact.
2. A minimal reproducer (code, configuration, or steps).
3. The commit/tag/SHA and version affected.
4. Whether the issue is currently being exploited in the wild.

## Response Timeline

- **Acknowledgement**: within 7 days of the report.
- **Triage & impact assessment**: within 14 days.
- **Patch & disclosure**: coordinated with the reporter, generally
  within 30–90 days depending on complexity and exploitability.

We follow a coordinated disclosure model: we will not publicly
disclose the issue until a fix is available, unless the reporter
explicitly requests earlier disclosure.

## Scope

In-scope issues include, but are not limited to:

- Memory unsafety in `unsafe` code (raw pointer manipulation,
  manual `Drop` impls, `from_utf8_unchecked`, etc.).
- Panics reachable from untrusted input that could lead to a
  denial-of-service (e.g. in the writer thread, in proc-macro
  output, in JSON serialization).
- Information leaks (e.g. buffer over-reads, uninitialized memory
  exposure).
- Supply-chain compromises of dependencies that the crate pulls in
  at build time or runtime.
- Proc-macro panics or UB affecting the build of downstream crates.

Out-of-scope:

- Issues in the `tokio`, `tracing`, `chrono`, `ulid`, `sonic-rs`,
  `faststr`, `smallvec`, or other upstream dependencies. Please
  report those to the upstream maintainers.
- Resource exhaustion caused by intentionally malicious logging
  callers (e.g. infinite loops, memory blow-up from huge strings)
  is best-effort but not a guaranteed invariant.
- Compiler/toolchain bugs.

## Recognition

We are happy to credit reporters in the fix's commit message and
in the `CHANGELOG.md` entry, unless the reporter prefers to remain
anonymous.
