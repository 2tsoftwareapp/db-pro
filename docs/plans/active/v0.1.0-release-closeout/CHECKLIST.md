# Checklist: v0.1.0 Release Closeout

- [ ] Task 1 — HD-002…008 DECIDED; freeze-signoff SUPERSEDED; LIM-001 đồng bộ HD-009
- [ ] Task 2 — LIM-002 / release notes khớp MySQL thật
- [ ] Task 3 — `ci.yml` job `macos-compile` (rustc 1.95.0, macos-14) xanh trên PR
- [ ] Task 4 — M2 NSApp terminate; Linux tests xanh; runtime Mac BLOCKED đến khi có host
- [ ] Task 5 — `cargo +1.95.0` fmt/clippy/test/audit trên freeze SHA
- [ ] Task 6 — `release.yml` artifact + checksum + LICENSE/notices; không sửa tay manifest
- [ ] Task 7 — reduced GUI smoke trên đúng archive SHA
- [ ] Task 8 — tag `v0.1.0-rc.1` == freeze SHA
- [ ] Task 9 — MySQL SAVEPOINT (P2, sau rc.1)

Không tick runtime macOS nếu chưa có `DiagnosticReports` rỗng + exit 0.
