package io.github.iamcaominhtien.kakemono;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

/** The Java API on the repository's samples (tests/data). */
class DocumentTest {
    private static final Path DATA = Path.of(System.getProperty("kakemono.test.data", "../../../tests/data"));
    private static final Path LIGO = DATA.resolve("1602.03837v1.pdf");
    private static final Path DOCX = DATA.resolve("word/sample.docx");

    @Test
    void aPdfIsWrittenAsMarkdownWithItsPictures(@TempDir Path out) throws Exception {
        try (Document doc = Document.open(LIGO, new OpenOptions().jobs(1))) {
            assertEquals("pdf", doc.format());
            assertTrue(doc.pageCount() > 10);
            assertTrue(doc.title().orElseThrow().contains("Gravitational Waves"));
            TextOutput md = doc.toMarkdown();
            assertTrue(md.text().contains("Observation of Gravitational Waves"));
            assertFalse(md.files().isEmpty());
            md.writeTo(out.resolve("paper.md"));
            assertTrue(Files.isRegularFile(out.resolve(md.files().get(0).path())));
            TextOutput inline = doc.toMarkdown(new MarkdownOptions().inlineImages(true));
            assertTrue(inline.files().isEmpty());
            assertTrue(doc.toHtml(new HtmlOptions().mode(Mode.HARD_95)).text().startsWith("<!DOCTYPE html>"));
            assertTrue(doc.toLatex(new LatexOptions().equations(Equations.IMAGE)).text().contains("\\begin{document}"));
            assertTrue(doc.analyze().contains("\"blocks\""));
        }
    }

    @Test
    void chunksHaveTheirSectionAndPages() {
        try (Document doc = Document.open(LIGO, new OpenOptions().jobs(1))) {
            List<Chunk> chunks = doc.toChunks(new ChunkOptions().maxTokens(256));
            assertTrue(chunks.size() > 10);
            assertEquals(0, chunks.get(0).id());
            assertTrue(chunks.stream().allMatch(chunk -> !chunk.text().isEmpty()));
            assertTrue(chunks.stream().anyMatch(chunk -> !chunk.pages().isEmpty()));
            assertTrue(chunks.stream().anyMatch(chunk -> !chunk.section().isEmpty()));
        }
    }

    @Test
    void aWordDocumentFromBytesGivesBinaryOutputs() throws Exception {
        try (Document doc = Document.open(Files.readAllBytes(DOCX))) {
            assertEquals("docx", doc.format());
            assertEquals(0, doc.pageCount());
            assertArrayEquals("PK".getBytes(StandardCharsets.US_ASCII), head(doc.toDocx().data(), 2));
            assertArrayEquals("%PDF".getBytes(StandardCharsets.US_ASCII), head(doc.toPdf().data(), 4));
            byte[] epub = doc.toEpub(new EpubOptions().modified("2026-01-31T12:00:00Z")).data();
            assertArrayEquals("PK".getBytes(StandardCharsets.US_ASCII), head(epub, 2));
        }
    }

    @Test
    void editedContentShowsInEveryOutput(@TempDir Path dir) throws Exception {
        String json;
        try (Document doc = Document.open(DOCX)) {
            json = doc.toJson().text();
        }
        // The title's text, translated.
        assertTrue(json.contains("Convergence of Gradient Descent"));
        String edited = json.replace("Convergence of Gradient Descent", "Đã dịch");
        Path file = dir.resolve("edited.json");
        Files.writeString(file, edited, StandardCharsets.UTF_8);
        try (Document doc = Document.open(DOCX, new OpenOptions().content(file))) {
            assertEquals(List.of(), doc.warnings());
            assertTrue(doc.toMarkdown().text().contains("Đã dịch"), "edited text");
            KakemonoException err = assertThrows(KakemonoException.class,
                    () -> doc.toHtml(new HtmlOptions().mode(Mode.MEDIUM)));
            assertEquals(KakemonoException.Kind.INVALID_ARGUMENT, err.kind());
        }
    }

    @Test
    void errorsHaveAKindAndAMessage() {
        KakemonoException unreadable = assertThrows(KakemonoException.class,
                () -> Document.open("nothing".getBytes(StandardCharsets.US_ASCII)));
        assertEquals(KakemonoException.Kind.DOCUMENT, unreadable.kind());
        try (Document doc = Document.open(DOCX)) {
            KakemonoException scale = assertThrows(KakemonoException.class,
                    () -> doc.toMarkdown(new MarkdownOptions().scale(9)));
            assertEquals(KakemonoException.Kind.INVALID_ARGUMENT, scale.kind());
            assertTrue(scale.getMessage().contains("scale must be between"), scale.getMessage());
            assertThrows(KakemonoException.class, () -> doc.toMarkdown(new MarkdownOptions().scale(Double.NaN)));
            assertThrows(KakemonoException.class, () -> doc.toMarkdown(new MarkdownOptions().imageDir("../up")));
        }
        KakemonoException missing = assertThrows(KakemonoException.class, () -> Document.open(Path.of("/no/such.pdf")));
        assertTrue(missing.getMessage().contains("/no/such.pdf"), missing.getMessage());
        assertThrows(KakemonoException.class, () -> Document.addFont(new byte[] {1, 2, 3}));
        assertTrue(Document.version().matches("\\d+\\.\\d+\\.\\d+.*"));
    }

    @Test
    void aClosedDocumentRefusesCallsAndClosesOnce() {
        Document doc = Document.open(DOCX);
        doc.close();
        doc.close();
        assertThrows(IllegalStateException.class, doc::toMarkdown);
        assertThrows(IllegalStateException.class, doc::pageCount);
    }

    @Test
    void outputsRunFromSeveralThreadsAndCloseWaitsForThem() throws Exception {
        ExecutorService pool = Executors.newFixedThreadPool(4);
        try {
            Document doc = Document.open(LIGO, new OpenOptions().jobs(1));
            List<Future<String>> results = new ArrayList<>();
            for (int at = 0; at < 4; at++) {
                results.add(pool.submit(() -> doc.toMarkdown().text()));
            }
            results.add(pool.submit(() -> doc.toJson().text()));
            // Closing while they run waits for them, then frees the document.
            Future<?> closing = pool.submit(doc::close);
            int done = 0;
            for (Future<String> result : results) {
                try {
                    assertTrue(result.get().length() > 1000);
                    done++;
                } catch (java.util.concurrent.ExecutionException e) {
                    // A call that started after the close sees a closed document.
                    assertTrue(e.getCause() instanceof IllegalStateException, e.toString());
                }
            }
            closing.get();
            assertTrue(done >= 1);
        } finally {
            pool.shutdown();
        }
    }

    @Test
    void optionsAreWrittenAsJson() {
        String json = new OpenOptions().sourceName("a \"quoted\"\nname\u0001\ud800").tables(false).headerRows(2).toJson();
        assertEquals("{\"sourceName\":\"a \\\"quoted\\\"\\nname\\u0001�\",\"tables\":false,\"headerRows\":2}", json);
        assertEquals("{\"mode\":\"hard-95\",\"scale\":2.5}", new HtmlOptions().mode(Mode.HARD_95).scale(2.5).toJson());
    }

    /** The first {@code count} bytes of {@code data}. */
    private static byte[] head(byte[] data, int count) {
        return java.util.Arrays.copyOf(data, count);
    }

    @Test
    void aCancelledCallStopsAndTheDocumentStaysUsable() throws Exception {
        try (Document doc = Document.open(LIGO, new OpenOptions().jobs(1))) {
            CancelToken token = new CancelToken();
            Thread stopper = new Thread(() -> {
                try {
                    Thread.sleep(30);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                }
                token.cancel();
            });
            long started = System.nanoTime();
            stopper.start();
            KakemonoException error = assertThrows(KakemonoException.class, () -> {
                while (true) {
                    doc.toHtml(new HtmlOptions().mode(Mode.HARD).cancel(token));
                }
            });
            stopper.join();
            assertEquals(KakemonoException.Kind.CANCELLED, error.kind(), error.getMessage());
            assertTrue(System.nanoTime() - started < 30_000_000_000L);
            assertTrue(token.isCancelled());
            assertFalse(token.isTimedOut());
            assertEquals(KakemonoException.Kind.CANCELLED,
                    assertThrows(KakemonoException.class, () -> doc.toPdf(new StopOptions().cancel(token))).kind());
            assertTrue(doc.toMarkdown().text().contains("Gravitational"));
        }
    }

    @Test
    void aTimeLimitStopsACall() throws Exception {
        KakemonoException error = assertThrows(KakemonoException.class,
                () -> Document.open(LIGO, new OpenOptions().jobs(1).timeout(Duration.ofMillis(1))));
        assertEquals(KakemonoException.Kind.TIMED_OUT, error.kind(), error.getMessage());
        CancelToken expired = CancelToken.withTimeout(Duration.ofMillis(1));
        Thread.sleep(5);
        assertTrue(expired.isTimedOut());
        try (Document doc = Document.open(DOCX)) {
            assertEquals(KakemonoException.Kind.TIMED_OUT, assertThrows(KakemonoException.class,
                    () -> doc.toChunks(new ChunkOptions().cancel(expired))).kind());
            // A child stops with its parent; cancelling a child leaves the parent.
            CancelToken parent = new CancelToken();
            CancelToken child = parent.child(Duration.ofMinutes(1));
            child.cancel();
            assertFalse(parent.isCancelled());
            assertTrue(doc.analyze(new StopOptions().cancel(parent).timeout(Duration.ofMinutes(1))).startsWith("{"));
            parent.cancel();
            assertTrue(parent.child(null).isCancelled());
            assertEquals(KakemonoException.Kind.INVALID_ARGUMENT, assertThrows(KakemonoException.class,
                    () -> new JsonOptions().timeout(Duration.ZERO)).kind());
        }
    }
}
