package io.github.iamcaominhtien.kakemono.langchain4j;

import dev.langchain4j.data.document.BlankDocumentException;
import dev.langchain4j.data.document.DocumentParser;
import dev.langchain4j.data.document.Metadata;
import io.github.iamcaominhtien.kakemono.Document;
import io.github.iamcaominhtien.kakemono.MarkdownOptions;
import io.github.iamcaominhtien.kakemono.OpenOptions;
import java.io.IOException;
import java.io.InputStream;
import java.io.UncheckedIOException;

/**
 * A LangChain4j {@link DocumentParser} that reads PDF papers, Word documents ({@code .docx},
 * {@code .doc}), Excel workbooks, EPUB books and web pages with kakemono, as Markdown: headings,
 * lists, tables, formulas ({@code $...$}) and links kept, pictures inline as data URIs or left
 * out.
 *
 * <pre>{@code
 * Document doc = FileSystemDocumentLoader.loadDocument(path, new KakemonoDocumentParser());
 * }</pre>
 *
 * <p>The metadata holds {@code format} ({@code pdf}, {@code docx}, ...), {@code title} when
 * the document has one, and {@code page_count} for a PDF. For chunks that keep their section
 * and pages, see {@link KakemonoSegments}.
 */
public class KakemonoDocumentParser implements DocumentParser {
    private final OpenOptions openOptions;
    private final boolean pictures;

    /** A parser with kakemono's defaults, pictures left out of the text. */
    public KakemonoDocumentParser() {
        this(new OpenOptions(), false);
    }

    /**
     * A parser reading documents with {@code openOptions}; with {@code pictures}, the text keeps
     * the pictures as data URIs (large), otherwise their links are left out.
     */
    public KakemonoDocumentParser(OpenOptions openOptions, boolean pictures) {
        this.openOptions = openOptions;
        this.pictures = pictures;
    }

    @Override
    public dev.langchain4j.data.document.Document parse(InputStream inputStream) {
        byte[] data;
        try {
            data = inputStream.readAllBytes();
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
        try (Document doc = Document.open(data, openOptions)) {
            // Without pictures, they are not even encoded: their files are left out and
            // so are the links to them.
            String text = pictures
                    ? doc.toMarkdown(new MarkdownOptions().inlineImages(true)).text()
                    : withoutPictures(doc.toMarkdown(new MarkdownOptions().imageDir(PICTURES)).text());
            if (text.isBlank()) {
                throw new BlankDocumentException();
            }
            return dev.langchain4j.data.document.Document.from(text, metadata(doc));
        }
    }

    /** The metadata of a document: its format, title and number of pages. */
    static Metadata metadata(Document doc) {
        Metadata metadata = new Metadata();
        metadata.put("format", doc.format());
        doc.title().ifPresent(title -> metadata.put("title", title));
        if (doc.pageCount() > 0) {
            metadata.put("page_count", doc.pageCount());
        }
        return metadata;
    }

    /** The folder of the pictures, whose links are left out. */
    private static final String PICTURES = "kakemono-pictures";

    /** Markdown without the links to its pictures ({@code ![alt](kakemono-pictures/...)}). */
    static String withoutPictures(String markdown) {
        return markdown.replaceAll("!\\[[^\\]]*]\\(" + PICTURES + "/[^)]*\\)", "");
    }
}
