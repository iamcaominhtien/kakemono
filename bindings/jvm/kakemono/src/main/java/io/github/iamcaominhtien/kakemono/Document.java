package io.github.iamcaominhtien.kakemono;

import java.lang.ref.Cleaner;
import java.lang.ref.Reference;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;
import java.util.Optional;
import java.util.concurrent.locks.ReentrantReadWriteLock;
import java.util.function.LongFunction;

/**
 * A PDF, Word document ({@code .docx}, {@code .doc}), Excel workbook ({@code .xlsx}), EPUB book
 * or web page, read and analysed once: every output written from it shares that analysis.
 *
 * <pre>{@code
 * try (Document doc = Document.open(Path.of("paper.pdf"))) {
 *     TextOutput md = doc.toMarkdown();
 *     md.writeTo(Path.of("out/paper.md"));          // and its pictures, in out/images/
 *     List<Chunk> chunks = doc.toChunks(new ChunkOptions().maxTokens(512));
 * }
 * }</pre>
 *
 * <p>A document can be written from several threads at once. The work runs on a thread of
 * kakemono's own with a large stack, so any thread may call it; the calling thread waits, and a
 * virtual thread stays pinned to its carrier meanwhile (a native call), so run conversions on
 * platform threads (a bounded pool) when there are many at once. Close it to free its memory at
 * once (a document left open is freed once it is unreachable). Methods throw {@link
 * KakemonoException} on error, and {@link IllegalStateException} once the document is closed.
 */
public final class Document implements AutoCloseable {
    private static final Cleaner CLEANER = Cleaner.create();

    /**
     * The native handle and its lock: what the cleaner frees. It holds no reference to the
     * {@link Document}, so that the document can become unreachable.
     */
    private static final class State implements Runnable {
        private final ReentrantReadWriteLock lock = new ReentrantReadWriteLock();
        private long handle;

        State(long handle) {
            this.handle = handle;
        }

        /** Runs {@code call} with the handle, which stays valid until it returns. */
        <T> T with(LongFunction<T> call) {
            lock.readLock().lock();
            try {
                if (handle == 0) {
                    throw new IllegalStateException("the document is closed");
                }
                return call.apply(handle);
            } finally {
                lock.readLock().unlock();
            }
        }

        /** Frees the handle once no call runs on it. */
        @Override
        public void run() {
            lock.writeLock().lock();
            try {
                if (handle != 0) {
                    Native.free(handle);
                    handle = 0;
                }
            } finally {
                lock.writeLock().unlock();
            }
        }
    }

    private final State state;
    private final Cleaner.Cleanable cleanable;
    private final String format;

    private Document(long handle) {
        state = new State(handle);
        cleanable = CLEANER.register(this, state);
        format = state.with(Native::format);
    }

    /**
     * Runs {@code call} with the handle. The fence keeps this document reachable until the
     * call returns: otherwise, in {@code Document.open(data).toMarkdown()}, the garbage collector
     * could clean it while the call runs.
     */
    private <T> T call(LongFunction<T> call) {
        try {
            return state.with(call);
        } finally {
            Reference.reachabilityFence(this);
        }
    }

    /** The JSON of {@code options}, or null. */
    private static String json(Options<?> options) {
        return options == null ? null : options.toJson();
    }

    /** Reads and analyses the bytes of a document. */
    public static Document open(byte[] data) {
        return open(data, null);
    }

    /** Reads and analyses the bytes of a document with {@code options}. */
    public static Document open(byte[] data, OpenOptions options) {
        return new Document(Native.openData(data, json(options)));
    }

    /** Reads and analyses the file at {@code path} (a web page with its pictures next to it). */
    public static Document open(Path path) {
        return open(path, null);
    }

    /** Reads and analyses the file at {@code path} with {@code options}. */
    public static Document open(Path path, OpenOptions options) {
        return new Document(Native.openPath(path.toString(), json(options)));
    }

    /** Fetches, reads and analyses the document at the {@code http(s)} address {@code url}. */
    public static Document openUrl(String url, OpenOptions options) {
        return new Document(Native.openUrl(url, json(options)));
    }

    /**
     * Fetches (through the on-disk cache), reads and analyses the PDF of an arXiv paper: an id
     * such as {@code 1706.03762}, or an arXiv URL.
     */
    public static Document openArxiv(String id, OpenOptions options) {
        return new Document(Native.openArxiv(id, json(options)));
    }

    /** The input format: {@code pdf}, {@code docx}, {@code doc}, {@code xlsx}, {@code epub} or {@code html}. */
    public String format() {
        return format;
    }

    /** The number of pages of a PDF (0 for the formats that have none). */
    public int pageCount() {
        return call(Native::pageCount);
    }

    /** The title of the document, when it has one. */
    public Optional<String> title() {
        return Optional.ofNullable(call(Native::title)).map(Document::utf8);
    }

    /** The warnings of applying the edited content: blocks that kept their own text. */
    public List<String> warnings() {
        return List.of(call(Native::warnings));
    }

    /** UTF-8 text. */
    private static String utf8(byte[] text) {
        return new String(text, StandardCharsets.UTF_8);
    }

    /** Renders the document as {@code target}: {@code { text, data, paths, files }}. */
    private Object[] render(String target, Options<?> options) {
        String json = json(options);
        return call(handle -> Native.render(handle, target, json));
    }

    /** The picture files of a rendered output. */
    private static List<OutputFile> files(Object[] out) {
        String[] paths = (String[]) out[2];
        byte[][] data = (byte[][]) out[3];
        List<OutputFile> files = new ArrayList<>(paths.length);
        for (int at = 0; at < paths.length; at++) {
            files.add(new OutputFile(paths[at], data[at]));
        }
        return Collections.unmodifiableList(files);
    }

    /** A text output. */
    private TextOutput text(String target, Options<?> options) {
        Object[] out = render(target, options);
        return new TextOutput(utf8((byte[]) out[0]), files(out));
    }

    /** A binary output. */
    private BinaryOutput binary(String target, Options<?> options) {
        Object[] out = render(target, options);
        return new BinaryOutput((byte[]) out[1], files(out));
    }

    /** An HTML page, friendly mode, pictures inline. */
    public TextOutput toHtml() {
        return toHtml(null);
    }

    /** An HTML page. */
    public TextOutput toHtml(HtmlOptions options) {
        return text("html", options);
    }

    /** Markdown, with its pictures as files in {@code images/}. */
    public TextOutput toMarkdown() {
        return toMarkdown(null);
    }

    /** Markdown and its picture files. */
    public TextOutput toMarkdown(MarkdownOptions options) {
        return text("markdown", options);
    }

    /** The editable JSON: edit its text, then open the document again with it as content. */
    public TextOutput toJson() {
        return toJson(null);
    }

    /** The editable JSON and its picture files. */
    public TextOutput toJson(JsonOptions options) {
        return text("json", options);
    }

    /** LaTeX source and its picture files (in {@code images/}). */
    public TextOutput toLatex() {
        return toLatex(null);
    }

    /** LaTeX source and its picture files. */
    public TextOutput toLatex(LatexOptions options) {
        return text("latex", options);
    }

    /** Typst source and its picture files (in {@code images/}). */
    public TextOutput toTypst() {
        return toTypst(null);
    }

    /** Typst source and its picture files. */
    public TextOutput toTypst(TypstOptions options) {
        return text("typst", options);
    }

    /** A Word document ({@code .docx}). */
    public BinaryOutput toDocx() {
        return toDocx(null);
    }

    /** A Word document; its pictures are files too when {@code imageDir} is given. */
    public BinaryOutput toDocx(DocxOptions options) {
        return binary("docx", options);
    }

    /** An EPUB 3 book. */
    public BinaryOutput toEpub() {
        return toEpub(null);
    }

    /** An EPUB 3 book. */
    public BinaryOutput toEpub(EpubOptions options) {
        return binary("epub", options);
    }

    /** A Word document, workbook or EPUB book laid out in pages, as a PDF (an error for a PDF). */
    public BinaryOutput toPdf() {
        return binary("pdf", null);
    }

    /** Chunks for retrieval (RAG) of at most 512 tokens. */
    public List<Chunk> toChunks() {
        return toChunks(null);
    }

    /** Chunks for retrieval (RAG), cut at sentence ends and headed by their section. */
    public List<Chunk> toChunks(ChunkOptions options) {
        String json = json(options);
        Object[] items = call(handle -> Native.chunks(handle, json));
        List<Chunk> chunks = new ArrayList<>(items.length);
        for (Object item : items) {
            Object[] fields = (Object[]) item;
            chunks.add(new Chunk(
                    (Integer) fields[0],
                    utf8((byte[]) fields[1]),
                    (Integer) fields[2],
                    Optional.ofNullable((byte[]) fields[3]).map(Document::utf8),
                    List.of((String[]) fields[4]),
                    boxed((int[]) fields[5]),
                    boxed((int[]) fields[6])));
        }
        return Collections.unmodifiableList(chunks);
    }

    /** {@code values} as a list. */
    private static List<Integer> boxed(int[] values) {
        return Arrays.stream(values).boxed().toList();
    }

    /** The structure of the document (its blocks in reading order) as JSON text. */
    public String analyze() {
        return utf8((byte[]) render("structure", null)[0]);
    }

    /** Frees the document's memory. Waits for the calls running on it; closing twice does nothing. */
    @Override
    public void close() {
        cleanable.clean();
    }

    /** The version of kakemono. */
    public static String version() {
        return Native.version();
    }

    /**
     * Adds a TrueType or OpenType font (or a collection) to those documents are laid out and SVG
     * text drawn with, for the documents converted after it: for containers without fonts.
     * Returns its number of faces.
     */
    public static int addFont(byte[] data) {
        return Native.addFont(data);
    }
}
