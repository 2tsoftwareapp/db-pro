# Changelog

All notable user-facing changes to DB Pro. The shipped application is the native Rust binary
`db-pro-native` (`eframe`/`egui`).

## [Unreleased]

### Fixed

- Agent runs now count cached tool-call replays toward the existing ten-step limit, stopping
  repeated provider requests without re-executing tools or bypassing confirmation (#448).
- Saved-task schedules reject zero-second intervals when enabled; SkipMissed now skips full-interval
  backlog without suppressing ordinary polling jitter, while RunOnce still runs once (#504, #462).
- Data compare keeps SQL NULL distinct from literal `∅` and structural composite keys distinct when
  values contain U+001F; generated sync previews preserve the intended rows and scalar values (#457, #485).
- SQLite maintenance rejects table-target VACUUM instead of widening scope; VACUUM ANALYZE runs
  both operations on the selected database. Constraint errors distinguish primary keys and retain
  otherwise-unclassified extended codes rather than reporting every failure as CHECK (#491, #498).
- SSH password credentials reach the spawned transport process, tunnels remain alive through awaited
  provider disconnect, and MySQL tunnel choices are no longer silently discarded. Unsupported SQL
  Server tunnel choices reach explicit validation; tunneled diagnostics no longer invent SSH success
  or probe the direct database route (#435, #451, #475; partial diagnostics work for #400).
- Reconstructed table DDL uses provider-specific identifier quoting and preserves MySQL
  cross-database FK targets/supporting indexes. Primary-key reload uses the existing dialect's
  row-limit syntax, including SQL Server ordering/pagination (#442; #441 live SQL Server gate pending).
- PostgreSQL parameterized reads prepare using actual converted argument types before describing
  metadata, fixing INTEGER-key reloads with Int64 values while preserving contextual NULL inference,
  empty-result metadata and capped-query completion without draining an unused slow tail (#533).
- MySQL parameterized runs joined to a tab transaction discard the connection when abandoned or
  savepoint cleanup cannot be confirmed, preventing a later COMMIT from persisting partial writes.
  Rollback and savepoint release now share a bounded cleanup timeout.

## [0.1.0] - 2026-09-14

DB Pro 0.1.0 is the first release candidate of the native desktop application. The candidate SHA is
`85a7fa3cc0a84c56ac2a5049ce08130db06e0a20`; the **final release run `34860902181` is green end to
end** for that SHA (all three platforms built and packaged, with independently re-hashed
checksums; the superseded run `34859158012` covered `1a0c186`). The `v0.1.0` tag has not
been created — the recommended sequence is `v0.1.0-rc.1` first, then the host-install smoke, then
`v0.1.0`; see `docs/release/0.1.0-readiness.md` for the release decisions and
`docs/release/0.1.0-handoff.md` for the tag/release/rollback procedure.

### Added

- **Native application shell.** Rust `eframe`/`egui` desktop app (`db-pro-native`) with no
  WebView or Node runtime; fonts compiled in, SQLite bundled, no asset downloads at launch.
- **PostgreSQL and SQLite connections.** Create, test, connect, disconnect, reconnect, edit and
  delete; SQLite uses the native OS file picker; previously active connections reconnect on
  startup. Credentials are stored in the OS keyring with an encrypted-file fallback.
- **Connection Explorer.** Schemas, tables and views with targeted refresh; single click opens a
  Data preview, double click promotes it to a permanent tab.
- **Query Editor.** Syntax highlighting, schema-aware completion, current-statement / selection /
  run-all execution from a split Run control, multi-statement results as separate
  `Result 1..N` tabs plus a `Messages` tab, structured diagnostics, SQL formatting, query history
  and local drafts.
- **Table Data Editor.** Virtualized grid with filtering, sorting, pagination, column resize with
  persisted layout, row selection and scoped copy, PK-based staged insert/update/delete with
  Enter-to-stage, pending-change review, patch-style updates that send only changed columns, and
  stable staged revision identities.
- **Schema workbench.** Per-table Data / Columns / Indexes / Relations / Triggers / DDL
  inspection with reconstructed DDL and FK navigation.
- **ER Diagram.** Schema-level canvas with a native graph model, spatial index, three-tier level
  of detail, BFS neighbourhood exploration and a coalescing background layout worker.
- **Agent panel (Preview).** Ask/Edit/Agent modes producing reviewable SQL drafts, with a
  confirmation gate on every database-mutating tool call. Optional provider via
  `GROQ_API_KEY`/`OPENAI_API_KEY`.
- **Export and backup subset.** CSV/TSV export from the result grid; SQLite `VACUUM INTO` backup;
  PostgreSQL backup/restore through `pg_dump`/`pg_restore`.

### Changed

- **The React/TypeScript/Vite frontend and its Tauri WebView host were retired** (2026-09-11) and
  archived under `_archive/frontend/`; the native UI is the only UI. CI, the release pipeline and
  the documentation were switched to `db-pro-native`. The legacy `crates/tauri-app` host is kept
  only for parity comparison and is not shipped.
- **Native visual redesign (waves 1–14):** light theme first, full-window data surfaces,
  context-aware status bar, live query cursor status, keyboard grid focus, ER search recovery,
  composed empty states and compact query overflow actions.
- Explorer sidebar rebuilt as a unified hierarchical navigator with a filter bar and
  views/functions/triggers folders.
- Release packaging moved from Tauri bundles to portable archives + `SHA256SUMS.txt`
  (macOS ARM64 `.tar.gz` with a minimal `DB Pro.app`, Windows x86_64 `.zip`, Linux x86_64
  `.tar.gz`). The compiler is pinned to 1.95.0 through `rust-toolchain.toml`, and the release
  workflow reads and verifies that pin.

### Fixed

- Table single-click opens Data preview instead of structure; double-click promotes Data without
  creating duplicate tabs; "Open Structure" explicitly opens Columns.
- Run's main segment executes directly; the chevron only opens options.
- Shortcut labels follow macOS/Windows/Linux semantics; shortcut and grid focus handling were
  corrected for the native shell.
- Status bar distinguishes reconnecting and error states; startup reconnect behaviour corrected.
- Explorer "Refresh" targets the connection that was clicked and invalidates the backend and
  client metadata caches.
- Row-selection copy no longer uses a document-global keyboard listener, so it stops hijacking
  in-cell editor and input copy.
- Staged writes no longer replay already-successful changes after a partial apply; staged row
  edits no longer use stale full-row snapshots.
- **The packaged app can start when launched the normal way.** A Finder/`open`-launched macOS
  bundle inherits `cwd=/`, so the app had resolved its state directory to `/.db-pro-data`, failed
  with `CreateDataDir(ReadOnlyFilesystem)` and exited before opening a window; the data directory
  is now resolved from `DB_PRO_DATA_DIR`, an existing `<cwd>/.db-pro-data`, or the per-user
  platform data directory (`543b526`).
- **The packaged `README-INSTALL.txt` documents the real state location** (it previously described
  the pre-`543b526` behaviour); the "First run" section now lists the resolution order (`85a7fa3`).
- **The release pre-flight is deterministic.** The ER layout-worker coalescing test asserted on the
  first asynchronous layout result and failed on slow runners; the tests (not the worker) were
  rewritten to wait for the newest layout, with a negative control that still catches a worker
  losing the newest request (`1a0c186`).
- Cancelling a connection test now cancels its active runtime request and releases the temporary
  provider operation; the connection dialog remains open if the cancellation request cannot be
  dispatched.
- The connection modal now shows a dedicated **Cancel Test** action while a test request is pending;
  it cancels the request and keeps the dialog open, separate from modal **Cancel**.

### Security & Safety

- Read-only connections are enforced in the backend mutation services, not only in the UI;
  tables without a primary key are read-only in the grid.
- Destructive SQL is classified and confirmed in the canonical action path before execution.
- Table mutations apply atomically per batch: a failure rolls the batch back and keeps all staged
  changes for retry. Concurrent external edits surface a three-way conflict dialog
  (Original / Local / DB Current, Keep Mine / Use Database).
- Connection secrets are kept in the OS keyring and referenced from the metadata database by a
  `secret_ref` indirection rather than being stored inline.
- The release build no longer contains the developer connection preset (label, database name,
  tag, user, password); it is compiled only into debug builds. The component gallery no longer
  renders the developer connection label.
- Release workflow runs with `contents: read`, uses no secrets, and can neither create a GitHub
  Release nor push a tag.

### Known limitations

- Runtime smoke of the 0.1.0 candidate is not complete; V01-01/02/04/05 runtime evidence is
  missing and V01-03 is partial (`docs/release/evidence/v01-06/04-v01-01-05-evidence-audit.md`).
- Workspace/tab/settings persistence is not implemented in the native build.
- Query cancellation works for SQLite; PostgreSQL cancellation is capability-gated `Unsupported`.
- Row insert is narrower than a full workflow for complex column types (JSON, array, UUID).
- macOS x86_64 is not built; no installers (`.dmg`, MSI/NSIS, `.deb`/`.rpm`, AppImage) and no
  auto-update.
- Linux credential storage requires a D-Bus Secret Service provider.
- `pg_dump`/`pg_restore` and `ssh` are not bundled and must be on `PATH`.
- CHECK/Unique constraint introspection is unqualified; SSH tunnels are not end-to-end qualified.
- Release artifacts are unsigned: macOS Gatekeeper and Windows SmartScreen will warn.
- The project source and v0.1.0 artifacts now use MIT (`R-LICENSE` resolved); third-party notices
  retain their own terms and the exact binary notice bundle remains a pre-public-release task.
- The Agent panel is Preview and has no recorded live-provider verification run.

---

## Historical (pre-native)

The sections below record the retired frontend/Tauri era. They are kept for history; the
React/Vite frontend was archived on 2026-09-11 and no longer gates release.

### Added / Completed (native release-candidate scope, recorded before 2026-09-14)

**Database support**
- PostgreSQL connection lifecycle
- SQLite file connections + native Browse wiring
- startup reconnect source flow

**Explorer / DB Object**
- schemas, tables, views
- targeted connection metadata refresh
- Data-first table/view navigation
- Data / Columns / Indexes / Relations / DDL inspection

**Query workbench**
- Native SQL editor
- current-statement execution
- selection execution
- run-all / multi-statement execution
- split Run button
- Explain
- SQL format
- query history
- result grid
- Stop/Escape cancellation source flow

**Workspace / navigation**
- query and DB Object tabs
- preview/pin/close/navigation
- compact inactive pinned tabs
- collision-aware resource titles
- orphan-tab recovery
- Quick Open and Command Palette

**Data Grid**
- virtualization
- filtering, sorting, pagination
- column resize/layout persistence
- row selection/copy
- PK-based staged update/delete
- patch-style row updates (only changed columns sent)
- same-row multi-cell patch composition
- stable staged revision IDs and in-flight safety
- partial-success/failure cleanup by exact revision ID
- apply confirmation for destructive staged changes

**Architecture / safety**
- canonical Action Platform runtime
- confirmation lifecycle
- cancellation identity
- read-only backend policy
- secret/keyring infrastructure

### Deferred from 0.1.0

- a complete row-insert workflow for complex column types (staged insert ships; JSON/array/UUID
  inputs are incomplete — LIM-003)
- advanced schema mutation / DDL execution workbench
- users / roles workbench
- production/autonomous Agent execution (the Agent panel ships as Preview)
- MCP server
- additional database drivers
- installer formats and code signing/notarization

### Remaining release gates (recorded 2026-09-11, superseded by the 2026-09-14 state above)

- exact-SHA Rust verification (`cargo fmt/check/clippy/test --workspace`)
- `cargo build --release --locked -p db-pro-native`
- macOS / Windows / Linux native release builds + retained artifacts
- packaged/manual runtime smoke on the native app

---

## [Unreleased]

Any change made after the 0.1.0 release candidate cut is recorded here. The 0.1.0 candidate itself
is frozen at `fbf9fdab9100f08f12e29434983f32c18f14ac2f`.

### Changed

- **Native UI: visual redesign and DBeaver/Codex sidebar consolidated into main (2026-09-11).**
  - Native visual redesign (waves 1–14): Codex-aligned theme tokens, full-window data surfaces,
    context-aware status bar, live query cursor status, keyboard grid focus, ER search recovery,
    staged-grid interaction correctness, composed empty states, compact query overflow actions.
  - Explorer sidebar rebuilt as a unified DBeaver/Codex hierarchical navigator with a filter bar,
    views/functions/triggers folders and a Codex-style tree row.
  - Light theme is now the default (`THEME_STORAGE_VERSION = "light-first-v1"`); window opens
    maximized.
  - New shared components (spinner, progress bar, skeleton, switch, segmented control, kbd chip,
    tag chip, status dot, toast) added to `crates/ui` and exported from `lib.rs`.
  - Development default: in **debug builds only**, a "Xe Lạc Hồng (PostgreSQL)" connection is
    seeded on launch and pre-fills the New connection dialog. Developer convenience, not a
    product default; release builds no longer contain it.
- **UI: React frontend archived; native UI is the development direction (2026-09-11).**
  - `frontend/` moved to `_archive/frontend/`; it is reference material only and is no longer built, tested, or packaged.
  - `bench/` (React Flow / Cytoscape ER-renderer harnesses with vendored React) moved to `_archive/bench/` for the same reason.
  - `crates/ui` (egui), `crates/runtime`, and `crates/native-app` (`db-pro-native`) are the shipped UI stack.
  - CI: the frontend job was removed; the Rust job installs native GL/X11/Wayland headers and builds `db-pro-native`.
  - Release: the pipeline now builds `db-pro-native` for macOS/Windows/Linux instead of Tauri bundles; installer packaging and signing are not implemented yet.
  - `crates/tauri-app` is retained as a legacy transitional host and is marked for removal at cutover.
  - README, AGENTS.md, architecture docs, release docs, and plans were updated to describe native UI as the development direction.

### Fixed

- Native UI regressions after the egui 2→3 upgrade: legacy sidebar buttons now use the shared
  theme-aware component; modal Tab/Shift+Tab traversal stays inside the dialog; required connection
  and Save Query fields show markers and inline errors; Agent Settings marks the API key required.
- Connection port validation now rejects `0`, matching the existing 1–65535 validation message.
- Removed global noninteractive strokes, standard input outlines at rest, and dialog separators while
  preserving search, focus, error, and explicitly framed component outlines.
- **Keyboard input & focus fixes:** global shortcuts no longer die after a dialog closes (stale
  modal registry) or after clicking a non-text widget (`egui_wants_keyboard_input` was too broad);
  editor-scoped chords (Save, New, Find) no longer steal keys while an egui text field is typing;
  Run defaults to Ctrl/Cmd+Enter (F5 still works) and Enter with a selection no longer deletes the
  selected text; the data-grid WHERE input keeps focus across autocomplete and runs on Ctrl/Cmd+Enter;
  Esc inside cell editors/date pickers stays local and only closes the Agent panel when nothing is
  focused; grid rows no longer hover-highlight beneath the Agent panel; the Agent composer accepts
  typing again (pinned to the panel bottom so it is never clipped) and Enter submits with
  Shift+Enter for newlines (#337–#340, #342, #349, #358, #372, #379).

### Fixed during release UX closure

- table single-click now opens Data preview rather than structure/config
- table/view double-click promotes Data without duplicate tabs
- Open Structure explicitly opens Columns
- Run main segment executes directly; chevron only opens options
- shortcut labels aligned across macOS/Windows/Linux semantics
- status bar distinguishes reconnecting/error states
- startup reconnect behavior corrected
- SQLite dialog plugin registered on Rust side with minimal capability
- Explorer Refresh targets the clicked connection and invalidates backend/client metadata caches
- row selection copy no longer uses a document-global keyboard listener
- staged writes no longer replay already-successful changes after partial apply
- staged row edits no longer use stale full-row snapshots
- platform-specific Rollup native binary removed from root dependencies; lockfile regenerated

See `docs/plans/project/07-current-status.md`, `docs/release/0.1.0-readiness.md` and
`docs/release/0.1.0-handoff.md` for current status.

## [0.1.0] — historical placeholder

The earlier `## [0.1.0]` section read: *"Not released yet. This section will be finalized when the
release candidate passes all gates and the tag is created."* It is superseded by the dated
`## [0.1.0] - 2026-09-14` section above. The tag is still not created.
