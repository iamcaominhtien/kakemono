# Thư viện cho các ngôn ngữ khác

Ngày: 2026-11-28.

## 1. Quyết định

| Ngôn ngữ | Trạng thái | Cách làm |
| --- | --- | --- |
| Python | Đã có (`python/`, PyO3, PyPI) | Giữ nguyên |
| JavaScript / TypeScript | **Làm, ưu tiên 1** | napi-rs (Node, Bun), sau đó wasm (trình duyệt, edge); một gói npm, `.d.ts` tự sinh |
| Java / Kotlin (JVM) | **Làm, ưu tiên 2** | Một JAR trên Maven Central; kèm module `DocumentParser` cho LangChain4j |
| C# / .NET | **Làm, ưu tiên 3** | Gói NuGet, P/Invoke trên C ABI |
| Go, Swift, Android, Flutter/Dart, C++ | Không làm binding | Dùng CLI dựng sẵn, bản HTTP/Docker hoặc server MCP |

## 2. Vì sao chọn bốn ngôn ngữ này: khảo sát nhu cầu

Lượt tải lấy trực tiếp từ registry ngày 2026-11-28: npm (`api.npmjs.org`, 7 ngày), PyPI
(`pypistats.org`, 7 ngày), NuGet (tổng), pub.dev (30 ngày), sao GitHub. Số tải bị thổi phồng bởi
CI, nên chỉ dùng để so độ lớn giữa các hệ sinh thái.

| Hệ sinh thái | Tín hiệu | Nhận xét |
| --- | --- | --- |
| Python | pypdf 54,8 tr/tuần, pymupdf 42,3 tr, markitdown 6,6 tr, mammoth 4,7 tr, docling 0,91 tr, unstructured 0,57 tr | Thị trường RAG lớn nhất. |
| JS / TS | pdfjs-dist 59,8 tr/tuần, turndown 17,3 tr, mammoth 16,1 tr, pdf-parse 13,0 tr, unpdf 12,6 tr, officeparser 1,8 tr | Lớn ngang Python, nhưng các thư viện chủ yếu cho ra văn bản thô: chuyển PDF/Word thành Markdown/HTML có cấu trúc vẫn còn trống. Wasm còn chạy được ngay trong trình duyệt. |
| Java / Kotlin | Tika, PDFBox có module riêng trong LangChain4j; Spring AI có reader Tika | Doanh nghiệp. Có sẵn chỗ để cắm vào framework. |
| C# / .NET | DocumentFormat.OpenXml 501 tr (tổng), PDFsharp 87,5 tr, itext7 57,8 tr; mảng AI: ManagedCode.MarkItDown 63 nghìn, Kreuzberg 80 nghìn | Thị trường tài liệu lớn; mảng PDF→Markdown cho AI còn nhỏ nhưng đang tăng. |
| Go | pdfcpu 8,9k sao (chỉnh sửa PDF, không trích xuất), docconv 1,8k (văn bản thô) | Người dùng thích thuần Go; cgo là rào cản. |
| Flutter / Dart | read_pdf_text 6,3 nghìn/30 ngày, docx_to_text 6,5 nghìn | Rất nhỏ. |
| Swift / Android / C++ | Ứng dụng dùng PDFKit, PdfRenderer để hiển thị; C++ có MuPDF, Poppler, PDFium | Ngách; binary lớn là điểm trừ trên di động. |

## 3. Đối thủ gần nhất: Kreuzberg

Kreuzberg có lõi Rust và binding cho khoảng 16 ngôn ngữ, nhưng chỉ đạt khoảng 69 nghìn lượt
tải/tuần trên npm, 50 nghìn/tuần trên PyPI và 80 nghìn tổng trên NuGet. Phủ nhiều ngôn ngữ tự nó
không tạo ra người dùng.

So sánh trực tiếp trên 8 bài test của repo (`scripts/fetch-test-pdfs.sh`): Kreuzberg 4.10.4 (gói
PyPI, cấu hình mặc định, `output_format="markdown"`) với kakemono 0.20.0
(`convert … -o x.md`). Các chỉ số là đếm dòng trong file Markdown.

| Bài | Tiêu đề (kk / kz) | Bảng Markdown (kk / kz) | Công thức `$…$` (kk / kz) | Hình (kk / kz) |
| --- | --- | --- | --- | --- |
| 1412.6980 Adam | 21 / **532** | 0 / 0 | 112 / 0 | 8 / 0 |
| 1506.01497 Faster R-CNN | 19 / 51 | 12 / 1 | 32 / 0 | 6 / 0 |
| 1512.03385 ResNet | 22 / 56 | 13 / 1 | 38 / 0 | 9 / 0 |
| 1602.03837 LIGO | 13 / 36 | 1 / 1 | 17 / 0 | 5 / 0 |
| 1706.03762 Attention | 26 / 74 | 4 / 0 | 53 / 0 | 5 / 0 |
| 1810.04805 BERT | 33 / 95 | 8 / 0 | 18 / 0 | 5 / 0 |
| 2010.11929 ViT | 35 / 109 | 8 / 0 | 55 / 0 | 15 / 0 |
| 2303.08774 GPT-4 | 138 / 246 | 8 / 3 | 24 / 0 | 41 / 0 |

Ví dụ trong bài Attention:
- Kreuzberg biến dòng tác giả, công thức và chú thích hình thành tiêu đề:
  `### Attention(Q, K, V ) = softmax(QKT √ dk )V (1)`, `### Figure 1: …`.
- Bảng 1 của Kreuzberg thành một tiêu đề cộng một dòng chữ:
  `### Layer Type Complexity per Layer … Self-Attention O(n 2` rồi `- d) O(1) O(1) Recurrent …`.
- kakemono cho ra bảng Markdown đúng 4 cột, với `$O(n^{2}\cdot d)$`, cùng tham chiếu `[Table 1](#tab-1)`
  và `[38](#ref-38)`.

Tốc độ ngang nhau (khoảng 0,2 đến 1 giây mỗi bài, trên máy này). Phép so sánh chỉ dùng cấu hình mặc định của Kreuzberg; chưa thử
OCR hay các tuỳ chọn layout của nó. Trên bài báo khoa học, chất lượng là điểm khác biệt của
kakemono. Nên đưa phép so sánh này vào README của từng gói.

## 4. Kiến trúc

```
src/ (crate kakemono)
ffi/            crate "kakemono-ffi", không thuộc workspace (như python/)
                façade Document / Options / Output { text|bytes, files, warnings } / lỗi
                + extern "C" (cbindgen → kakemono.h)
bindings/node/  napi-rs trên façade        → npm (JS, TS)
bindings/wasm/  wasm-bindgen trên façade   → cùng gói npm, export "browser"
bindings/jvm/   JNI (crate jni) + lớp Java → Maven Central (Java, Kotlin)
bindings/dotnet/ P/Invoke trên kakemono.h (csbindgen) → NuGet
```

- Façade lấy đúng hình dạng của binding Python: một đối tượng `Document` (đọc và phân tích một lần),
  tuỳ chọn là chuỗi/số/bool, kết quả là `bytes`/`str` kèm `(đường dẫn, bytes)` của hình. Có thể
  chuyển `python/src/lib.rs` sang dùng chung façade.
- **Không cần UniFFI** khi chỉ có 4 ngôn ngữ này: UniFFI mạnh nhất cho Kotlin và Swift, còn C# là
  generator của bên thứ ba và phải ghim phiên bản.
- **JVM bằng JNI, không dùng FFM**: LangChain4j và Spring AI chạy từ Java 17, mà FFM (Panama) cần
  Java 22+. Bề mặt API nhỏ (khoảng 15 hàm) nên JNI viết tay vẫn gọn. Kotlin gọi lớp Java trực
  tiếp. JAR chứa native cho linux x64/arm64, macOS x64/arm64, Windows x64.
- **C#**: P/Invoke là cách chuẩn của .NET; `csbindgen` sinh mã từ các hàm `extern "C"`. Native
  nằm trong `runtimes/<rid>/native/`.
- Chung: hàm chạy lâu được export bất đồng bộ (`Promise` / `CompletableFuture` / `Task`). Panic
  được chặn bằng `catch_unwind` ở ranh giới C ABI (napi-rs tự làm). Lỗi gộp thành một kiểu
  `KakemonoError`. Mọi gói lấy phiên bản theo tag release, như `scripts/set-python-version.py`.

## 5. Thay đổi cần trong lõi

Trên một bản sao, `cargo check --lib --target wasm32-unknown-unknown --no-default-features` chỉ
vướng:
- `ring` (C, qua `ureq` → `rustls`).
- Sau khi tách `ureq`, `mimalloc`, `libmimalloc-sys`: còn đúng 10 lỗi, ở `src/source/web.rs`,
  `src/source/arxiv.rs` (tải mạng) và `src/html/mod.rs:237, 283` (file-URL).

Toàn bộ phần đọc PDF, Word, Excel, EPUB, HTML và layout đều biên dịch được. Việc cần làm (vài
chục dòng; mặc định không đổi, nên các snapshot phải giữ nguyên):
1. Feature `net`, bật mặc định, bọc `ureq`; khi tắt, `fetch` trả lỗi rõ ràng.
2. `cfg` cho hai chỗ file-URL.
3. `mimalloc` chỉ là dependency ngoài wasm (chỉ `main.rs` dùng).
4. Font: `src/layout/fonts.rs::font_dirs()` và `backend/render.rs::svg_fonts()` không có font nào
   trên wasm, nên cần API truyền font (bytes) vào. Ảnh hưởng: PDF xuất từ Word/Excel, chữ trong SVG→PNG.
5. Stack wasm: worker dùng 16 MB; cần `-zstack-size`.
6. CI: thêm bước build `--no-default-features` và một bước `cargo check` cho wasm.

**Đã làm (bước 0):** các mục 1, 2, 3 và 6. `cargo check --lib --target wasm32-unknown-unknown
--no-default-features --features openai-tokenizers` chạy qua; CI kiểm tra nó trong job
`no default features + wasm`.

Còn phải làm khi tới bước wasm: feature `tokenizers` (Hugging Face) cần `getrandom` với cấu hình
`wasm_js` (đặt trong crate `bindings/wasm`, không phải trong lõi); kích thước `.wasm`; mục 4 và 5.

## 6. Lộ trình

| Bước | Việc | Kết quả |
| --- | --- | --- |
| 0 | Mục 5, bước 1–3 và 6 | Lõi build được không có mạng, build được cho wasm |
| 1 | `kakemono-ffi`: façade + C ABI | Nền cho JVM và .NET |
| 2 | napi-rs, CI prebuild (5 nền tảng desktop/server) | npm: JS, TS |
| 3 | wasm-bindgen, font truyền vào (mục 5, bước 4–5) | npm, chạy trong trình duyệt |
| 4 | JNI + lớp Java + module LangChain4j | Maven: Java, Kotlin |
| 5 | P/Invoke + NuGet | C# |
| Song song | Phát hành binary CLI, image Docker có HTTP, server MCP | Mọi ngôn ngữ khác |

Phần tốn công nhất là CI dựng và phát hành binary cho từng nền tảng, cùng phần chạy thử mẫu
(LIGO, `sample.docx`) trên mỗi ngôn ngữ.

## 7. Nguồn

- Số tải: `https://api.npmjs.org/downloads/point/last-week/<gói>`,
  `https://pypistats.org/api/packages/<gói>/recent`, `https://azuresearch-usnc.nuget.org/query`,
  `https://pub.dev/api/packages/<gói>/score`
- Kreuzberg: https://docs.rs/crate/kreuzberg/latest
- LangChain4j: https://central.sonatype.com/artifact/dev.langchain4j/langchain4j-document-parser-apache-tika
- napi-rs v3: https://napi.rs/blog/announce-v3, https://napi.rs/docs/concepts/webassembly
- UniFFI: https://github.com/mozilla/uniffi-rs
