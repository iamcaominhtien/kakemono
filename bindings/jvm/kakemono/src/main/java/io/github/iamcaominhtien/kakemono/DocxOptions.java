package io.github.iamcaominhtien.kakemono;

/** Options of the Word output: with {@code imageDir}, its pictures are files too. */
public final class DocxOptions extends Options<DocxOptions> {
    /** Options with kakemono's defaults. */
    public DocxOptions() {}

    /** How displayed formulas are written (default MATHML). */
    public DocxOptions equations(Equations equations) {
        return set("equations", equations == null ? null : equations.value());
    }

    /** Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3). */
    public DocxOptions scale(double scale) {
        return set("scale", scale);
    }

    /** The folder of the picture files, relative to the output's (default {@code images}). */
    public DocxOptions imageDir(String imageDir) {
        return set("imageDir", imageDir);
    }
}
