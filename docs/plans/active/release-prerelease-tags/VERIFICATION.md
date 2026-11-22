# Kiểm chứng

## Bằng chứng đã thu thập

- Ngày kiểm tra: 2026-11-13.
- Baseline: `7ac4d7facd7f62bdd30361580eb5d6e636481b70`.
- `gh issue list --state open --limit 100 ...`: 0 issue.
- `gh pr list --state open ...`: 1 PR, #565, Draft; check release thất bại.
- `gh api repos/2tsoftwareapp/db-pro/check-runs/126521710345/annotations`: lỗi version/tag như FINDINGS.
- `gh run view 41814490913 --log-failed`: không tải được log (`EOF` từ máy chủ log); dùng annotation API và trạng thái từng bước, không suy diễn nội dung log.

## Tái hiện trước sửa

Chạy bước shell `Record immutable release identity` lấy từ YAML workflow baseline bằng PyYAML, với manifest fixture `0.1.0` và `git` fixture trả SHA cố định:

| SOURCE_KIND | SOURCE_REF | Exit thực tế |
|---|---|---:|
| tag | `refs/tags/v0.1.0` | 0 |
| tag | `refs/tags/v0.1.0-rc.1` | **1 — lỗi cần sửa** |
| tag | `refs/tags/v0.2.0-rc.1` | 1 |
| branch | `refs/heads/main` | 0 |

Lần chạy unit test đầu tiên có 6 PASS / 1 FAIL do test tự đặt trường hợp `--tag ""` là hợp lệ dù workflow không truyền tham số đó cho nhánh/commit. Đã sửa test theo hợp đồng thật: không truyền `--tag` cho nhánh/commit; truyền chuỗi rỗng là tag lỗi. Không đổi production để chấp nhận tag rỗng.

## Gate cục bộ sau sửa

- `python3 -m unittest discover -s scripts/release/tests -v`: **8 tests PASS / 0 FAIL**, exit 0; có subtest cho RC đúng/sai, số 0/đệm zero, hậu tố lạ, tag ổn định, version Cargo có prerelease/build metadata, manifest thiếu/sai và cây nguồn thật.
- `python3 scripts/release/resolve-version.py crates/native-app/Cargo.toml --tag refs/tags/v0.1.0-rc.1`: stdout `0.1.0`, exit 0.
- Harness PyYAML chạy cả bước `Preserve workflow version resolver` và `Record immutable release identity` thật: **5/5 trường hợp PASS**. Candidate fixture không có thư mục `scripts/`, chứng minh resolver của workflow hiện tại vẫn kiểm tra được tag lịch sử. Tag đúng trả `app_version=0.1.0`; tag sai không ghi output.
- PyYAML parse + `bash -n` từng bước shell: `release.yml` **25 bước**, `ci.yml` **13 bước**, exit 0. Không chạy PowerShell steps.
- `python3 tools/cc-scan.py scripts/release/resolve-version.py scripts/release/tests/test_resolve_version.py`: **0 error / 0 warning**, exit 0.
- `python3 tools/cc-scan.py . --fail-on error`: exit 0; **0 error mới**, 126 warning nợ hiện có, 23 finding baseline được giữ nguyên. Không cập nhật baseline.
- `bash .skills/clean-code/scripts/clean-code-scan.sh rust --diff --ratchet --ci`: exit 0; 0 file Rust thay đổi, 0 fail/warn.
- `bash scripts/check-ui-architecture.sh`: PASS, exit 0.
- `bash scripts/verify-artifact-policy.sh`: PASS, exit 0.
- `git diff --check`: exit 0.

## Gate chưa chạy / giới hạn

- `cargo`, `rustc`, `rustup` không có trong môi trường cục bộ: chưa chạy fmt/check/clippy/test/build Rust. Không tuyên bố workspace PASS.
- Chưa cài được `actionlint-py`: wheel build tải binary qua GitHub release redirect bị chặn (`TLS/SSL connection has been closed`). Đã dùng YAML parse + `bash -n`, không coi đó là thay thế tương đương mọi rule actionlint.
- Chưa dispatch lại Release Build và chưa tạo/xóa/di chuyển tag hoặc phát hành public. Nội dung workflow tại tag cũ không tự thay đổi khi sửa nhánh này.
- Không có thay đổi UI/SQL: provider matrix PostgreSQL/SQLite và screenshot không áp dụng. Không dùng kết quả này để đóng gate macOS/Windows của PR #565.
