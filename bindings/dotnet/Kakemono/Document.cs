using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Kakemono
{
    /// <summary>
    /// A PDF, Word document (<c>.docx</c>, <c>.doc</c>), Excel workbook (<c>.xlsx</c>), EPUB book or
    /// web page, read and analysed once: every output written from it shares that analysis.
    /// </summary>
    /// <example>
    /// <code>
    /// using var doc = Document.Open("paper.pdf");
    /// doc.ToMarkdown().WriteTo("out/paper.md");          // and its pictures, in out/images/
    /// IReadOnlyList&lt;Chunk&gt; chunks = doc.ToChunks(new ChunkOptions { MaxTokens = 512 });
    /// </code>
    /// </example>
    /// <remarks>
    /// A document can be written from several threads at once. The work runs on a thread of
    /// kakemono's own with a large stack, so any thread may call it; the <c>Async</c> methods run
    /// it on a thread of their own (it cannot be cancelled once started). Dispose it to free its memory
    /// at once: calls running then finish first. Methods throw <see cref="KakemonoException"/> on
    /// error, and <see cref="ObjectDisposedException"/> once the document is disposed.
    /// </remarks>
    public sealed class Document : IDisposable
    {
        private static readonly Lazy<bool> Loaded = new Lazy<bool>(() =>
        {
            uint abi;
            try
            {
                abi = Native.kk_abi_version();
            }
            catch (DllNotFoundException e)
            {
                throw new DllNotFoundException(
                    "kakemono's native library (kakemono_ffi) was not found for this platform: the Kakemono "
                        + "package has it for linux-x64, linux-arm64, linux-musl-x64, linux-musl-arm64, osx-x64, "
                        + "osx-arm64, win-x64 and win-arm64 (in runtimes/<rid>/native); .NET Framework is not supported",
                    e);
            }
            if (abi != Native.AbiVersion)
            {
                throw new InvalidOperationException(
                    $"the native library kakemono_ffi has ABI {abi}, this package needs {Native.AbiVersion}");
            }
            return true;
        });

        private readonly DocumentHandle handle;

        private Document(DocumentHandle handle, IntPtr err)
        {
            Native.Check(err);
            this.handle = handle;
            Format = Native.Text(Native.kk_document_format(handle, out UIntPtr len), len) ?? "";
        }

        /// <summary>Checks the native library once, before the first call.</summary>
        private static void EnsureLoaded() => _ = Loaded.Value;

        /// <summary>Reads and analyses the bytes of a document.</summary>
        public static Document Open(byte[] data, OpenOptions? options = null)
        {
            EnsureLoaded();
            DocumentHandle doc = Native.kk_open_data(data, (UIntPtr)data.Length, Options.ToJson(options), out IntPtr err);
            return new Document(doc, err);
        }

        /// <summary>Reads and analyses the file at <paramref name="path"/> (a web page with its pictures next to it).</summary>
        public static Document Open(string path, OpenOptions? options = null)
        {
            EnsureLoaded();
            DocumentHandle doc = Native.kk_open_path(Native.Utf8(path), Options.ToJson(options), out IntPtr err);
            return new Document(doc, err);
        }

        /// <summary>Fetches, reads and analyses the document at the <c>http(s)</c> address <paramref name="url"/>.</summary>
        public static Document OpenUrl(string url, OpenOptions? options = null)
        {
            EnsureLoaded();
            DocumentHandle doc = Native.kk_open_url(Native.Utf8(url), Options.ToJson(options), out IntPtr err);
            return new Document(doc, err);
        }

        /// <summary>
        /// Fetches (through the on-disk cache), reads and analyses the PDF of an arXiv paper: an id
        /// such as <c>1706.03762</c>, or an arXiv URL.
        /// </summary>
        public static Document OpenArxiv(string id, OpenOptions? options = null)
        {
            EnsureLoaded();
            DocumentHandle doc = Native.kk_open_arxiv(Native.Utf8(id), Options.ToJson(options), out IntPtr err);
            return new Document(doc, err);
        }

        /// <summary>Reads and analyses the file at <paramref name="path"/> on the thread pool.</summary>
        public static Task<Document> OpenAsync(string path, OpenOptions? options = null, CancellationToken cancellationToken = default) =>
            Run(() => Open(path, options), cancellationToken);

        /// <summary>The input format: <c>pdf</c>, <c>docx</c>, <c>doc</c>, <c>xlsx</c>, <c>epub</c> or <c>html</c>.</summary>
        public string Format { get; }

        /// <summary>The number of pages of a PDF (0 for the formats that have none).</summary>
        public int PageCount => checked((int)Native.kk_document_page_count(Live()).ToUInt64());

        /// <summary>The title of the document, or null.</summary>
        public string? Title => Native.Text(Native.kk_document_title(Live(), out UIntPtr len), len);

        /// <summary>The warnings of applying the edited content: blocks that kept their own text.</summary>
        public IReadOnlyList<string> Warnings
        {
            get
            {
                DocumentHandle doc = Live();
                ulong count = Native.kk_document_warning_count(doc).ToUInt64();
                var warnings = new List<string>();
                for (ulong at = 0; at < count; at++)
                {
                    warnings.Add(Native.Text(Native.kk_document_warning(doc, (UIntPtr)at, out UIntPtr len), len) ?? "");
                }
                return warnings;
            }
        }

        /// <summary>The handle, or an <see cref="ObjectDisposedException"/>.</summary>
        private DocumentHandle Live()
        {
            if (handle.IsClosed)
            {
                throw new ObjectDisposedException(nameof(Document));
            }
            return handle;
        }

        /// <summary>Renders the document as <paramref name="target"/> and reads the output.</summary>
        private T Render<T>(string target, Options? options, Func<IntPtr, IReadOnlyList<OutputFile>, T> read)
        {
            // P/Invoke holds a reference to the SafeHandle during the call: disposing the
            // document meanwhile frees it once the call returns.
            IntPtr output = Native.kk_render(Live(), Native.Utf8(target), Options.ToJson(options), out IntPtr err);
            Native.Check(err);
            try
            {
                ulong count = Native.kk_output_file_count(output).ToUInt64();
                var files = new List<OutputFile>();
                for (ulong at = 0; at < count; at++)
                {
                    string path = Native.Text(Native.kk_output_file_path(output, (UIntPtr)at, out UIntPtr pathLen), pathLen) ?? "";
                    byte[] data = Native.Bytes(Native.kk_output_file_data(output, (UIntPtr)at, out UIntPtr dataLen), dataLen);
                    files.Add(new OutputFile(path, data));
                }
                return read(output, files);
            }
            finally
            {
                Native.kk_output_free(output);
            }
        }

        /// <summary>A text output.</summary>
        private TextOutput Text(string target, Options? options) =>
            Render(target, options, (output, files) =>
                new TextOutput(Native.Text(Native.kk_output_text(output, out UIntPtr len), len) ?? "", files));

        /// <summary>A binary output.</summary>
        private BinaryOutput Binary(string target, Options? options) =>
            Render(target, options, (output, files) =>
                new BinaryOutput(Native.Bytes(Native.kk_output_data(output, out UIntPtr len), len), files));

        /// <summary>An HTML page (friendly mode, pictures inline by default).</summary>
        public TextOutput ToHtml(HtmlOptions? options = null) => Text("html", options);

        /// <summary>Markdown and its picture files (in <c>images/</c> by default).</summary>
        public TextOutput ToMarkdown(MarkdownOptions? options = null) => Text("markdown", options);

        /// <summary>The editable JSON and its picture files: edit its text, then open the document again with it as content.</summary>
        public TextOutput ToJson(JsonOptions? options = null) => Text("json", options);

        /// <summary>LaTeX source and its picture files.</summary>
        public TextOutput ToLatex(LatexOptions? options = null) => Text("latex", options);

        /// <summary>Typst source and its picture files.</summary>
        public TextOutput ToTypst(TypstOptions? options = null) => Text("typst", options);

        /// <summary>A Word document; its pictures are files too when ImageDir is given.</summary>
        public BinaryOutput ToDocx(DocxOptions? options = null) => Binary("docx", options);

        /// <summary>An EPUB 3 book.</summary>
        public BinaryOutput ToEpub(EpubOptions? options = null) => Binary("epub", options);

        /// <summary>A Word document, workbook or EPUB book laid out in pages, as a PDF (an error for a PDF).</summary>
        public BinaryOutput ToPdf() => Binary("pdf", null);

        /// <summary>Chunks for retrieval (RAG), cut at sentence ends and headed by their section.</summary>
        public IReadOnlyList<Chunk> ToChunks(ChunkOptions? options = null)
        {
            string lines = Text("chunks", options).Text;
            var chunks = new List<Chunk>();
            foreach (string line in lines.Split('\n'))
            {
                if (line.Length > 0)
                {
                    chunks.Add(Chunk.Read(line));
                }
            }
            return chunks;
        }

        /// <summary>The structure of the document (its blocks in reading order) as JSON text.</summary>
        public string Analyze() => Text("structure", null).Text;

        /// <summary>
        /// Runs <paramref name="work"/> on a thread of its own (a conversion takes seconds: it
        /// would hold a thread of the pool). <paramref name="cancellationToken"/> only cancels it
        /// before it starts: a conversion cannot be stopped.
        /// </summary>
        private static Task<T> Run<T>(Func<T> work, CancellationToken cancellationToken) =>
            Task.Factory.StartNew(work, cancellationToken, TaskCreationOptions.LongRunning, TaskScheduler.Default);

        /// <summary><see cref="ToHtml"/> on a thread of its own.</summary>
        public Task<TextOutput> ToHtmlAsync(HtmlOptions? options = null, CancellationToken cancellationToken = default) =>
            Run(() => ToHtml(options), cancellationToken);

        /// <summary><see cref="ToMarkdown"/> on a thread of its own.</summary>
        public Task<TextOutput> ToMarkdownAsync(MarkdownOptions? options = null, CancellationToken cancellationToken = default) =>
            Run(() => ToMarkdown(options), cancellationToken);

        /// <summary><see cref="ToJson"/> on a thread of its own.</summary>
        public Task<TextOutput> ToJsonAsync(JsonOptions? options = null, CancellationToken cancellationToken = default) =>
            Run(() => ToJson(options), cancellationToken);

        /// <summary><see cref="ToLatex"/> on a thread of its own.</summary>
        public Task<TextOutput> ToLatexAsync(LatexOptions? options = null, CancellationToken cancellationToken = default) =>
            Run(() => ToLatex(options), cancellationToken);

        /// <summary><see cref="ToTypst"/> on a thread of its own.</summary>
        public Task<TextOutput> ToTypstAsync(TypstOptions? options = null, CancellationToken cancellationToken = default) =>
            Run(() => ToTypst(options), cancellationToken);

        /// <summary><see cref="ToDocx"/> on a thread of its own.</summary>
        public Task<BinaryOutput> ToDocxAsync(DocxOptions? options = null, CancellationToken cancellationToken = default) =>
            Run(() => ToDocx(options), cancellationToken);

        /// <summary><see cref="ToEpub"/> on a thread of its own.</summary>
        public Task<BinaryOutput> ToEpubAsync(EpubOptions? options = null, CancellationToken cancellationToken = default) =>
            Run(() => ToEpub(options), cancellationToken);

        /// <summary><see cref="ToChunks"/> on a thread of its own.</summary>
        public Task<IReadOnlyList<Chunk>> ToChunksAsync(ChunkOptions? options = null, CancellationToken cancellationToken = default) =>
            Run(() => ToChunks(options), cancellationToken);

        /// <summary><see cref="ToPdf"/> on a thread of its own.</summary>
        public Task<BinaryOutput> ToPdfAsync(CancellationToken cancellationToken = default) =>
            Run(ToPdf, cancellationToken);

        /// <summary>Frees the document's memory, once the calls running on it have returned.</summary>
        public void Dispose() => handle.Dispose();

        /// <summary>The version of kakemono.</summary>
        public static string Version
        {
            get
            {
                EnsureLoaded();
                return Native.Text(Native.kk_version(out UIntPtr len), len) ?? "";
            }
        }

        /// <summary>
        /// Adds a TrueType or OpenType font (or a collection) to those documents are laid out and SVG
        /// text drawn with, for the documents converted after it, in the whole process: for
        /// containers without fonts. Returns its number of faces.
        /// </summary>
        public static int AddFont(byte[] data)
        {
            EnsureLoaded();
            UIntPtr faces = Native.kk_add_font(data, (UIntPtr)data.Length, out IntPtr err);
            Native.Check(err);
            return checked((int)faces.ToUInt64());
        }
    }
}
