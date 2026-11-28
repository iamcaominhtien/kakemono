package io.github.iamcaominhtien.kakemono;

/** Options of the Markdown output. */
public final class MarkdownOptions extends Options<MarkdownOptions> {
    /** Options with kakemono's defaults. */
    public MarkdownOptions() {
        // Every option starts unset, so kakemono's defaults apply.
    }

    /** MEDIUM keeps tables as HTML; the default is FRIENDLY (the hard modes do not apply). */
    public MarkdownOptions mode(Mode mode) {
        return set("mode", mode == null ? null : mode.value());
    }

    /** How displayed formulas are written (default MATHML). */
    public MarkdownOptions equations(Equations equations) {
        return set("equations", equations == null ? null : equations.value());
    }

    /** Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3). */
    public MarkdownOptions scale(double scale) {
        return set("scale", scale);
    }

    /** The folder of the picture files, relative to the output's (default {@code images}). */
    public MarkdownOptions imageDir(String imageDir) {
        return set("imageDir", imageDir);
    }

    /** Pictures as data URIs instead of files. */
    public MarkdownOptions inlineImages(boolean inlineImages) {
        return set("inlineImages", inlineImages);
    }
}
