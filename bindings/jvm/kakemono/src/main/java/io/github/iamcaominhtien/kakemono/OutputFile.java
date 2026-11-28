package io.github.iamcaominhtien.kakemono;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.List;
import java.util.Objects;

/**
 * A picture file of an output.
 *
 * @param path its path, relative to the folder of the output ({@code images/fig-1.png})
 * @param data its bytes
 */
public record OutputFile(String path, byte[] data) {
    /** Writes the file under {@code folder}, making its folders. */
    public void writeTo(Path folder) throws IOException {
        Path target = folder.resolve(path).normalize();
        if (!target.startsWith(folder.normalize())) {
            throw new IOException("the picture path " + path + " leaves " + folder);
        }
        Path parent = target.getParent();
        if (parent != null) {
            Files.createDirectories(parent);
        }
        Files.write(target, data);
    }

    /** Equal to another file with the same path and bytes. */
    @Override
    public boolean equals(Object other) {
        return other instanceof OutputFile that && path.equals(that.path) && Arrays.equals(data, that.data);
    }

    @Override
    public int hashCode() {
        return Objects.hash(path, Arrays.hashCode(data));
    }

    /** The path and the size of the bytes, not the bytes themselves. */
    @Override
    public String toString() {
        return "OutputFile[path=" + path + ", data=" + data.length + " bytes]";
    }

    /** Writes every file of {@code files} under {@code folder}. */
    static void writeAll(List<OutputFile> files, Path folder) throws IOException {
        for (OutputFile file : files) {
            file.writeTo(folder);
        }
    }
}
