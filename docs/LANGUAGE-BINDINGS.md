# Hỗ trợ thư viện cho các ngôn ngữ khác: nghiên cứu

Ngày: 2026-11-28 · Phạm vi: Kotlin, Java, JavaScript, TypeScript, C++, C#, Swift, Go, Flutter/Dart.

## 1. Điểm xuất phát

- Thư viện Rust (`src/lib.rs`) đã có API ổn định: `Paper` (đọc) → `Analysis` → `to_html / to_docx /
  to_markdown / to_latex / to_typst / to_epub / to_json / to_chunks / to_pdf`.
- Binding Python (`python/src/lib.rs`, ~530 dòng, PyO3) đã chứng minh một **hình dạng API mỏng, dễ
  chuyển ngữ**: một đối tượng `Document` (đọc + phân tích một lần), tuỳ chọn là chuỗi/số/bool, kết
  quả là `bytes` / `str` kèm danh sách `(đường dẫn, bytes)` của hình ảnh. Không có callback, không có
  kiểu dữ liệu lồng phức tạp. Đây là hình dạng lý tưởng cho FFI: mọi ngôn ngữ nên dùng lại đúng
  hình dạng này.

## 2. Đề xuất kiến trúc

```
src/ (crate kakemono)
  └── ffi/  crate mới "kakemono-ffi" (không thuộc workspace, như python/)
        ├── API façade: Document, Options, Output { text|bytes, files }, KakemonoError
        ├── #[uniffi::export]      → Kotlin, Java, Swift, C#, Go
        └── extern "C" + cbindgen  → kakemono.h (C) + kakemono.hpp (C++ RAII)
  bindings/node/   napi-rs          → JavaScript + TypeScript (.d.ts tự sinh), Node/Bun/Deno
  bindings/wasm/   wasm-bindgen     → trình duyệt (cùng gói npm, export khác)
  bindings/dart/   flutter_rust_bridge v2 → Flutter (Android, iOS, desktop, web)
```

Nguyên tắc: **một façade Rust duy nhất** chứa logic chuyển tuỳ chọn (giống `parse_mode`,
`check_scale`… của Python), các binding chỉ là lớp vỏ sinh tự động. Có thể chuyển dần
`python/src/lib.rs` sang dùng chung façade này.

## 3. Từng ngôn ngữ

| Ngôn ngữ | Công cụ đề xuất | Độ chín | Phân phối | Ghi chú |
| --- | --- | --- | --- | --- |
| **Kotlin** | UniFFI (chính thức) | Cao (Mozilla dùng trong Firefox Android) | Maven Central: AAR (Android) + JAR có native cho linux/mac/win | Dùng JNA. Có hỗ trợ `async` → coroutine. |
| **Java** | Dùng chung JAR Kotlin của UniFFI | Trung bình | Cùng artifact Maven | Gọi được từ Java (cần kotlin-stdlib); API mang phong cách Kotlin. Phương án B: Java 22+ FFM (Panama) + `jextract` trên `kakemono.h`, không cần JNA/Kotlin. |
| **Swift** | UniFFI (chính thức) | Cao | SwiftPM `binaryTarget` với XCFramework (iOS, iOS-sim, macOS) | Hàm dài nên export `async` để không chặn main thread. |
| **C#** | `uniffi-bindgen-cs` (NordSecurity) | Trung bình | NuGet, native trong `runtimes/<rid>/native/` | Phải ghim phiên bản UniFFI mà generator hỗ trợ. Phương án B: `csbindgen` (Cysharp) trên C ABI. |
| **Go** | `uniffi-bindgen-go` (NordSecurity) | Trung bình | Go module + thư viện tĩnh dựng sẵn | cgo bắt buộc; Go không có cơ chế tải binary như npm/NuGet → phải commit `.a` theo nền tảng hoặc hướng dẫn tự build. Đây là ngôn ngữ phân phối khó nhất. |
| **JavaScript / TypeScript** | napi-rs v3 (Node) + wasm-bindgen (trình duyệt) | Cao | npm: gói chính + `optionalDependencies` theo nền tảng; WASM làm phương án dự phòng | JS và TS là **một gói**: napi-rs sinh `.d.ts` tự động. napi-rs v3 còn build được `wasm32-wasip1-threads`. |
| **C++** | C ABI (`extern "C"`) + `cbindgen` + header C++17 tự viết | Cao | Archive release (`.h`, `.hpp`, `.so/.dylib/.dll/.a`); sau này vcpkg/Conan | `cxx` hợp với dự án nhúng C++ vào Rust hơn là phân phối thư viện. C ABI này cũng là nền cho phương án B của Java/C#. |
| **Flutter/Dart** | flutter_rust_bridge v2 (2.12 ổn định, 2.14 beta) | Cao | pub.dev, plugin tải binary dựng sẵn (Cargokit) | Tự chạy hàm nặng ngoài UI isolate. `uniffi-dart` còn non → không chọn. |

**Tóm lại: 4 công cụ phủ 9 ngôn ngữ**: UniFFI (Kotlin, Java, Swift, C#, Go), napi-rs/wasm-bindgen
(JS, TS), cbindgen (C++), flutter_rust_bridge (Dart).

## 4. Rào cản kỹ thuật đã kiểm tra trong mã

### 4.1 WebAssembly (trình duyệt, cũng là đường cho Flutter web)

Đã thử `cargo check --lib --target wasm32-unknown-unknown --no-default-features` (bản sao, không
sửa repo):

- Lỗi đầu tiên: `ring` (C, qua `ureq` → `rustls`) không biên dịch cho wasm.
- Sau khi tách `ureq`, `mimalloc`, `libmimalloc-sys` khỏi wasm: **chỉ còn 10 lỗi**, tất cả nằm ở:
  - `src/source/web.rs`, `src/source/arxiv.rs`: dùng `ureq` (tải trang web, arXiv).
  - `src/html/mod.rs:237, 283`: `Url::from_directory_path` / `to_file_path` không có trên wasm.
- Toàn bộ phần đọc PDF (hayro), Word, Excel, EPUB, HTML, layout, krilla, resvg **đều biên dịch được**.

Việc cần làm: một feature `net` (mặc định bật) bọc `ureq`; `cfg` cho 2 chỗ file-URL; chuyển
`mimalloc` sang dependency chỉ của binary (hiện là dependency của cả lib, dù chỉ `main.rs` dùng).
Còn phải kiểm tra:
- Luồng: `common/parallel.rs` chạy trên luồng gọi khi `jobs <= 1`, và `available_parallelism()` trả lỗi
  trên wasm → 1, nên không spawn luồng. Ổn.
- Stack: worker dùng stack 16 MB vì PDF đệ quy sâu; wasm mặc định ~1 MB → cần
  `-C link-arg=-zstack-size=...`.
- Font: xem 4.2. Kích thước file `.wasm` và feature `tokenizers` chưa đo.

### 4.2 Font trên di động và wasm

- `src/layout/fonts.rs::font_dirs()` chỉ biết Windows, macOS và Linux (`/usr/share/fonts`…):
  **thiếu Android (`/system/fonts`) và iOS**. Trên wasm không có thư mục font nào.
- `backend/render.rs::svg_fonts()` gọi `fontdb.load_system_fonts()`: cùng vấn đề.
- Biến `KAKEMONO_FONT_DIRS` đã có, nhưng nên thêm API truyền font (bytes) từ ứng dụng, và thêm
  `/system/fonts` cho Android. Ảnh hưởng: xuất PDF từ Word/Excel và vẽ chữ trong SVG→PNG.

### 4.3 Mạng trên di động

`ureq` dùng `native-certs` (`rustls-native-certs`) để đọc chứng chỉ hệ thống, cách này không chạy
trên Android/iOS. Lựa chọn: `rustls-platform-verifier` hoặc `webpki-roots` cho mobile; hoặc để ứng
dụng tự tải (đã có `Paper::from_page_at(data, url)`) và tắt feature `net`.

### 4.4 Chung cho mọi binding

- Hàm chạy lâu (giây): export bất đồng bộ (UniFFI `async`, napi `AsyncTask`, FRB tự làm) để không
  chặn UI/event loop.
- Panic: UniFFI, napi-rs, FRB đều bắt panic ở ranh giới; C ABI phải tự `catch_unwind`.
- Lỗi: gộp `anyhow::Error` thành một `KakemonoError { message }`, giống `ValueError` của Python.
- Cảnh báo (`warn_all` của Python): trả về `warnings: Vec<String>` trong kết quả thay vì callback.
- Phiên bản: mọi gói theo tag release, như `scripts/set-python-version.py`.

## 5. Lộ trình đề xuất

| Bước | Việc | Mở khoá | Ước lượng |
| --- | --- | --- | --- |
| 0 | Feature `net`, `mimalloc` chỉ cho binary, font Android + API truyền font | wasm, mobile | 1–2 ngày |
| 1 | Crate `kakemono-ffi`: façade + C ABI + `kakemono.h/.hpp` | C++ | 3–4 ngày |
| 2 | napi-rs + `.d.ts`, CI prebuild 6 nền tảng | JS, TS (Node) | 3 ngày |
| 3 | UniFFI trên façade: Kotlin/JVM + Android AAR, Swift XCFramework | Kotlin, Java, Swift | 1 tuần |
| 4 | `uniffi-bindgen-cs`, `uniffi-bindgen-go` | C#, Go | 3–4 ngày |
| 5 | flutter_rust_bridge plugin | Flutter/Dart | 3–4 ngày |
| 6 | wasm-bindgen cho trình duyệt | JS/TS web, Flutter web | 3–5 ngày |

Phần tốn công nhất không phải viết binding mà là **CI dựng và phát hành binary cho từng nền tảng**
(linux x64/arm64, macOS x64/arm64, Windows x64, Android 4 ABI, iOS) và chạy thử test mẫu (LIGO,
`sample.docx`) trên mỗi ngôn ngữ.

## 6. Nguồn

- UniFFI và danh sách binding bên thứ ba: https://github.com/mozilla/uniffi-rs
- napi-rs v3 (WebAssembly): https://napi.rs/blog/announce-v3, https://napi.rs/docs/concepts/webassembly
- flutter_rust_bridge: https://docs.rs/crate/flutter_rust_bridge/latest
