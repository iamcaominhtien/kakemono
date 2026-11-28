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

## 1b. Khảo sát nhu cầu (số liệu lấy ngày 2026-11-28)

Lượt tải lấy trực tiếp từ registry: npm (`api.npmjs.org`, 7 ngày), PyPI (`pypistats.org`, 7 ngày),
NuGet (tổng từ trước đến nay), pub.dev (30 ngày), sao GitHub. Số tải bị thổi phồng bởi CI, nên chỉ
dùng để so sánh độ lớn giữa các hệ sinh thái.

| Hệ sinh thái | Tín hiệu | Nhận xét |
| --- | --- | --- |
| Python (đã có) | pypdf 54,8 tr/tuần, pymupdf 42,3 tr, markitdown 6,6 tr, mammoth 4,7 tr, docling 0,91 tr, unstructured 0,57 tr | Thị trường RAG lớn nhất. |
| JS / TS | pdfjs-dist 59,8 tr/tuần, turndown 17,3 tr, mammoth 16,1 tr, pdf-parse 13,0 tr, unpdf 12,6 tr, officeparser 1,8 tr | Lớn ngang Python, nhưng các thư viện chủ yếu cho ra **văn bản thô**: thiếu một công cụ PDF/Word → Markdown/HTML có cấu trúc. Có thêm lợi thế chạy được trong trình duyệt và trên edge (wasm). |
| Java / Kotlin (JVM) | Tika, PDFBox đã có module riêng trong LangChain4j, và Tika có reader riêng trong Spring AI | Thị trường doanh nghiệp. Đã có sẵn chỗ để cắm vào: một `DocumentParser` của LangChain4j. Một JAR phục vụ cả Java và Kotlin phía server. |
| C# / .NET | DocumentFormat.OpenXml 501 tr (tổng), PDFsharp 87,5 tr, itext7 57,8 tr; mảng AI: ManagedCode.MarkItDown 63 nghìn, Kreuzberg 80 nghìn | Thị trường tài liệu lớn, mảng PDF→Markdown cho AI còn nhỏ nhưng đang tăng. |
| Go | pdfcpu 8,9k sao (chỉnh sửa PDF, không trích xuất), docconv 1,8k (văn bản thô), tabula 31 | Người dùng Go thích thư viện thuần Go; cgo là rào cản. Dịch vụ Go thường gọi CLI hoặc HTTP. |
| Flutter / Dart | read_pdf_text 6,3 nghìn/30 ngày, docx_to_text 6,5 nghìn | Rất nhỏ (gói `pdf` 2,3 tr là gói *tạo* PDF). |
| Swift / Kotlin Android | Ứng dụng dùng PDFKit, PdfRenderer để hiển thị; ít khi chuyển đổi ngay trên máy | Ngách: ứng dụng đọc sách, dịch offline. Binary lớn là điểm trừ. |
| C++ | Đã có MuPDF, Poppler, PDFium | Ngách. |

Đối thủ đáng chú ý: **Kreuzberg** (lõi Rust, binding cho khoảng 16 ngôn ngữ) chỉ đạt khoảng 69 nghìn/tuần
trên npm, 50 nghìn/tuần trên PyPI, 80 nghìn tổng trên NuGet. Phủ nhiều ngôn ngữ tự nó không tạo
ra người dùng. Thứ tạo ra người dùng là chất lượng và chỗ cắm vào các framework (LangChain,
LlamaIndex, LangChain4j, Semantic Kernel).

**Kết luận ưu tiên:**
1. **JS/TS** (napi-rs, sau đó wasm): nhu cầu lớn nhất còn trống.
2. **JVM** (một JAR cho Java + Kotlin server) kèm module LangChain4j.
3. **C#**: khi có người dùng hỏi.
4. **Go, Swift, Android, Flutter, C++**: hoãn. Thay vào đó phát hành CLI dựng sẵn và một bản HTTP/Docker
   (hoặc server MCP cho agent AI), dùng được từ mọi ngôn ngữ mà không phải duy trì binding.

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

Bảng dưới đây là lộ trình **đầy đủ** nếu làm mọi ngôn ngữ. Theo khảo sát ở mục 1b, nên làm
theo thứ tự: bước 0, bước 2 (JS/TS), bước 6 (wasm), rồi JVM ở bước 3 (chỉ Kotlin/Java). Các bước
còn lại để sau.

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
- Số tải: https://api.npmjs.org/downloads/point/last-week/<gói>, https://pypistats.org/api/packages/<gói>/recent,
  https://azuresearch-usnc.nuget.org/query, https://pub.dev/api/packages/<gói>/score
- Kreuzberg: https://docs.rs/crate/kreuzberg/latest
- LangChain4j parser modules: https://central.sonatype.com/artifact/dev.langchain4j/langchain4j-document-parser-apache-tika
