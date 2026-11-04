# VERIFICATION — v0.1.0 Release Closeout

State: GATES RUN (fmt/clippy/test PASS; audit FAIL with dispositions)

Baseline gap report: `~/.agents/outputs/db-pro/reports/2026-11-04-release-checklist-v0.1.0-gap-rev1.md`
HEAD lúc lập plan: `7ac4d7facd7f62bdd30361580eb5d6e636481b70`
Freeze SHA (this run): `285ccc685f4d15442621b58a339d425a3d3ed972`
rustc: `rustc 1.95.0 (59807616e 2026-04-14)` (`cargo +1.95.0`; not host 1.96.1)

## Gates (điền khi chạy)

| Gate | Command | SHA | rustc | Result |
|---|---|---|---|---|
| fmt | `cargo +1.95.0 fmt --all -- --check` | `285ccc685f4d15442621b58a339d425a3d3ed972` | 1.95.0 (59807616e 2026-04-14) | PASS (exit 0) |
| clippy | `cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings` | `285ccc685f4d15442621b58a339d425a3d3ed972` | 1.95.0 (59807616e 2026-04-14) | PASS (exit 0) |
| test | `cargo +1.95.0 test --workspace --locked -- --test-threads=4` | `285ccc685f4d15442621b58a339d425a3d3ed972` | 1.95.0 (59807616e 2026-04-14) | PASS: 2328 passed, 67 ignored, 0 failed (27 suites; TEST_EXIT 0) |
| audit | `cargo +1.95.0 audit` | `285ccc685f4d15442621b58a339d425a3d3ed972` | 1.95.0 (59807616e 2026-04-14) | FAIL: 20 vulnerabilities, 8 allowed warnings; cargo-audit 0.22.2. High/critical dispositions below. |
| macos-compile | CI job | | 1.95.0 | |
| release.yml | run id | | | |
| GUI smoke | runbook subset | | | |
| R-01 runtime | red-button quit | | | |

Không copy hàng PASS từ host 1.96.1 (2026-11-04) vào đây như evidence release.

## Audit high/critical dispositions

`cargo +1.95.0 audit` scanned 674 lockfile crates against advisory-db (1403 advisories). Freeze not auto-blocked; lockfile not mutated in this task.

| ID | Crate | CVSS | Disposition |
|---|---|---|---|
| RUSTSEC-2026-0340 | quinn-proto 0.11.17 | 7.5 high | Accept for freeze: transitive QUIC (`quinn` 0.11.11). Product path is SQL desktop + HTTP/TLS via rustls; ACK_FREQUENCY panic needs a QUIC peer. Upgrade to ≥0.11.18 is lockfile bump, not this SHA. |
| RUSTSEC-2026-0343 | quinn-proto 0.11.17 | 7.5 high | Same as 0340: datagram buffer panic on sender; same unused-in-app QUIC surface. |
| RUSTSEC-2026-0369 | zip 2.4.2 | 9.1 critical | Accept for freeze: `zip` is via `rust_xlsxwriter` 0.80.0 (xlsx write). Advisories are `ZipArchive::extract` / `ZipStreamReader::extract` symlink + Unix mode on untrusted archives. App does not extract attacker ZIPs. Fix is zip ≥9.0.0 (xlsxwriter bump). |
| RUSTSEC-2026-0429 | zip 2.4.2 | 7.5 high | Same extract() abort on large symlink size; write-only xlsx, not extract. |
| RUSTSEC-2026-0367 | zip 2.4.2 | 7.3 high | SUID/SGID on extract; same write-only path. |

No other advisories reported as high/critical. Remaining 15 vulns are medium/low/unscored (rsa Marvin, rustls, rustls-webpki, webpki-roots, zip 0368 unscored symlink extract, quinn-proto 0338 unscored panic). Unmaintained/yanked warnings allowed.

Critical zip extract is not exercised on this SHA if xlsx export only creates archives. If freeze policy is “zero high/critical in lockfile regardless of reachability”, this SHA must not ship until zip/quinn bumps — that is a product call, not a silent PASS.
