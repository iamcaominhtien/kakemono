package io.github.iamcaominhtien.kakemono;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.util.Locale;

/**
 * The native methods of kakemono, and the loading of its JNI library.
 *
 * <p>The library is found, in order:
 * <ol>
 *   <li>at the path of the system property {@code kakemono.library.path};
 *   <li>on the class path, at {@code /io/github/iamcaominhtien/kakemono/native/<platform>/<file>}
 *       (the artifact {@code io.github.iamcaominhtien:kakemono-native} with the platform as its
 *       classifier holds it), copied once to a cache
 *       folder ({@code kakemono.cache.dir}, by default {@code java.io.tmpdir}), since a library
 *       cannot be loaded from inside a JAR;
 *   <li>by {@link System#loadLibrary} ({@code kakemono_jni} on {@code java.library.path}).
 * </ol>
 * The platform is {@code linux-x86_64}, {@code linux-aarch64}, {@code linux-musl-x86_64},
 * {@code linux-musl-aarch64}, {@code macos-x86_64}, {@code macos-aarch64},
 * {@code windows-x86_64} or {@code windows-aarch64}.
 */
final class Native {
    /** The version of the native methods this class declares. */
    private static final int ABI_VERSION = 1;

    static {
        load();
        int abi = abiVersion();
        if (abi != ABI_VERSION) {
            throw new UnsatisfiedLinkError(
                    "kakemono's JNI library has ABI " + abi + ", this JAR needs " + ABI_VERSION);
        }
    }

    private Native() {}

    /** The platform name of this JVM, as the native artifacts are named. */
    static String platform() {
        String os = System.getProperty("os.name", "").toLowerCase(Locale.ROOT);
        String arch = System.getProperty("os.arch", "").toLowerCase(Locale.ROOT);
        String cpu;
        if (arch.equals("amd64") || arch.equals("x86_64") || arch.equals("x64")) {
            cpu = "x86_64";
        } else if (arch.equals("aarch64") || arch.equals("arm64")) {
            cpu = "aarch64";
        } else {
            cpu = arch;
        }
        if (os.startsWith("windows")) {
            return "windows-" + cpu;
        }
        if (os.startsWith("mac") || os.startsWith("darwin")) {
            return "macos-" + cpu;
        }
        if (os.startsWith("linux")) {
            return (isMusl() ? "linux-musl-" : "linux-") + cpu;
        }
        return os.replace(' ', '_') + "-" + cpu;
    }

    /** Whether this Linux uses musl (Alpine), whose libraries differ from glibc's. */
    private static boolean isMusl() {
        try (var files = Files.list(Paths.get("/lib"))) {
            return files.anyMatch(file -> file.getFileName().toString().startsWith("ld-musl-"));
        } catch (IOException | RuntimeException e) {
            return false;
        }
    }

    /** The file name of the JNI library on this platform. */
    private static String fileName(String platform) {
        if (platform.startsWith("windows")) {
            return "kakemono_jni.dll";
        }
        if (platform.startsWith("macos")) {
            return "libkakemono_jni.dylib";
        }
        return "libkakemono_jni.so";
    }

    /** Loads the JNI library (see the class comment). */
    private static void load() {
        String path = System.getProperty("kakemono.library.path", "");
        if (!path.isEmpty()) {
            System.load(Paths.get(path).toAbsolutePath().toString());
            return;
        }
        String platform = platform();
        String file = fileName(platform);
        String resource = "/io/github/iamcaominhtien/kakemono/native/" + platform + "/" + file;
        try (InputStream in = Native.class.getResourceAsStream(resource)) {
            if (in != null) {
                System.load(extract(in, platform, file).toString());
                return;
            }
        } catch (IOException e) {
            throw linkError("cannot copy kakemono's JNI library out of its JAR: " + e, e);
        }
        try {
            System.loadLibrary("kakemono_jni");
        } catch (UnsatisfiedLinkError e) {
            throw linkError(
                    "kakemono's JNI library for " + platform + " is not on the class path: add the"
                            + " dependency io.github.iamcaominhtien:kakemono-native (classifier " + platform
                            + "), or set -Dkakemono.library.path",
                    e);
        }
    }

    /** An {@link UnsatisfiedLinkError} with a cause. */
    private static UnsatisfiedLinkError linkError(String message, Throwable cause) {
        UnsatisfiedLinkError error = new UnsatisfiedLinkError(message);
        error.initCause(cause);
        return error;
    }

    /**
     * Copies the library to {@code <cache>/kakemono-<version>/<platform>/<file>}, once: a copy
     * is written next to it, then renamed into place, so that processes starting together never
     * load a half-written file.
     */
    private static Path extract(InputStream in, String platform, String file) throws IOException {
        String version = Native.class.getPackage().getImplementationVersion();
        Path base = Paths.get(System.getProperty("kakemono.cache.dir", System.getProperty("java.io.tmpdir")));
        Path dir = base.resolve("kakemono-" + (version == null ? "dev" : version)).resolve(platform);
        Path target = dir.resolve(file);
        byte[] data = in.readAllBytes();
        if (Files.isRegularFile(target) && Files.size(target) == data.length) {
            return target;
        }
        Files.createDirectories(dir);
        Path partial = Files.createTempFile(dir, file, ".part");
        try {
            Files.write(partial, data);
            try {
                Files.move(partial, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
            } catch (AtomicMoveNotSupportedException e) {
                Files.move(partial, target, StandardCopyOption.REPLACE_EXISTING);
            }
        } finally {
            Files.deleteIfExists(partial);
        }
        return target;
    }

    static native int abiVersion();

    static native String version();

    static native int addFont(byte[] data);

    static native long openData(byte[] data, String options);

    static native long openPath(String path, String options);

    static native long openUrl(String url, String options);

    static native long openArxiv(String id, String options);

    static native void free(long handle);

    static native String format(long handle);

    static native int pageCount(long handle);

    static native byte[] title(long handle);

    static native String[] warnings(long handle);

    /** {@code { byte[] text (UTF-8) or null, byte[] data or null, String[] paths, byte[][] files }}. */
    static native Object[] render(long handle, String target, String options);

    /** For each chunk: {@code { Integer id, byte[] text, Integer tokens, byte[] title or null,
     * String[] section, int[] blocks, int[] pages }}. */
    static native Object[] chunks(long handle, String options);
}
