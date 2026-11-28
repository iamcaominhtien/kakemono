package io.github.iamcaominhtien.kakemono.langchain4j;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import dev.langchain4j.data.document.Document;
import dev.langchain4j.data.segment.TextSegment;
import io.github.iamcaominhtien.kakemono.ChunkOptions;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import org.junit.jupiter.api.Test;

/** The LangChain4j parser and segments on the repository's samples. */
class KakemonoDocumentParserTest {
    private static final Path DATA = Path.of(System.getProperty("kakemono.test.data", "../../../tests/data"));

    @Test
    void aPdfIsParsedAsMarkdownWithMetadata() throws Exception {
        try (InputStream in = Files.newInputStream(DATA.resolve("1602.03837v1.pdf"))) {
            Document doc = new KakemonoDocumentParser().parse(in);
            assertTrue(doc.text().contains("Observation of Gravitational Waves"));
            assertFalse(doc.text().contains("data:image/"), "pictures are left out");
            assertEquals("pdf", doc.metadata().getString("format"));
            assertTrue(doc.metadata().getInteger("page_count") > 10);
            assertTrue(doc.metadata().getString("title").contains("Gravitational"));
        }
    }

    @Test
    void segmentsKeepTheirSectionAndPages() throws Exception {
        List<TextSegment> segments =
                KakemonoSegments.of(DATA.resolve("1602.03837v1.pdf"), new ChunkOptions().maxTokens(256));
        assertTrue(segments.size() > 10);
        TextSegment first = segments.get(0);
        assertEquals("1602.03837v1.pdf", first.metadata().getString("source"));
        assertEquals(0, first.metadata().getInteger("chunk_id"));
        assertTrue(segments.stream().anyMatch(segment -> segment.metadata().getString("section") != null));
        assertTrue(segments.stream().anyMatch(segment -> segment.metadata().getString("pages") != null));
    }

    @Test
    void aWordDocumentIsParsed() throws Exception {
        try (InputStream in = Files.newInputStream(DATA.resolve("word/sample.docx"))) {
            Document doc = new KakemonoDocumentParser().parse(in);
            assertEquals("docx", doc.metadata().getString("format"));
            assertTrue(doc.text().startsWith("# "));
        }
    }
}
