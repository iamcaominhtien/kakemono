package io.github.iamcaominhtien.kakemono;

/** How hard PNG pictures are compressed; the pixels are the same. */
public enum PngCompression {
    /** Several times faster, files about a quarter larger. */
    FAST("fast"),
    /** The default. */
    BALANCED("balanced"),
    /** About a tenth smaller, ten times slower. */
    HIGH("high");

    private final String value;

    PngCompression(String value) {
        this.value = value;
    }

    /** The name kakemono gives it. */
    public String value() {
        return value;
    }
}
