package io.github.iamcaominhtien.kakemono;

/** Options of the HTML output (pictures inline unless {@code inlineImages(false)}). */
public final class HtmlOptions extends Options<HtmlOptions> {
    /** Options with kakemono's defaults. */
    public HtmlOptions() {}

    /** The mode (default FRIENDLY). */
    public HtmlOptions mode(Mode mode) {
        return set("mode", mode == null ? null : mode.value());
    }

    /** How displayed formulas are written (default MATHML). */
    public HtmlOptions equations(Equations equations) {
        return set("equations", equations == null ? null : equations.value());
    }

    /** Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3). */
    public HtmlOptions scale(double scale) {
        return set("scale", scale);
    }

    /** How the pages are drawn in the hard modes (default SVG). */
    public HtmlOptions pageImage(PageImage pageImage) {
        return set("pageImage", pageImage == null ? null : pageImage.value());
    }

    /** Embed the fonts of the PDF (default true). */
    public HtmlOptions embedFonts(boolean embedFonts) {
        return set("embedFonts", embedFonts);
    }

    /** Pictures as data URIs instead of files. */
    public HtmlOptions inlineImages(boolean inlineImages) {
        return set("inlineImages", inlineImages);
    }

    /** The folder of the picture files, relative to the output's (default {@code images}). */
    public HtmlOptions imageDir(String imageDir) {
        return set("imageDir", imageDir);
    }
}
