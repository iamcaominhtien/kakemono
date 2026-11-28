package io.github.iamcaominhtien.kakemono;

/** How the pages are drawn in the hard modes. */
public enum PageImage {
    /** SVG, with a PNG fallback for what SVG cannot draw. */
    SVG("svg"),
    /** SVG only. */
    SVG_ONLY("svg-only"),
    /** PNG pictures. */
    PNG("png");

    private final String value;

    PageImage(String value) {
        this.value = value;
    }

    /** The name kakemono gives it. */
    public String value() {
        return value;
    }
}
