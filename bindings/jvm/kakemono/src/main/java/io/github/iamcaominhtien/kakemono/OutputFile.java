package io.github.iamcaominhtien.kakemono;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;

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

    /** Writes every file of {@code files} under {@code folder}. */
    static void writeAll(List<OutputFile> files, Path folder) throws IOException {
        for (OutputFile file : files) {
            file.writeTo(folder);
        }
    }
}
