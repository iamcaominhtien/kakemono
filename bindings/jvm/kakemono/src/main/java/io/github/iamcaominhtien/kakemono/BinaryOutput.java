package io.github.iamcaominhtien.kakemono;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.List;
import java.util.Objects;

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

    /** Equal to another output with the same bytes and files. */
    @Override
    public boolean equals(Object other) {
        return other instanceof BinaryOutput that && Arrays.equals(data, that.data) && files.equals(that.files);
    }

    @Override
    public int hashCode() {
        return Objects.hash(Arrays.hashCode(data), files);
    }

    /** The size of the bytes and the files, not the bytes themselves. */
    @Override
    public String toString() {
        return "BinaryOutput[data=" + data.length + " bytes, files=" + files + "]";
    }
}
