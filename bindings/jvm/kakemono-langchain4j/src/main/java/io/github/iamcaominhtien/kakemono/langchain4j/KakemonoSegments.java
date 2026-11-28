package io.github.iamcaominhtien.kakemono.langchain4j;

import dev.langchain4j.data.document.Metadata;
import dev.langchain4j.data.segment.TextSegment;
import io.github.iamcaominhtien.kakemono.Chunk;
import io.github.iamcaominhtien.kakemono.ChunkOptions;
import io.github.iamcaominhtien.kakemono.Document;
import io.github.iamcaominhtien.kakemono.OpenOptions;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.stream.Collectors;

/**
 * kakemono's chunks as LangChain4j {@link TextSegment}s, ready to embed: cut at sentence ends,
 * each headed by its section path, tables kept whole when they fit.
 *
 * <pre>{@code
 * List<TextSegment> segments = KakemonoSegments.of(Path.of("paper.pdf"), new ChunkOptions().maxTokens(512));
 * embeddingStore.addAll(embeddingModel.embedAll(segments).content(), segments);
 * }</pre>
 *
 * <p>Each segment's metadata holds the document's ({@code format}, {@code title}, {@code
 * page_count}) and the chunk's: {@code chunk_id}, {@code tokens}, {@code section} (the headings,
 * joined by " > "), {@code pages} (0-based page numbers, joined by ","), and {@code source}
 * (the file name, when read from a file).
 */
public final class KakemonoSegments {
    private KakemonoSegments() {}

    /** The segments of the document in the file {@code path}. */
    public static List<TextSegment> of(Path path, ChunkOptions options) throws IOException {
        if (!Files.isRegularFile(path)) {
            throw new IOException("no file at " + path);
        }
        try (Document doc = Document.open(path)) {
            List<TextSegment> segments = of(doc, options);
            Path name = path.getFileName();
            if (name != null) {
                segments.forEach(segment -> segment.metadata().put("source", name.toString()));
            }
            return segments;
        }
    }

    /** The segments of the document in {@code data}, read with {@code openOptions}. */
    public static List<TextSegment> of(byte[] data, OpenOptions openOptions, ChunkOptions options) {
        try (Document doc = Document.open(data, openOptions)) {
            return of(doc, options);
        }
    }

    /** The segments of an open document. */
    public static List<TextSegment> of(Document doc, ChunkOptions options) {
        Metadata base = KakemonoDocumentParser.metadata(doc);
        List<TextSegment> segments = new ArrayList<>();
        for (Chunk chunk : doc.toChunks(options)) {
            Metadata metadata = base.copy();
            metadata.put("chunk_id", chunk.id());
            metadata.put("tokens", chunk.tokens());
            if (!chunk.section().isEmpty()) {
                metadata.put("section", String.join(" > ", chunk.section()));
            }
            if (!chunk.pages().isEmpty()) {
                metadata.put("pages", chunk.pages().stream().map(String::valueOf).collect(Collectors.joining(",")));
            }
            segments.add(TextSegment.from(chunk.text(), metadata));
        }
        return segments;
    }
}
