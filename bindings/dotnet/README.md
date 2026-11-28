# Kakemono for .NET

Convert PDF papers, Word documents (`.docx`, `.doc`), Excel workbooks, EPUB books and web pages to
Markdown, HTML, Word, LaTeX, Typst, EPUB, editable JSON and chunks for RAG, from .NET (8+, and
.NET Standard 2.0 hosts that load `runtimes/<rid>/native`: .NET Framework is not supported). The
work is done by [kakemono](https://github.com/iamcaominhtien/kakemono), a Rust library, through
its C ABI.

```bash
dotnet add package Kakemono
```

```csharp
using Kakemono;

using var doc = Document.Open("paper.pdf");                    // or bytes, OpenUrl, OpenArxiv
doc.ToMarkdown().WriteTo("out/paper.md");                       // and out/images/
IReadOnlyList<Chunk> chunks = doc.ToChunks(new ChunkOptions { MaxTokens = 512 });
byte[] docx = doc.ToDocx().Data;
string html = doc.ToHtml(new HtmlOptions { Mode = Mode.Hard95 }).Text;
var md = await doc.ToMarkdownAsync();                           // on the thread pool
```

A `Document` is read and analysed once; it can be written from several threads at once, and
`Dispose()` frees it once the calls running on it return. Errors are `KakemonoException`s with a
`Kind` (`InvalidArgument`, `Document`, `Internal`). `Jobs` (default: one thread per core) is per
document: a server converting several documents at once should set `new OpenOptions { Jobs = 1 }`.
Every call, sync or `Async`, takes a `CancellationToken`: once it is cancelled, the call stops soon
after (normally within a page) with an `OperationCanceledException`, and the document stays
usable. For a time limit, use `CancelAfter`:

```csharp
using var source = new CancellationTokenSource(TimeSpan.FromSeconds(30));
var md = await doc.ToMarkdownAsync(null, source.Token);
```

Native libraries are included for Linux (x64, arm64; glibc and musl), macOS (x64, arm64) and
Windows (x64, arm64). In a container without fonts, give one with `Document.AddFont(bytes)` before
laying out a Word document or a workbook as a PDF.

Build and test: `cargo build --release -p kakemono-c` in `bindings/`, then `dotnet test` here.
