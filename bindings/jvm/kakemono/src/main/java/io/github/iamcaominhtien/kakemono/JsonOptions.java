package io.github.iamcaominhtien.kakemono;

/** Options of the editable JSON output. */
public final class JsonOptions extends Options<JsonOptions> {
    /** Options with kakemono's defaults. */
    public JsonOptions() {
        // Every option starts unset, so kakemono's defaults apply.
    }

    /** How displayed formulas are written (default MATHML). */
    public JsonOptions equations(Equations equations) {
        return set("equations", equations == null ? null : equations.value());
    }

    /** Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3). */
    public JsonOptions scale(double scale) {
        return set("scale", scale);
    }

    /** The folder of the picture files, relative to the output's (default {@code images}). */
    public JsonOptions imageDir(String imageDir) {
        return set("imageDir", imageDir);
    }

    /** Pictures as data URIs instead of files. */
    public JsonOptions inlineImages(boolean inlineImages) {
        return set("inlineImages", inlineImages);
    }
}
