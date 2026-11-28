package io.github.iamcaominhtien.kakemono;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

/**
 * A binary output (Word document, EPUB book, PDF) and its picture files.
 *
 * @param data the bytes
 * @param files the picture files to write next to it (a Word document's, with {@code imageDir})
 */
public record BinaryOutput(byte[] data, List<OutputFile> files) {
    /** Writes the bytes to {@code file} and the pictures next to it. */
    public void writeTo(Path file) throws IOException {
        Path folder = file.toAbsolutePath().getParent();
        Files.createDirectories(folder);
        Files.write(file, data);
        OutputFile.writeAll(files, folder);
    }
}
