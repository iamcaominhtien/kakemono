package io.github.iamcaominhtien.kakemono;

import java.time.Duration;
import java.util.LinkedHashMap;
import java.util.Map;

/**
 * Options given to kakemono: each set option is written as JSON for the native library, under
 * the name the other bindings use (camelCase). An option not set takes kakemono's default.
 *
 * @param <S> the options class itself, so that setters can be chained
 */
public abstract class Options<S extends Options<S>> {
    private final Map<String, Object> values = new LinkedHashMap<>();
    private CancelToken cancel;
    private double timeoutMillis;

    Options() {}

    /**
     * Stops the call when {@code token} is cancelled (or times out): it then throws a {@link
     * KakemonoException} of kind {@link KakemonoException.Kind#CANCELLED} (or {@link
     * KakemonoException.Kind#TIMED_OUT}). Null for none.
     */
    public final S cancel(CancelToken token) {
        this.cancel = token;
        return self();
    }

    /**
     * A time limit for the call: past it, the call throws a {@link KakemonoException} of kind
     * {@link KakemonoException.Kind#TIMED_OUT}. Null for none.
     *
     * @throws KakemonoException when {@code limit} is zero or negative
     */
    public final S timeout(Duration limit) {
        this.timeoutMillis = limit == null ? 0 : millis(limit);
        return self();
    }

    /** {@code limit} in milliseconds, which must be positive. */
    static double millis(Duration limit) {
        if (limit.isNegative() || limit.isZero()) {
            throw new KakemonoException("timeout must be positive", KakemonoException.Kind.INVALID_ARGUMENT);
        }
        return limit.getSeconds() * 1000.0 + limit.getNano() / 1e6;
    }

    /** The token of the call, or null. */
    final CancelToken cancelToken() {
        return cancel;
    }

    /** The native token of the call, or 0. */
    static long cancelHandle(Options<?> options) {
        return options == null || options.cancel == null ? 0 : options.cancel.handle();
    }

    /** The time limit of the call in milliseconds, or 0. */
    static double timeoutMillis(Options<?> options) {
        return options == null ? 0 : options.timeoutMillis;
    }

    /** Sets option {@code name} to {@code value} (a String, Number or Boolean). */
    final S set(String name, Object value) {
        if (value == null) {
            values.remove(name);
        } else {
            values.put(name, value);
        }
        return self();
    }

    @SuppressWarnings("unchecked")
    private S self() {
        return (S) this;
    }

    /** The options as a JSON object. */
    final String toJson() {
        StringBuilder out = new StringBuilder("{");
        for (Map.Entry<String, Object> entry : values.entrySet()) {
            if (out.length() > 1) {
                out.append(',');
            }
            quote(out, entry.getKey());
            out.append(':');
            Object value = entry.getValue();
            if (value instanceof String text) {
                quote(out, text);
            } else if (value instanceof Double || value instanceof Float) {
                double number = ((Number) value).doubleValue();
                if (!Double.isFinite(number)) {
                    throw new KakemonoException(entry.getKey() + " must be a finite number", KakemonoException.Kind.INVALID_ARGUMENT);
                }
                out.append(number);
            } else {
                out.append(value);
            }
        }
        return out.append('}').toString();
    }

    /** Appends {@code text} as a JSON string; a lone surrogate becomes U+FFFD. */
    static void quote(StringBuilder out, String text) {
        out.append('"');
        for (int at = 0; at < text.length(); at++) {
            char ch = text.charAt(at);
            switch (ch) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (ch < 0x20) {
                        out.append(String.format("\\u%04x", (int) ch));
                    } else if (Character.isHighSurrogate(ch) && at + 1 < text.length()
                            && Character.isLowSurrogate(text.charAt(at + 1))) {
                        out.append(ch).append(text.charAt(++at));
                    } else if (Character.isSurrogate(ch)) {
                        out.append('�');
                    } else {
                        out.append(ch);
                    }
                }
            }
        }
        out.append('"');
    }
}
