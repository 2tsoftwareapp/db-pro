# Known Limitations & Non-Goals — v0.1

> Canonical registry of what DB Pro v0.1 intentionally does not support or has not yet qualified.
> Baseline SHA: `65bbca3` (historical); **corrected 2026-09-14 for candidate `fbf9fdab9100f08f12e29434983f32c18f14ac2f`**
> Corrections in this revision: LIM-001/LIM-007/LIM-009 evidence updated to the native build, LIM-003 wording aligned with the staged insert that ships, **LIM-014 inversion fixed** (SQLite cancellation is supported; PostgreSQL is not), LIM-016/LIM-017 added. No entry was deleted.
> Further correction (2026-09-14, after the `543b526` state-directory fix): **LIM-018 added** — application state location and the first-launch credential prompt. No entry was deleted or rewritten.
> Further note (2026-09-14, release-pipeline finalization): the release pipeline is **green end to
> end** — the final run `34860902181` (candidate `85a7fa3`) built and packaged all three platforms
> and produced `SHA256SUMS.txt`, all independently re-hashed (macOS ARM64 / Linux x86_64 / Windows
> x86_64). That verifies **builds/packages only**: Windows and Linux stay
> `RUNTIME_UNVERIFIED` (no host), and the interactive GUI smoke is `NOT VERIFIED`
> (`docs/release/evidence/v01-06/14-install-smoke.txt` §7.7). Final artifact values are recorded in
> `docs/release/0.1.0-readiness.md`, `docs/release/0.1.0-handoff.md` §3 and `risk-register.md` §4.
> No limitation below is retracted by this note.
> Risk IDs referenced below (`R003`, `R-LICENSE`, `R001`) are defined in `docs/release/risk-register.md`.
> Correction (2026-09-15, #234): **LIM-002 corrected** — it said "Only PostgreSQL and SQLite drivers are implemented",
> which stopped being true when the MySQL 8 provider landed (`640eaf4c`): MySQL now has a real adapter with recorded
> live evidence. The entry keeps the v0.1 scope decision and states MySQL's actual level and what it still lacks.
> No entry was deleted.
> Further addition (2026-09-15, #244): **LIM-020 added** — PostgreSQL restore is not transactional and backup/restore cannot be cancelled. No entry was deleted or rewritten.
> Further addition (2026-09-15, #242): **LIM-019 added** — the AI features are the app's only egress and the in-app/release disclosure for it. No entry was deleted or rewritten.
> Further addition (2026-10-26, release-gap close-out): **LIM-021 added** — the macOS shutdown crash (R-01), root-caused to `accesskit_macos` and recorded as **Fix before v0.1**, the first non-zero entry in that status; **LIM-022 added** — the 838 logical-px capture cap is a macOS host constraint, not product behaviour (Linux/Xvfb returns 1440×900 and 1920×1080 exactly). Closed items from the same wave are deliberately **not** entered here: the SSH-tunnel option-injection fix (`754980dc`), the query-history credential masking (`0a2418d9`) and the missing LICENSE/notices in the macOS archive (`ef2197b4`) are fixed, not limitations. No entry was deleted or rewritten.
> Issue: #135
> Supports: #27, #30, #105, #110, #111

## Registry fields

Each entry includes:
- **Title**: short name
- **Category**: connection | schema | query | data-grid | export | er | security | brand | platform | agent
- **Actual behavior**: what the code actually does
- **User-visible impact**: what the user sees
- **Reason**: why this is a limitation
- **Status**: `Accepted v0.1` | `Blocked decision` | `Fix before v0.1` | `Deferred v0.2+`
- **Target issue/release**: tracking issue
- **Safe release-note wording**: what can be said publicly
- **UI/docs locations that must not contradict**: where overclaims could appear
- **Evidence/source**: code references

---

## LIM-001: Public name retained (HD-009)

| Field | Value |
|---|---|
| Category | brand |
| Actual behavior | App is named "DB Pro" with identifier `com.dbpro.app`; name collision with existing dbpro.app product |
| User-visible impact | Public release ships as DB Pro. Brand collision with dbpro.app remains an accepted risk (HD-009). |
| Reason | HD-009 (decided 2026-09-16): retain DB Pro. No source rename, storage migration, or repository rename. |
| Status | Accepted v0.1 |
| Target issue | #101, #104 |
| Safe release-note wording | "DB Pro" |
| Must not contradict | README, app title bar, installer metadata, website (identity is DB Pro) |
| Evidence | HD-009 retain DB Pro. Native literals `with_title("DB Pro")` in `crates/native-app/src/main.rs`. Register: `R001`. |

## LIM-002: PostgreSQL, SQLite, and a post-v0.1 MySQL 8 provider

| Field | Value |
|---|---|
| Category | connection |
| Actual behavior | **PostgreSQL and SQLite are the shipped drivers.** MySQL 8 also has a real provider on `main` (connector, class-aware decoder, introspection, factory registration) with recorded live evidence, but it is **not a shipped v0.1 driver**: it has no parameter binder, no dialect arm, and its driver card in the UI is disabled |
| User-visible impact | In the shipped build, MySQL, MariaDB, SQL Server, Oracle, etc. cannot be connected. The MySQL provider on `main` is reachable from tests and the runtime, not from the UI |
| Reason | v0.1 scope decision for the shipped product; MySQL is the first post-v0.1 engine (parent goal #182, issues #234/#235) and is being landed incrementally behind the provider contract |
| Status | Accepted v0.1 (the shipped-driver restriction is unchanged; the MySQL provider's own level is stated under Actual behavior and in the note below) |
| Target issue | #234 (provider SDK/capability contract), #235 (MySQL 8 provider) |
| Safe release-note wording | "Supports PostgreSQL and SQLite" |
| Must not contradict | README, feature list, marketing |
| Evidence | `crates/core/src/domain/connection.rs` `DriverType` (Postgres, SQLite, Mysql); MySQL provider `crates/infrastructure/src/mysql/` with live evidence `docs/release/evidence/v01-runtime/providers/60-mysql-live-fixture-and-mapper.md`; capability set with the MySQL gaps advertised as `false` in `crates/core/src/domain/capabilities.rs` (`DatabaseCapabilities::mysql`); UI driver card still disabled (`crates/ui/src/connection_view.rs`); contract recorded in `docs/architecture/provider-contract.md` |

**What is still missing for MySQL to be a supported driver** (recorded here so this entry is not
mistaken for "MySQL is supported, just not advertised"): a parameter binder
(`MySqlConnector::query`/`execute` drop the parameter list, so `parameters` and
`positional_parameters` are advertised `false`), a `SqlDialect` arm (without it the table-data
mutation and data-diff paths return a validation error, and `data_diff` is advertised `false`), an
enabled UI driver card, and the service-level features that remain PostgreSQL-only
(`server_sessions`, `partitions`, `backup` — all advertised `false` for MySQL).

## LIM-003: Row insertion incomplete

| Field | Value |
|---|---|
| Category | data-grid |
| Actual behavior | Staged row insert **ships** in the native Table Data Editor (insert dialog + staged commit, PK-targeted). It is narrower than a full insert workflow because complex column types do not have complete input widgets. |
| User-visible impact | Basic inserts work for common types; JSONB/array/UUID columns may lack a proper input widget |
| Reason | Provider-aware editing is complex; deferred for advanced types |
| Status | Accepted v0.1 |
| Target issue | N/A |
| Safe release-note wording | "Basic row insertion for common types" |
| Must not contradict | Data grid documentation, feature claims |
| Evidence | `DataCapabilities.insert = true`; `crates/ui/src/table_editor_view.rs` insert path; native `ChangeSet` staged insert. Corrected 2026-09-14: the earlier "implemented at the backend level but UI may not expose" wording was the pre-native framing |

## LIM-004: Advanced schema mutation deferred

| Field | Value |
|---|---|
| Category | schema |
| Actual behavior | ALTER COLUMN TYPE is supported for PG but the UI does not expose a schema mutation workflow |
| User-visible impact | Users cannot modify column types, add constraints, or restructure schemas through the UI |
| Reason | Schema mutation UX is complex and error-prone; deferred to v0.2 |
| Status | Deferred v0.2+ |
| Target issue | N/A |
| Safe release-note wording | "Schema browsing and introspection; DDL via query editor" |
| Must not contradict | Schema panel UI, feature descriptions |
| Evidence | `SchemaCapabilities.alter_column_type = true` (PG) / `false` (SQLite) but no schema mutation UI |

## LIM-005: Users/roles administration deferred

| Field | Value |
|---|---|
| Category | connection |
| Actual behavior | PG user_manager port exists but full role administration UI is not implemented |
| User-visible impact | Cannot create/modify database users or roles through the app |
| Reason | Out of v0.1 scope |
| Status | Deferred v0.2+ |
| Target issue | N/A |
| Safe release-note wording | "Database connection management; user administration not included" |
| Must not contradict | Feature list |
| Evidence | `crates/core/src/ports/user_manager.rs` (port only); `FeatureCapabilities.server_sessions` |

## LIM-006: SSH tunnel not E2E qualified

| Field | Value |
|---|---|
| Category | connection |
| Actual behavior | SSH tunnel implementation exists (shells out to `ssh` binary) but has not been end-to-end tested |
| User-visible impact | SSH tunneling may not work reliably; no automated test coverage |
| Reason | E2E qualification requires CI infrastructure for SSH server setup |
| Status | Accepted v0.1 |
| Target issue | N/A |
| Safe release-note wording | "SSH tunnel support is experimental" or omit from release notes |
| Must not contradict | Connection docs, feature claims |
| Evidence | `crates/infrastructure/src/ssh/tunnel.rs`; `FeatureCapabilities.ssh_tunnel = true` (PG) / `false` (SQLite) |

## LIM-007: Agent workspace is Preview

| Field | Value |
|---|---|
| Category | agent |
| Actual behavior | Agent workspace exists as a preview feature; not production autonomous DB execution |
| User-visible impact | Agent features may be unstable or incomplete |
| Reason | Agent is a preview feature for v0.1; full autonomy deferred |
| Status | Accepted v0.1 |
| Target issue | N/A |
| Safe release-note wording | "Agent workspace (Preview)" |
| Must not contradict | Agent documentation, marketing claims |
| Evidence | `crates/ui/src/agent_view.rs` (Ask/Edit/Agent; renders a `Preview` badge); confirmation gate in the agent executor. No live-provider run is recorded — `R-AGENT` (`ACCEPTED`) |

## LIM-008: MCP not in v0.1

| Field | Value |
|---|---|
| Category | agent |
| Actual behavior | Model Context Protocol integration is not implemented |
| User-visible impact | No MCP server/client capability |
| Reason | Deferred to v0.2+ |
| Status | Deferred v0.2+ |
| Target issue | N/A |
| Safe release-note wording | Do not mention MCP |
| Must not contradict | Agent docs, feature list |
| Evidence | No MCP code in codebase |

## LIM-009: Signing/notarization absent

| Field | Value |
|---|---|
| Category | security |
| Actual behavior | No code signing or notarization is configured for any platform |
| User-visible impact | macOS Gatekeeper warnings; Windows SmartScreen warnings; Linux no trust indicator |
| Reason | Signing certificate and process not yet established (#118) |
| Status | Blocked decision |
| Target issue | #118 |
| Safe release-note wording | Do not claim signed or notarized |
| Must not contradict | Download page, installation docs |
| Evidence | Measured on the release binary: `Signature=adhoc`, `flags=0x20002(adhoc,linker-signed)`, `TeamIdentifier=not set`, `Info.plist=not bound`; `spctl -a -vvv -t execute` → **rejected** (exit 3). No `notarytool`/`stapler`/`signtool` step exists; no signing secrets exist and none were added. Register: `R003` (`ACCEPTED`) |

## LIM-010: Final binary third-party notice bundle pending

| Field | Value |
|---|---|
| Category | brand |
| Actual behavior | The repository and workspace packages use MIT. The bundled Inter fonts retain SIL OFL 1.1, and the exact resolved dependency graph has been inventoried; a final binary notice bundle is not yet generated |
| User-visible impact | Legal ambiguity for public release |
| Reason | Exact-artifact dependency notices must be preserved before public binary distribution (#119) |
| Status | Accepted v0.1 |
| Target issue | #119 |
| Safe release-note wording | Source and artifacts use MIT; third-party components retain their own licenses and the final notice bundle is pending |
| Must not contradict | Repository, distribution metadata |
| Evidence | `LICENSE`, `THIRD_PARTY_NOTICES.md`, `Cargo.lock`, `crates/ui/assets/fonts/OFL.txt` |

## LIM-011: CHECK constraints kept with residual SQLite parser risk

| Field | Value |
|---|---|
| Category | schema |
| Actual behavior | CHECK constraints are introspected (PostgreSQL catalog; SQLite CREATE TABLE parser), serialized on the Tauri introspect/TableInfo DTOs, mapped through the native bridge, and shown in the table metadata constraints panel |
| User-visible impact | Rare SQLite DDL edge cases may still omit a constraint |
| Reason | Disposition **KEEP v0.1** (#68). #69 hardened the SQLite parser; #70 completed provider→domain→Tauri DTO→native UI exposure; MySQL CHECK via `information_schema` (#235) |
| Status | Accepted with residual |
| Target issue | #68 (decision), #69, #70, #235 |
| Safe release-note wording | "CHECK constraints listed for PostgreSQL, SQLite, and MySQL 8" |
| Must not contradict | Schema panel, introspection docs |
| Evidence | `docs/release/evidence/v01-runtime/providers/65-check-constraint-disposition-keep.md`, `66-sqlite-check-parser-hardening.md`, `67-check-e2e-contract.md` |

## LIM-012: Import deferred

| Field | Value |
|---|---|
| Category | export |
| Actual behavior | Data import (CSV, JSON → table) is not implemented |
| User-visible impact | Users cannot import data files into tables |
| Reason | Out of v0.1 scope |
| Status | Deferred v0.2+ |
| Target issue | N/A |
| Safe release-note wording | "Export to CSV and XLSX; import not yet available" |
| Must not contradict | Data grid docs, feature list |
| Evidence | No import code in infrastructure; `BackupEngine` only handles backup/restore |

## LIM-013: SQLite type limitations

| Field | Value |
|---|---|
| Category | schema |
| Actual behavior | SQLite stores UUID as TEXT, JSON as TEXT, has no native array/enum/generated-column support |
| User-visible impact | Type display may differ from PostgreSQL equivalent; no UUID validation, no JSON editing |
| Reason | SQLite's type system is fundamentally different (affinity-based) |
| Status | Accepted v0.1 |
| Target issue | #132 (capability matrix) |
| Safe release-note wording | "SQLite support with type-system limitations" |
| Must not contradict | Provider capability matrix, type documentation |
| Evidence | `DataCapabilities.sqlite()` — `uuid_type: false`, `array_types: false`, `generated_columns: false` |

## LIM-014: Provider-asymmetric query cancellation

| Field | Value |
|---|---|
| Category | query |
| Actual behavior | **SQLite cancellation works** (the actor interrupts the running VM and waits for acknowledgement). **PostgreSQL cancellation is not implemented**: the connector exposes no wire-level cancel, and the capability is declared `Unsupported` and gates the Stop control off. |
| User-visible impact | Stop/Escape cancels a SQLite query; a long PostgreSQL query cannot be cancelled from the app |
| Reason | PostgreSQL's cancel primitive is not wired through the connector; advertising best-effort task cancellation as provider cancellation would be misleading |
| Status | Accepted v0.1 |
| Target issue | #132 |
| Safe release-note wording | "Query cancellation is supported for SQLite; PostgreSQL cancellation is not available in 0.1.0" |
| Must not contradict | Query editor docs, release notes, provider capability matrix |
| Evidence | `crates/core/src/domain/capabilities.rs`: `postgres.cancel = false`, `sqlite.cancel = true`; `crates/infrastructure/src/postgres/connector.rs:199-203`; `crates/infrastructure/src/sqlite/actor.rs:106-124`. **Corrected 2026-09-14 — the previous entry asserted the exact opposite ("SQLite queries cannot be cancelled", `sqlite.cancel = false`) and contradicted the code.** Register: `R-PROV` |

## LIM-015: pg_dump/pg_restore not bundled

| Field | Value |
|---|---|
| Category | platform |
| Actual behavior | PostgreSQL backup shells out to `pg_dump`/`pg_restore` which must be on PATH |
| User-visible impact | Backup/restore fails silently if pg_dump is not installed |
| Reason | Bundling pg_dump adds significant size; not done in v0.1 |
| Status | Accepted v0.1 |
| Target issue | #134 |
| Safe release-note wording | "PostgreSQL backup requires pg_dump on PATH" |
| Must not contradict | Backup docs, platform prerequisites |
| Evidence | `crates/infrastructure/src/backup/pg_dump.rs` |

## LIM-016: Workspace/session persistence not implemented

| Field | Value |
|---|---|
| Category | platform |
| Actual behavior | The native build does not persist workspace tabs or settings across restarts (eframe persistence is off); previously active *connections* reconnect on startup, but tabs/drafts are not restored |
| User-visible impact | After quitting, the tab layout and open resources are gone; only the startup connection reconnect is restored |
| Reason | Persistence is deferred; implementing it is feature work outside the v0.1 closure scope |
| Status | Accepted v0.1 |
| Target issue | N/A |
| Safe release-note wording | Do not claim workspace/tab restore after restart |
| Must not contradict | `0.1.0-manual-smoke.md` "Workspace tabs restore" item, README, release notes |
| Evidence | eframe persistence feature not enabled; `docs/release/risk-register.md` `R-015` (`ACCEPTED`) |

## LIM-017: macOS ARM64 only; no installers or auto-update

| Field | Value |
|---|---|
| Category | platform |
| Actual behavior | The v0.1 matrix produces a macOS **arm64** archive (Apple Silicon only), a Windows x86_64 `.zip` and a Linux x86_64 `.tar.gz`. No macOS x86_64 artifact, no universal binary, no `.dmg`/MSI/NSIS/`.deb`/`.rpm`/AppImage, no auto-update. All artifacts are unsigned. |
| User-visible impact | Intel Macs have no artifact; there is no installer or update mechanism; macOS/Windows show OS security warnings on first run |
| Reason | Recorded deferral decision (Option A packaging contract); signing identity and installer toolchains do not exist |
| Status | Accepted v0.1 |
| Target issue | N/A |
| Safe release-note wording | "Apple Silicon macOS, Windows x86_64 and Linux x86_64 portable archives; unsigned; no installers" |
| Must not contradict | `0.1.0-packaging.md`, release notes, readiness |
| Evidence | `.github/workflows/release.yml` (matrix pins `macos-14`), `docs/release/0.1.0-packaging.md`, `docs/release/risk-register.md` `R-PKG-DEFER` / `R003`. **Release-pipeline verification (2026-09-14, final):** the final run `34860902181` built and packaged all three targets green for the candidate `85a7fa3` (`BUILD_VERIFIED`), with the archives' member lists and checksums independently reproduced; Windows/Linux remain `RUNTIME_UNVERIFIED` (no host) and the interactive GUI smoke is `NOT VERIFIED` (`14-install-smoke.txt` §7.7). Final artifact values: `docs/release/0.1.0-readiness.md` / `0.1.0-handoff.md` §3 |

## LIM-018: Application state location and first-launch credential prompt

| Field | Value |
|---|---|
| Category | platform |
| Actual behavior | State (`meta.db`, `secrets/`, query history, settings) is stored in the per-user platform data directory — macOS `~/Library/Application Support/DB Pro`, Windows `%APPDATA%\DB Pro`, other Unix `$XDG_DATA_HOME/db-pro` (fallback `~/.local/share/db-pro`) — **unless** `<cwd>/.db-pro-data` already exists, which keeps being used; `DB_PRO_DATA_DIR` overrides both. The build is unsigned, so on macOS a first launch can show a Keychain authorization prompt for the app's own stored provider key before the window appears, and an unattended launch waits there. |
| User-visible impact | On a fresh install the state directory is no longer created next to the working directory; an existing `.db-pro-data` continues to be used, so nothing migrates and no data is orphaned. A first launch can pause on an OS credential prompt. |
| Reason | The previous fallback (`current_dir()/.db-pro-data`) made a Finder/`open`-launched `.app` (which inherits `cwd=/`) resolve `/.db-pro-data`, fail with `CreateDataDir(ReadOnlyFilesystem)` and exit before opening a window; fixed in `543b526`. The prompt follows from shipping unsigned (`R003`): an ad-hoc-signed binary cannot read an existing keyring item without an interactive grant. |
| Status | Accepted v0.1 |
| Target issue | N/A |
| Safe release-note wording | "Application state is stored in your user profile; an existing `.db-pro-data` beside the app is still used. An unsigned build can ask for keychain access on first launch." |
| Must not contradict | `0.1.0-handoff.md` §7, `0.1.0-release-notes.md`, `0.1.0-packaging.md` §4, `README.md` |
| Evidence | `crates/native-app/src/main.rs` (`choose_data_dir`, `platform_data_dir`; tests `platform_dir_is_used_when_no_legacy_dir_exists` and the override/legacy/cwd cases); `docs/release/evidence/v01-06/12-state-dir-blocker-fix.txt` §5–§6; `docs/release/evidence/v01-06/14-install-smoke.txt` §3–§4 (the host smoke had to bypass the prompt with a placeholder `GROQ_API_KEY`, so it does not evidence a plain double-click launch on a machine with a stored key); register `R-STATE-DIR` (`FIXED`), `R-KEYRING-STALL` (`ACCEPTED`), `R003` (`ACCEPTED`), `R011` |

---

## LIM-019: The AI features are the app's only egress

| Field | Value |
|---|---|
| Category | security |
| Actual behavior | The AI features are the **only** component that sends data off the machine, and they do so only once a provider key is configured: with no key the runtime answers "AI provider is not configured" and sends nothing (`crates/runtime/src/worker.rs:1118`). When a key is set, two paths egress to the configured provider — `https://api.groq.com/openai/v1/responses` or `https://api.openai.com/v1/responses`, HTTPS-enforced (`crates/runtime/src/agent.rs:13,15,153`): (a) **inline SQL prediction**, which is on by default (`PredictionMode` defaults to `Eager`, `crates/ui/src/editor/prediction.rs:5-11`) and schedules a request 300 ms after an edit or cursor move (`crates/ui/src/query/query_document.rs:13`), sending the SQL text around the cursor plus the schema context it references; (b) **the agent panel**, which sends the conversation so far — including, for agent query tool runs, an `AgentResultSummary` of at most **20 sample rows × 50 columns, each cell truncated to 256 characters** (`crates/core/src/domain/agent.rs:10-12`, applied in `agent_context.rs`; the #122 audit's "20×12" is stale). A key can also be seeded into the process from the OS keyring at startup (`crates/native-app/src/main.rs:203-220`, service `com.dbpro.app`, account `agent/groq_api_key`), so a key saved once keeps working in later sessions without re-entry. The app's only other outbound connections are the database and SSH connections the user configures. |
| User-visible impact | Enabling an AI provider sends SQL text, schema metadata and — for agent query runs — sample result rows to that provider. The agent panel's API-key section and the editor's AI prediction control now say so in-app (the disclosure added for #242); before that, nothing in the product did. |
| Reason | The AI features need a hosted model provider; there is no local model in v0.1. Because the app is otherwise offline and local-first, the data flow has to be stated rather than assumed. |
| Status | Accepted v0.1 |
| Target issue | #242 (disclosure implemented); #122 (source audit); per-connection or per-session AI opt-out deferred to the post-v0.1 backlogs (#34 agent-native operations, #32 MCP) |
| Safe release-note wording | "The AI features are the app's only network egress beyond your own database and SSH connections. With a key configured, inline prediction sends the SQL around your cursor and its schema context to Groq or OpenAI, and an agent query run sends up to 20 sample result rows. No key, no egress." |
| Must not contradict | `0.1.0-release-notes.md` (Known limitations), `README.md` (Agent provider), `docs/notes/PRODUCT_CAPABILITY_MATRIX.md` §Privacy-security, `docs/release/audit-security-boundaries.md` §1/§5 (T-1), UI text in `crates/ui/src/agent_view.rs` and `crates/ui/src/query_view.rs` |
| Evidence | `docs/release/audit-security-boundaries.md` §1 (data-flow inventory: no telemetry, no update check, no licence check, no remote asset; only these two endpoints) and §5 (T-1); `crates/ui/src/agent_view.rs` (`AI_EGRESS_DISCLOSURE`), `crates/ui/src/query_view.rs` (`AI_PREDICTION_EGRESS_NOTE`); `docs/release/evidence/v01-runtime/providers/54-ai-egress-disclosure.md` |

## LIM-020: PostgreSQL restore is not transactional; backup and restore cannot be cancelled

| Field | Value |
|---|---|
| Category | data-grid |
| Actual behavior | Two related gaps in the shipped backup/restore surface. (1) **A PostgreSQL restore is not atomic**: the plain path runs `psql -f <dump>` and the custom path runs `pg_restore <dump>` with neither `--single-transaction` nor `--exit-on-error` (`crates/infrastructure/src/backup/pg_dump.rs`, argv pinned by `restore_argv_has_no_transaction_boundary_and_is_documented_as_such`), so a mid-script failure leaves the **target database partially restored**. #244 chose option (b) of that issue — keep the behaviour, state the consequence — and the failure message now says so ("the restore was NOT run in a transaction … may now be partially restored; inspect it before using it"), naming the state rather than leaving it in a bare subprocess error. SQLite restore is validate-then-rename by contrast (`sqlite_backup.rs`), i.e. all-or-nothing. (2) **A running backup or restore cannot be cancelled**: `RuntimeCommand::CancelOperation` is declared (`crates/runtime/src/worker.rs:205`) and handled (`:1563`) but constructed nowhere in the workspace, so there is no Stop affordance for `pg_dump`. |
| User-visible impact | A failed PostgreSQL restore can leave the target database in a mixed state — some objects restored, some not — and the user has to inspect it; the app says this at failure time but cannot undo it. A long-running `pg_dump` runs to completion or to the query timeout, with no way to stop it from the UI. |
| Reason | `--single-transaction` would make the restore atomic but makes any dump containing non-transactional statements (`CREATE INDEX CONCURRENTLY`, `VACUUM`, …) fail as a whole, which is a compatibility trade the owner owns; the issue offers both options and option (b) is the one implemented. Cancellation needs a Stop control wired to `CancelOperation` plus child-process kill and temp-file cleanup, which the issue's acceptance allows to be recorded instead of built. |
| Status | Accepted v0.1 |
| Target issue | #244 (implemented: atomic export, restore state stated, cancellation recorded); a cancel affordance is post-v0.1 work, and `--single-transaction` remains an owner decision |
| Safe release-note wording | "Export publishes atomically, so a failed export never leaves a half-written file. A failed PostgreSQL restore can leave the target database partially restored — the app says so when it happens, and the database should be inspected before use. A running backup or restore cannot be cancelled yet." |
| Must not contradict | `0.1.0-release-notes.md` (§Export and Backup, §Known limitations), `provider-capability-matrix.md` (Backup / Restore / CSV-TSV export / Backup-restore cancellation rows), `docs/release/audit-data-integrity.md` (E-1/E-2/E-3), `crates/ui/src/query_view.rs` (export dialog), `crates/infrastructure/src/backup/pg_dump.rs` (restore) |
| Evidence | `docs/release/audit-data-integrity.md` §2 (findings E-1/E-2/E-3) and §3 (partial-failure table); `crates/ui/src/query_view.rs` (`write_file_atomically`, `export_result_to_disk`); `crates/infrastructure/src/backup/pg_dump.rs` (`restore_command`); `docs/release/evidence/v01-runtime/providers/56-export-atomicity-and-restore-state.md` |

## LIM-021: Closing the window on macOS aborts instead of exiting cleanly

| Field | Value |
|---|---|
| Category | platform |
| Actual behavior | On macOS, closing the DB Pro window — red close button, `Cmd+W`/`Cmd+Q`, or any `ViewportCommand::Close` including the capture harness — tears the window down and then raises an uncaught `NSRangeException` (`removeObserver:forKeyPath:context:` from `___NSTouchBarFinderSetNeedsUpdateOnMain_block_invoke_2` inside `NSDisplayCycleFlush`), aborting the process with `-6` / `SIGABRT`. The cause is not DB Pro code: `accesskit_macos`'s `SubclassingAdapter` re-classes `WinitView` back to its superclass on drop, removing the KVO registration AppKit's `NSTouchBarFinder` had made against that subclass, and the deferred invalidation then deregisters an observer it no longer owns. The pinned stack cannot receive the upstream fix: eframe `0.36.2` → egui-winit `0.36.2` → `accesskit_winit ^0.32.0` → `accesskit_macos ^0.26.0` (locked `0.26.3`), while AccessKit/accesskit#835 (commit `451885c1`, 2026-10-23) shipped in `accesskit_macos 0.27.2`, reachable only through eframe `0.37`. Because `eframe::App::save` runs on loop exit and the abort happens one display cycle later, **the crash also discloses persisted state** — the 2026-10-24 readiness run recorded the dark-theme key gone after an aborting quit. |
| User-visible impact | A clean-looking quit ends in a macOS crash report every time on hardware that carries (or emulates) a Touch Bar; anything the user expected the app to have saved on quit may not be on disk. Exit status is non-zero, so the deterministic capture gate reports failing cells for captures that rendered fine. |
| Reason | The fix is an upstream dependency upgrade (`eframe 0.36 → 0.37`) that no code in this repository can substitute for; the interim mitigation (`-[NSApp terminate:]` on the close path, hardware-verified 0/11 crashes on the identical stack) is Objective-C gated behind `#[cfg(target_os = "macos")]`, which this Linux host cannot compile and `ci.yml` never builds (no macOS job). Landing it blind would ship unverifiable unsafe code. |
| Status | Fix before v0.1 |
| Target issue | R-01 in `docs/release/2026-10-25-v0.1.0-audit-and-release-gap.md`; plan `docs/plans/active/macos-shutdown-crash/` (state `BLOCKED` — needs a macOS host with Touch-Bar-class hardware and a macOS compile gate) |
| Safe release-note wording | "On macOS, closing the window can show a system crash dialog after the app has already quit, and the last session's saved state may not be written. Known issue; the fix depends on an accessibility-library update landing in a future eframe release." |
| Must not contradict | `docs/release/2026-10-25-v0.1.0-audit-and-release-gap.md` §1.4 (R-01), `docs/plans/STATUS.md` (R-01 row), `docs/plans/active/macos-shutdown-crash/{PLAN,FINDINGS,VERIFICATION}.md`, `0.1.0-readiness.md` (public gate) |
| Evidence | `docs/release/evidence/readiness-2026-10-24/packaged-runtime.log:11-51` (throw `NSRangeException` … `not registered as an observer`, AppKit frames `:19-20`, `libc++abi: terminating due to uncaught exception` `:51`); `observations.json:37` (`window_close`: `process_exit -6`, `SIGABRT`) and `:38` (after the abnormal close the previous dark-theme selection was gone); `docs/plans/STATUS.md:196` ("6/12 latest release capture processes ended in AppKit NSTouchBar SIGABRT after rendering"), `:135` ("`exit=-6` cells are the post-capture macOS TouchBar crash"); upstream AccessKit/accesskit#835 and `adapters/macos/CHANGELOG.md` 0.27.2; dependency ranges verified from the crates.io sparse index on 2026-10-26 |

## LIM-022: The macOS qualification host cannot produce exact 1440×900 / 1920×1080 captures

| Field | Value |
|---|---|
| Category | platform |
| Actual behavior | `AGENTS.md` requires runtime captures at 1280×800, 1440×900 and 1920×1080. The deterministic harness pins the viewport from `DB_PRO_WINDOW_SIZE`, but on the macOS host used for the 2026-10-13 / 2026-10-24 qualification runs, requests for 1440×900 and 1920×1080 both render at **838 logical px of height** — measured from the surviving artifacts: 154 files at `2880×1676` (= 1440×838 @2x) and 135 at `3840×1676` (= 1920×838 @2x), many named `*-host-capped*` / `*-clamped*`. The same harness on Linux/Xvfb returns the requested size exactly: 29 files at `1440×900` and 13 at `1920×1080` (`docs/plans/active/qa-keyboard-correctness-wave/captures/`, setup at `VERIFICATION.md:50-52`). So this is a host display constraint, **not** application behaviour; the exact mechanism (menu bar vs Dock vs screen bounds) is still unproven, and 900→838 / 1080→838 is not a constant delta. |
| User-visible impact | None in the product. The impact is on evidence quality: the 1440/1920 rows of the visual acceptance matrix are host-capped on macOS, so they do not prove layout at those sizes. |
| Reason | Closing it is an infrastructure decision (run the capture matrix on a Linux job), not a code change; a fresh Linux capture run at HEAD also showed the harness needs the app-state directory reset first, because a re-attached saved live connection kept the settle loop from finishing. |
| Status | Accepted v0.1 |
| Target issue | Nhóm 3 of `docs/release/2026-10-25-v0.1.0-audit-and-release-gap.md` (capture matrix on a Linux runner) |
| Safe release-note wording | n/a — evidence-pipeline constraint, not a product behaviour. |
| Must not contradict | `docs/10-egui-native-migration-plan.md` (visual acceptance gate), `docs/plans/STATUS.md` (rows "Native UI Regressions…", "Viewport Select, Complex Picker and Light Chrome Fit"), `docs/release/2026-10-25-v0.1.0-audit-and-release-gap.md` §1.2, `docs/plans/active/macos-shutdown-crash/FINDINGS.md` (F-4) |
| Evidence | PNG IHDR scan of all 665 repository evidence images, 2026-10-26; `docs/plans/_audit-2026-10-13/CAPTURE-EVIDENCE.md` ("1440/1920 effective height still capped at 838"); `docs/plans/active/qa-keyboard-correctness-wave/VERIFICATION.md:50-52` |

## Summary by status

| Status | Count | IDs |
|---|---|---|
| Accepted v0.1 | 16 | LIM-001, LIM-002, LIM-003, LIM-005, LIM-006, LIM-007, LIM-011, LIM-013, LIM-014, LIM-015, LIM-016, LIM-017, LIM-018, LIM-019, LIM-020, LIM-022 |
| Blocked decision | 2 | LIM-009, LIM-010 |
| Deferred v0.2+ | 3 | LIM-004, LIM-008, LIM-012 |
| Fix before v0.1 | 1 | LIM-021 |

## Rules

- This registry tracks decisions dynamically; do not freeze stale assumptions.
- If a limitation is fixed before release, mark superseded with evidence instead of deleting history.
- Any limitation that actually constitutes P0/P1 correctness/security must not be hidden as a harmless known limitation.
- Differentiate unsupported from unqualified/experimental.
