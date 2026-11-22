# Sửa kiểm tra tag release candidate

- **Trạng thái:** REVIEW (chờ CI trên nhánh/PR; không đánh dấu COMPLETED khi chưa có kết quả)
- **Nguồn phát hiện:** Release Build run `41814490913`, job `126521710345`; không có GitHub issue đang mở tại thời điểm kiểm tra 2026-11-13.
- **Baseline:** `7ac4d7facd7f62bdd30361580eb5d6e636481b70`.
- **Nhánh thực hiện:** `arena/28a10f4b-db-pro` (nhánh phiên làm việc hiện tại).

## Mục tiêu và phạm vi

Cho phép tag `v<version>-rc.N` đi qua bước xác minh version khi `N` là số nguyên dương và version khớp `crates/native-app/Cargo.toml`. Giữ nguyên kiểm tra từ chối tag khác phiên bản; giữ tên archive, checksum và `app_version` theo version nguồn. Tách phần đọc/kiểm tra thành script Python chuẩn để có regression test chạy trong CI.

## Ngoài phạm vi

- Không tạo/xóa/di chuyển tag, không phát hành bản public, không merge PR #565.
- Không thay đổi pin Rust, chính sách signing/notarization hay các bước build/package.
- Không mở rộng hậu tố tag sang mọi loại prerelease (`alpha`, `beta`, build metadata); tag khớp nguyên chuỗi với version Cargo có sẵn prerelease/build metadata vẫn được giữ tương thích.
- Không sửa các finding UI/provider chưa tái hiện hoặc đã có PR khác xử lý.

## Tiêu chí chấp nhận

1. Tái hiện lỗi `v0.1.0-rc.1` với version nguồn `0.1.0` trước khi sửa.
2. Tag ổn định và RC đúng phiên bản được chấp nhận; RC sai phiên bản hoặc sai định dạng bị từ chối.
3. Nhánh/commit không cần tag; manifest lỗi không tạo stdout/output giả.
4. Workflow gọi cùng script được test; `app_version` vẫn là version nguồn. Resolver thuộc workflow hiện tại được giữ ngoài workspace trước khi checkout candidate cũ, nên không đòi tag lịch sử phải chứa script mới.
5. Chạy test Python, kiểm tra cú pháp shell/YAML và các gate hiện có; ghi rõ gate Rust/CI nào chưa chạy.

## Provider/UI

Không áp dụng: thay đổi chỉ ở bước xác định danh tính release, không đụng đường thực thi SQL hoặc UI. Không dùng kết quả này để đóng các gate macOS/Windows của PR #565.
