package io.github.iamcaominhtien.kakemono;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

/** Options of reading and analysing a document, kept for every output. */
public final class OpenOptions extends Options<OpenOptions> {
    /** Options with kakemono's defaults. */
    public OpenOptions() {}

    /** The name recorded as the document's source (by default: the file name, the address, {@code arXiv:<id>}, nothing for bytes). */
    public OpenOptions sourceName(String sourceName) {
        return set("sourceName", sourceName);
    }

    /** Rebuild tables as real tables when their grid is clear (default true). */
    public OpenOptions tables(boolean tables) {
        return set("tables", tables);
    }

    /** Worker threads for one document, 0 for one per core (default 0). A server converting several documents at once should use 1 or 2. */
    public OpenOptions jobs(int jobs) {
        return set("jobs", jobs);
    }

    /** How hard PNG pictures are compressed (default BALANCED). */
    public OpenOptions pngCompression(PngCompression pngCompression) {
        return set("pngCompression", pngCompression == null ? null : pngCompression.value());
    }

    /** An edited copy of the JSON of {@link Document#toJson} for the same document (its text): every output shows its text. */
    public OpenOptions content(String content) {
        return set("content", content);
    }

    /** The folder a relative {@code replace_image} of the content is read from. */
    public OpenOptions contentDir(String contentDir) {
        return set("contentDir", contentDir);
    }

    /** The header rows of every table of an Excel workbook, given instead of found. */
    public OpenOptions headerRows(int headerRows) {
        return set("headerRows", headerRows);
    }

    /**
     * The edited JSON in the file {@code json}: its text, and its folder for a relative {@code
     * replace_image}.
     *
     * @throws UncheckedIOException when the file cannot be read
     */
    public OpenOptions content(Path json) {
        try {
            content(Files.readString(json, StandardCharsets.UTF_8));
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
        Path dir = json.toAbsolutePath().getParent();
        return dir == null ? this : contentDir(dir.toString());
    }
}
