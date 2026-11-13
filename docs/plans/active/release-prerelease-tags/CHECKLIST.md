# Checklist

- [x] Kiểm tra issue/PR đang mở và đối chiếu backlog hiện có.
- [x] Lấy annotation lỗi thật từ GitHub API; phân biệt run tag thất bại với run dispatch thành công.
- [x] Tìm memory release/toolchain; đọc quy ước version và đóng gói.
- [x] Thêm regression test và tái hiện lỗi với hành vi so sánh nguyên chuỗi.
- [x] Sửa kiểm tra tag RC, giữ kiểm tra sai phiên bản.
- [x] Nối script và test vào workflow/CI; giữ resolver ngoài workspace để hỗ trợ candidate lịch sử.
- [x] Chạy gate cục bộ khả dụng và kiểm tra diff; ghi gate Rust bị thiếu công cụ.
- [ ] Chạy CI trên nhánh/PR và ghi kết quả thực tế.
- [x] Ghi bằng chứng, giới hạn và thứ tự xử lý phần tồn đọng.
