package io.github.iamcaominhtien.kakemono;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.PosixFilePermissions;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Locale;

/**
 * The native methods of kakemono, and the loading of its JNI library.
 *
 * <p>The library is found, in order:
 * <ol>
 *   <li>at the path of the system property {@code kakemono.library.path};
 *   <li>on the class path, at {@code /io/github/iamcaominhtien/kakemono/native/<platform>/<file>}
 *       (the artifact {@code io.github.iamcaominhtien:kakemono-native} with the platform as its
 *       classifier holds it), copied once to a cache folder named by its content
 *       ({@code kakemono.cache.dir}, by default the user's cache), since a library cannot be
 *       loaded from inside a JAR;
 *   <li>by {@link System#loadLibrary} ({@code kakemono_jni} on {@code java.library.path}).
 * </ol>
 * The platform is {@code linux-x86_64}, {@code linux-aarch64}, {@code linux-musl-x86_64},
 * {@code linux-musl-aarch64}, {@code macos-x86_64}, {@code macos-aarch64},
 * {@code windows-x86_64} or {@code windows-aarch64}.
 */
final class Native {
    /** The version of the native methods this class declares. */
    private static final int ABI_VERSION = 2;

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

    /**
     * Whether this JVM runs on musl (Alpine): its own libraries are mapped from musl's loader.
     * (A file {@code /lib/ld-musl-*} alone is not enough: Debian can install musl beside glibc.)
     */
    private static boolean isMusl() {
        try {
            String maps = Files.readString(Paths.get("/proc/self/maps"));
            return maps.contains("/ld-musl-") || maps.contains("/libc.musl-");
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
     * Copies the library to {@code <cache>/kakemono-<sha256>/<platform>/<file>}, named by its
     * content: a file already there is loaded only when its bytes are the same, and the folder
     * is the user's own ({@code rwx------} where the file system has POSIX permissions). A copy
     * is written next to it, then renamed into place, so that processes starting together never
     * load a half-written file; when the rename fails (Windows, a library another process has
     * loaded), the copy itself is loaded.
     */
    private static Path extract(InputStream in, String platform, String file) throws IOException {
        byte[] data = in.readAllBytes();
        String digest = sha256(data);
        Path dir = cacheDir().resolve("kakemono-" + digest.substring(0, 32)).resolve(platform);
        Path target = dir.resolve(file);
        if (Files.isRegularFile(target) && sha256(Files.readAllBytes(target)).equals(digest)) {
            return target;
        }
        createPrivateDirectories(dir);
        Path partial = Files.createTempFile(dir, file, ".part");
        Files.write(partial, data);
        return moveIntoPlace(partial, target) ? target : partial;
    }

    /** Renames {@code partial} to {@code target}, atomically where it can: whether it could. */
    private static boolean moveIntoPlace(Path partial, Path target) {
        try {
            Files.move(partial, target, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING);
            return true;
        } catch (AtomicMoveNotSupportedException e) {
            return moveReplacing(partial, target);
        } catch (IOException e) {
            return false;
        }
    }

    /** Renames {@code partial} to {@code target}, replacing it: whether it could. */
    private static boolean moveReplacing(Path partial, Path target) {
        try {
            Files.move(partial, target, StandardCopyOption.REPLACE_EXISTING);
            return true;
        } catch (IOException e) {
            return false;
        }
    }

    /**
     * The cache folder: {@code kakemono.cache.dir}, else the user's cache ({@code
     * $XDG_CACHE_HOME}, {@code ~/.cache}, {@code %LOCALAPPDATA%}), else the temporary folder.
     */
    private static Path cacheDir() {
        String dir = System.getProperty("kakemono.cache.dir", "");
        if (!dir.isEmpty()) {
            return Paths.get(dir);
        }
        String xdg = System.getenv("XDG_CACHE_HOME");
        if (xdg != null && !xdg.isEmpty()) {
            return Paths.get(xdg);
        }
        String local = System.getenv("LOCALAPPDATA");
        if (local != null && !local.isEmpty()) {
            return Paths.get(local);
        }
        String home = System.getProperty("user.home", "");
        if (!home.isEmpty() && Files.isDirectory(Paths.get(home))) {
            return Paths.get(home, ".cache");
        }
        return Paths.get(System.getProperty("java.io.tmpdir"));
    }

    /** Makes {@code dir} and its parents, readable and writable by this user only. */
    private static void createPrivateDirectories(Path dir) throws IOException {
        if (Files.isDirectory(dir)) {
            return;
        }
        try {
            Files.createDirectories(dir, PosixFilePermissions.asFileAttribute(PosixFilePermissions.fromString("rwx------")));
        } catch (UnsupportedOperationException e) {
            Files.createDirectories(dir);
        }
    }

    /** The SHA-256 of {@code data}, in hexadecimal. */
    private static String sha256(byte[] data) {
        try {
            byte[] hash = MessageDigest.getInstance("SHA-256").digest(data);
            StringBuilder hex = new StringBuilder();
            for (byte b : hash) {
                hex.append(String.format("%02x", b));
            }
            return hex.toString();
        } catch (NoSuchAlgorithmException e) {
            throw new IllegalStateException(e);
        }
    }

    static native int abiVersion();

    static native String version();

    static native int addFont(byte[] data);

    // The calls that convert take the native token {@code cancel} (0 for none) and a time limit
    // {@code timeoutMs} (0 for none).

    static native long openData(byte[] data, String options, long cancel, double timeoutMs);

    static native long openPath(String path, String options, long cancel, double timeoutMs);

    static native long openUrl(String url, String options, long cancel, double timeoutMs);

    static native long openArxiv(String id, String options, long cancel, double timeoutMs);

    static native void free(long handle);

    static native String format(long handle);

    static native int pageCount(long handle);

    static native byte[] title(long handle);

    static native String[] warnings(long handle);

    /** {@code { byte[] text (UTF-8) or null, byte[] data or null, String[] paths, byte[][] files }}. */
    static native Object[] render(long handle, String target, String options, long cancel, double timeoutMs);

    /** For each chunk: {@code { Integer id, byte[] text, Integer tokens, byte[] title or null,
     * String[] section, int[] blocks, int[] pages }}. */
    static native Object[] chunks(long handle, String options, long cancel, double timeoutMs);

    /** A new native token: a child of {@code parent} unless it is 0, timing out after {@code timeoutMs} unless it is 0. */
    static native long cancelNew(long parent, double timeoutMs);

    static native void cancelCancel(long token);

    /** 0, 4 (cancelled) or 5 (timed out). */
    static native int cancelState(long token);

    static native void cancelFree(long token);
}
