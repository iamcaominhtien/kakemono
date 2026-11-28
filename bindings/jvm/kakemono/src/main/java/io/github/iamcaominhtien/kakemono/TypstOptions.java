package io.github.iamcaominhtien.kakemono;

/** Options of the Typst output. */
public final class TypstOptions extends Options<TypstOptions> {
    /** Options with kakemono's defaults. */
    public TypstOptions() {}

    /** How displayed formulas are written (default MATHML). */
    public TypstOptions equations(Equations equations) {
        return set("equations", equations == null ? null : equations.value());
    }

    /** Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3). */
    public TypstOptions scale(double scale) {
        return set("scale", scale);
    }

    /** The folder of the picture files, relative to the output's (default {@code images}). */
    public TypstOptions imageDir(String imageDir) {
        return set("imageDir", imageDir);
    }

    /** The layout (default AUTO). */
    public TypstOptions layout(TypstLayout layout) {
        return set("layout", layout == null ? null : layout.value());
    }
}
