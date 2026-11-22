# v0.1.0 Release Closeout — Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Đưa `main` từ NO-GO (HEAD `7ac4d7fa`) thành candidate có artifact, evidence cùng SHA, và cổng `INTERNAL_RC_READY = YES` tại SHA đó; `PUBLIC_RELEASE_READY` chỉ lật YES sau R-01 đã verify trên Mac + HD-002/007/008 đã ghi.

**Architecture:** Umbrella plan. Không thay thế `docs/plans/active/macos-shutdown-crash/` — Wave B thực thi remediation đã chọn ở đó. Wave A/C/D là docs, CI, packaging, smoke. Wave E (MySQL SAVEPOINT) là P2, không chặn `v0.1.0-rc.1`.

**Tech Stack:** Rust 1.95.0 (`rust-toolchain.toml`), eframe 0.36.2, `accesskit_macos` 0.26.3, GitHub Actions `ci.yml` / `release.yml`.

**Spec:** `~/.agents/outputs/db-pro/reports/2026-11-04-release-checklist-v0.1.0-gap-rev1.md` (F-01…F-10). Hồ sơ nền: `docs/release/0.1.0-readiness.md`, `docs/release/0.1.0-human-decisions.md`, `docs/release/known-limitations.md` LIM-021.

State: PLANNING
Branch: `chore/v0.1.0-release-closeout` (docs/CI) rồi `fix/macos-shutdown-crash` (Wave B)

## Global Constraints

- Toolchain evidence phải chạy `cargo +1.95.0` (không tin host `RUSTUP_TOOLCHAIN=1.96.1`).
- Cấm `std::process::exit(0)` trên quit path (bỏ `App::save`).
- Cấm tắt feature `accesskit` (M4 đã reject).
- Cấm sửa tay `docs/release/evidence-manifest.json`; chỉ `release.yml` sinh manifest.
- Cấm tag `v0.1.0` trước khi `v0.1.0-rc.1` + smoke GUI tối thiểu trên **cùng SHA** artifact.
- Không merge/dispatch production secret. Signing vẫn unsigned trừ khi HD-002 đổi.
- MySQL joined SAVEPOINT không phải AC của `v0.1.0-rc.1`.

## Default owner (áp dụng trừ khi user override)

| ID | Default plan | Lý do |
|---|---|---|
| HD-002 | Accept unsigned + Gatekeeper/SmartScreen trong release notes | `R003` đã ACCEPTED cho internal |
| HD-003 | Ship Win/Linux archive, ghi `BUILD_VERIFIED` / `RUNTIME_UNVERIFIED` | Không có host runtime |
| HD-004 | Giữ Agent Preview + limitation | Status quo |
| HD-005 | SSH/backup documented unqualified | Status quo |
| HD-006 | Docs-only `pg_restore` skew | P2, không đụng backup path |
| HD-007 | Internal rc.1: reduced smoke (launch + window + SQLite `SELECT 1` + quit). Public: full runbook | Full 165 item không chặn rc.1 |
| HD-008 | Tag `v0.1.0-rc.1` trước, `v0.1.0` sau public gate | readiness recommendation |
| R-01 | **M2** cho v0.1.0 (terminate qua `NSApp`); **M1** eframe 0.37 = follow-up riêng | macos-shutdown-crash PLAN: M1 durable, M2 ship-now |

## Review Focus

- Quit bằng **red button / chuột** (AXPress không tái hiện R-01).
- Quit phải còn `App::save` (theme + `dbpro.native.query-history-v1` + saved connection).
- Artifact SHA phải khớp HEAD freeze, không reuse `85a7fa3`.
- `cargo +1.95.0` vs 1.96.1 không được trộn trong cùng hàng evidence.
- Capture harness `ViewportCommand::Close` cũng phải exit 0 (STATUS đã ghi abort sau capture).

## Non-goals

- Installer `.dmg`/MSI, notarization, macOS x86_64 (`R-PKG-DEFER`).
- eframe 0.37 (M1) trong closeout này.
- Bật CI `push` mọi commit main (tốn runner); Wave A chỉ thêm job macOS compile + document dispatch.
- Sửa MySQL `ERROR 1295` trong rc.1.

## File map

| File | Vai trò |
|---|---|
| `docs/release/0.1.0-human-decisions.md` | Ghi default HD-* |
| `docs/release/known-limitations.md` | LIM-001, LIM-002, LIM-021 status |
| `docs/release/0.1.0-final-freeze-and-signoff.md` | Addendum superseded |
| `docs/release/0.1.0-readiness.md` | Addendum HEAD freeze |
| `docs/plans/STATUS.md` | Hàng closeout |
| `.github/workflows/ci.yml` | Job compile macOS `db-pro-native` |
| `crates/native-app/src/main.rs` | M2: close → `NSApp terminate` sau save |
| `crates/native-app/src/app_menu.rs` | Cmd+Q đã terminate; không đổi semantics |
| `crates/app/src/shell/chrome/shell_topbar_view.rs` | In-app close vẫn ViewportCommand::Close — M2 phải bắt trước teardown |
| `scripts/release/*` | Không đổi trừ khi packaging fail trên SHA mới |

---

### Task 1: Ghi HD defaults + vô hiệu freeze-signoff giả

**Files:**
- Modify: `docs/release/0.1.0-human-decisions.md`
- Modify: `docs/release/0.1.0-final-freeze-and-signoff.md` (addendum đầu file)
- Modify: `docs/release/known-limitations.md` LIM-001 (HD-009 retain DB Pro)
- Modify: `docs/plans/STATUS.md` (hàng mới)

**Interfaces:**
- Consumes: gap report F-03, F-07, F-10
- Produces: HD-002…008 = DECIDED với default bảng trên; freeze-signoff không còn là cổng

- [ ] **Step 1:** Đổi từng HD-002…008 từ `PENDING` → `DECIDED` + ngày + default bảng. Không đụng HD-001/009.
- [ ] **Step 2:** Đầu `0.1.0-final-freeze-and-signoff.md` thêm block: Status **SUPERSEDED**; SHA `a48ec028` không còn candidate; cổng lấy `0.1.0-readiness.md`.
- [ ] **Step 3:** LIM-001 Status → `Accepted v0.1`, trích HD-009.
- [ ] **Step 4:** Commit `docs(release): record v0.1.0 closeout owner defaults`

**Acceptance:** `rg PENDING docs/release/0.1.0-human-decisions.md` không còn HD-002…008.

---

### Task 2: Đồng bộ LIM-002 với MySQL thật

**Files:**
- Modify: `docs/release/known-limitations.md` LIM-002
- Modify: `docs/release/0.1.0-release-notes.md` (câu driver)

**Interfaces:**
- Consumes: F-08, F-09
- Produces: wording “MySQL/SQL Server shipped với capability gap; parameterized tx không join query-tab lease (ERROR 1295)”

- [ ] **Step 1:** Đọc `DatabaseCapabilities::mysql` và UI driver card hiện tại; viết lại LIM-002 theo code, không theo bản 2026-09-15.
- [ ] **Step 2:** Release notes: không viết “chỉ PostgreSQL và SQLite” nếu UI đã có MySQL/SQL Server.
- [ ] **Step 3:** Commit `docs(release): correct LIM-002 mysql capability`

**Acceptance:** LIM-002 không còn “UI driver card still disabled” nếu code đã enable.

---

### Task 3: CI compile-check macOS

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: F-05, macos-shutdown-crash F-3 (`ci.yml` ubuntu-only)
- Produces: job `macos-compile` trên `macos-14`

- [ ] **Step 1:** Thêm job `macos-compile`:
  - `runs-on: macos-14`
  - `dtolnay/rust-toolchain@master` với `toolchain: 1.95.0`
  - `cargo build --locked -p db-pro-native --all-targets`
  - Không bắt buộc full workspace test trên Mac (tốn phút)
- [ ] **Step 2:** Giữ trigger `pull_request` + `workflow_dispatch`. Không thêm `push` main trong task này.
- [ ] **Step 3:** Mở PR nhỏ, đợi job xanh, merge.
- [ ] **Step 4:** Commit `ci: compile db-pro-native on macos-14 with rustc 1.95.0`

**Acceptance:** PR check `macos-compile` SUCCESS. Code `#[cfg(target_os = "macos")]` compile trước release.yml.

---

### Task 4: M2 — quit macOS qua `NSApp terminate` sau `App::save`

**Files:**
- Modify: `crates/native-app/src/main.rs` (~612–626 close/teardown)
- Possibly: wrapper `eframe::App` trong `crates/native-app/src/` (intercept `ViewportCommand::Close`)
- Test: `crates/native-app/src/` unit test `#[cfg(target_os = "macos")]` cho helper “close maps to terminate, not process::exit”
- Plan evidence: `docs/plans/active/macos-shutdown-crash/VERIFICATION.md`

**Interfaces:**
- Consumes: macos-shutdown-crash PLAN M2; `AppMenu` Cmd+Q đã `PredefinedMenuItem::quit` → `terminate:`
- Produces: mọi close path (red button, Cmd+W, in-app control, capture Close) gọi `-[NSApplication terminate:]` **sau** eframe save/flush; không `process::exit`

- [ ] **Step 1:** Viết test (Linux compile): helper `macos_close_uses_terminate()` / không chứa `process::exit`. Fail trước khi có hook.
- [ ] **Step 2:** Implement hook `#[cfg(target_os = "macos")]` trên close request. Không disable accesskit.
- [ ] **Step 3:** `cargo +1.95.0 test --workspace --locked` trên Linux PASS; `macos-compile` PASS.
- [ ] **Step 4:** **BLOCKED đến Mac Touch-Bar-class:** red click × N, exit 0, không `NSRangeException`, theme + history còn sau relaunch. Ghi SHA vào VERIFICATION.md.
- [ ] **Step 5:** Commit `fix(macos): terminate via NSApp on window close (R-01 M2)`

**Acceptance:** Protocol PLAN macos-shutdown-crash § Acceptance. Linux không được ghi PASS runtime.

---

### Task 5: Gate pin 1.95.0 + cargo audit trên freeze SHA

**Files:** none (commands). Ghi `docs/plans/active/v0.1.0-release-closeout/VERIFICATION.md`

- [ ] **Step 1:** `rustup toolchain install 1.95.0`
- [ ] **Step 2:** Run:
  ```
  cargo +1.95.0 fmt --all -- --check
  cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings
  cargo +1.95.0 test --workspace --locked -- --test-threads=4
  cargo +1.95.0 audit
  ```
- [ ] **Step 3:** Ghi số passed/ignored, rustc `--version` = 1.95.0, SHA.
- [ ] **Step 4:** High/critical audit: disposition từng advisory hoặc chặn freeze.

**Acceptance:** Cùng SHA sẽ đưa vào `release.yml`. Host 1.96.1 không được trích làm hàng PASS.

---

### Task 6: Freeze + dispatch CI + `release.yml`

**Files:**
- Modify: `docs/release/0.1.0-readiness.md` addendum SHA mới
- Do not hand-edit `evidence-manifest.json`

- [ ] **Step 1:** SHA freeze = merge Task 1–4 (Task 4 runtime Mac có thể freeze “code landed, runtime PENDING” chỉ cho **internal rc** nếu owner chấp nhận — default: **không** freeze macOS public trước Task 4 Step 4).
- [ ] **Step 2:** `gh workflow run CI.yml --ref <sha>` và `gh workflow run release.yml --ref <sha>`
- [ ] **Step 3:** Download 3 archive + SHA256SUMS; `shasum -a 256` khớp; archive chứa `LICENSE` + `THIRD_PARTY_NOTICES.md` (R-02).
- [ ] **Step 4:** Commit docs addendum với run id + checksums **sao chép từ artifact**, không bịa.

**Acceptance:** Manifest/run attest đúng freeze SHA. F-02 đóng.

---

### Task 7: Reduced GUI smoke trên artifact Task 6

**Files:** tick `docs/release/0.1.0-interactive-verification-runbook.md` (subset) hoặc file evidence mới dưới `docs/release/evidence/`

- [ ] **Step 1:** Extract archive macOS (và Linux nếu host có display).
- [ ] **Step 2:** Launch, window render, tạo SQLite, `SELECT 1;`, quit.
- [ ] **Step 3:** Relaunch: connection còn. Nếu M2 chưa verify, quit có thể abort — ghi BLOCKED, không PASS.
- [ ] **Step 4:** Không tick 165 item chưa chạy.

**Acceptance:** HD-007 reduced gate. Public full runbook = task riêng sau rc.1.

---

### Task 8: Tag `v0.1.0-rc.1`

- [ ] **Step 1:** Confirm `crates/native-app/Cargo.toml` version `0.1.0`.
- [ ] **Step 2:** `git tag -a v0.1.0-rc.1 -m "v0.1.0-rc.1 (<sha>)"` trên freeze SHA; push tag (owner).
- [ ] **Step 3:** Không GitHub Release public nếu HD-002 unsigned chưa được nêu trong notes.
- [ ] **Step 4:** `v0.1.0` final **out of this plan** cho đến PUBLIC_RELEASE_READY.

**Acceptance:** `git rev-parse v0.1.0-rc.1^{}` == freeze SHA.

---

### Task 9 (P2, sau rc.1): MySQL savepoint text-protocol

**Files:** `crates/infrastructure/src/mysql/connector.rs:549+`, tests live `mysql_integration`

- [ ] Chỉ làm sau rc.1. Dùng text protocol / capability-gate — **không** `sqlx::query("SAVEPOINT")`.
- [ ] Không salvage PR #555/#557/#558/#561.
- [ ] Cần session_id xuyên `ApplyTableChanges` nếu claim Data Grid join tab tx; nếu không, ghi limitation và đừng claim.

**Acceptance:** Live MySQL: BEGIN tab → mutation joined → ROLLBACK hoàn tác **hoặc** capability `false` + UI reason. Không ERROR 1295.

---

## Checkpoints

| Sau | Gate |
|---|---|
| Task 1–2 | Docs không còn freeze giả / LIM-001-002 sai |
| Task 3 | `macos-compile` xanh |
| Task 4 | M2 code + (nếu có Mac) runtime exit 0 |
| Task 5–6 | Artifact đúng SHA, 1.95.0 |
| Task 7–8 | `v0.1.0-rc.1` |
| Task 9 | Optional P2 |

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Không có Mac Touch-Bar | R-01 runtime BLOCKED | rc.1 ghi macOS known crash; không public macOS |
| M2 không đủ trên một số Mac | Vẫn abort | Fallback M1 eframe 0.37 (plan riêng) |
| eframe close không hook được | M2 fail compile/behavior | Prototype trên Mac trước merge |
| `cargo audit` high | Freeze trễ | Disposition từng CVE, không ignore im lặng |

## Open questions (user)

1. Override default HD-*? (bảng trên)
2. M2 now vs chờ M1 eframe 0.37 trước rc.1?
3. Có Mac Touch-Bar để chạy Task 4 Step 4 trong phiên này không?
