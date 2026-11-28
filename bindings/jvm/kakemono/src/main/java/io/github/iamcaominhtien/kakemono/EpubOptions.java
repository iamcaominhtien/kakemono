package io.github.iamcaominhtien.kakemono;

/** Options of the EPUB output. */
public final class EpubOptions extends Options<EpubOptions> {
    /** Options with kakemono's defaults. */
    public EpubOptions() {}

    /** How displayed formulas are written (default MATHML). */
    public EpubOptions equations(Equations equations) {
        return set("equations", equations == null ? null : equations.value());
    }

    /** Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3). */
    public EpubOptions scale(double scale) {
        return set("scale", scale);
    }

    /** Widest formula kept as MathML, in em (default 20). */
    public EpubOptions maxMathWidthEm(double maxMathWidthEm) {
        return set("maxMathWidthEm", maxMathWidthEm);
    }

    /** A cover page (default true). */
    public EpubOptions cover(boolean cover) {
        return set("cover", cover);
    }

    /** The book's date, {@code CCYY-MM-DDThh:mm:ssZ} (default: a fixed date). */
    public EpubOptions modified(String modified) {
        return set("modified", modified);
    }
}
