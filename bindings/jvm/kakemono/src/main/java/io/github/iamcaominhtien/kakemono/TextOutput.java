package io.github.iamcaominhtien.kakemono;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

/**
 * A text output (HTML, Markdown, JSON, LaTeX, Typst) and its picture files.
 *
 * @param text the text
 * @param files the picture files to write next to it (empty when the pictures are inline)
 */
public record TextOutput(String text, List<OutputFile> files) {
    /** Writes the text to {@code file} (UTF-8) and the pictures next to it. */
    public void writeTo(Path file) throws IOException {
        Path folder = file.toAbsolutePath().getParent();
        Files.createDirectories(folder);
        Files.writeString(file, text, StandardCharsets.UTF_8);
        OutputFile.writeAll(files, folder);
    }
}
