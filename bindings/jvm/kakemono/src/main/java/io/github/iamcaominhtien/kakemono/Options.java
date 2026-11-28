package io.github.iamcaominhtien.kakemono;

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

    Options() {}

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
