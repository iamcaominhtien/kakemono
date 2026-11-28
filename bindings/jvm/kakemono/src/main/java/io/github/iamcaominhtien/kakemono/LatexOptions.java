package io.github.iamcaominhtien.kakemono;

/** Options of the LaTeX output. */
public final class LatexOptions extends Options<LatexOptions> {
    /** Options with kakemono's defaults. */
    public LatexOptions() {
        // Every option starts unset, so kakemono's defaults apply.
    }

    /** How displayed formulas are written (default MATHML). */
    public LatexOptions equations(Equations equations) {
        return set("equations", equations == null ? null : equations.value());
    }

    /** Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3). */
    public LatexOptions scale(double scale) {
        return set("scale", scale);
    }

    /** The folder of the picture files, relative to the output's (default {@code images}). */
    public LatexOptions imageDir(String imageDir) {
        return set("imageDir", imageDir);
    }
}
