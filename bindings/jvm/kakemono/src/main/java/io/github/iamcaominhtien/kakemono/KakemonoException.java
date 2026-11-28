package io.github.iamcaominhtien.kakemono;

/** An error of kakemono: a wrong option, a document it cannot read or write, or a bug. */
public class KakemonoException extends RuntimeException {
    private static final long serialVersionUID = 1L;

    /** What kind of error it is. */
    public enum Kind {
        /** A wrong argument or option (an unknown mode, a scale out of range). */
        INVALID_ARGUMENT,
        /** The document cannot be read, fetched or written so. */
        DOCUMENT,
        /** A bug of kakemono. */
        INTERNAL
    }

    private final Kind kind;

    /** An error of {@code kind} with {@code message}. */
    public KakemonoException(String message, Kind kind) {
        super(message);
        this.kind = kind;
    }

    /** Called by the JNI library: {@code code} is 1, 2 or 3, as {@link Kind} orders them. */
    KakemonoException(String message, int code) {
        this(message, code == 1 ? Kind.INVALID_ARGUMENT : code == 2 ? Kind.DOCUMENT : Kind.INTERNAL);
    }

    /** The kind of error. */
    public Kind kind() {
        return kind;
    }
}
