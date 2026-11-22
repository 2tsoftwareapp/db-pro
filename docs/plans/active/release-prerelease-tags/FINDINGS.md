# Findings

## P1 — Tag release candidate hợp lệ bị chặn trước build

Tại baseline `7ac4d7facd7f62bdd30361580eb5d6e636481b70`, `.github/workflows/release.yml:144` so sánh phần sau chữ `v` của tag nguyên chuỗi với `app_version`. Tag `v0.1.0-rc.1` vì vậy không khớp `0.1.0`.

Bằng chứng runtime CI: run `41814490913` được kích hoạt bởi tag, checkout đúng commit `b55556f2c414f225323dd3ae3f32dd025cc0aa33`, nhưng bước `Record immutable release identity` thất bại. GitHub check-run annotation ghi chính xác: `Release tag v0.1.0-rc.1 does not match source app version 0.1.0`. Các job preflight/build/package/checksum bị bỏ qua. Run dispatch `41810843078` trên cùng commit thành công vì không đi qua kiểm tra tag; không được dùng nó làm bằng chứng cho đường tag-push.

**Ảnh hưởng:** luồng phát hành RC bằng tag không thể tạo artifact. Không phải lỗi dữ liệu database; P1 theo tiêu chí chặn một luồng release chính.

**Phạm vi sửa nhỏ nhất:** chỉ công nhận thêm hậu tố `-rc.N` khớp version nguồn. Không đổi `app_version` thành chuỗi tag vì các bước đóng gói/checksum đang dùng tên archive theo version nguồn. Không nới lỏng thành chấp nhận mọi hậu tố.

## Các mục không sửa lặp

- Không có GitHub issue mở ở cả repo hiện tại và repo `truongnat/db-pro` được tham chiếu trong tài liệu.
- PR #565 vẫn Draft và chứa phần sửa macOS/MySQL riêng; không sao chép thay đổi từ nhánh đó vào phiên này.
- Nhiều mục UAT cũ đã có implementation/regression test trong cây hiện tại (ví dụ UAT-10 tại `crates/app/src/connection/logic.rs` và `tests/logic_tests.rs`). Checklist cũ không tự động là bằng chứng lỗi còn tồn tại.
