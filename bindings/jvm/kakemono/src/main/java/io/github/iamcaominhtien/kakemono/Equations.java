package io.github.iamcaominhtien.kakemono;

/** How displayed formulas are written. */
public enum Equations {
    /** As MathML where their structure is clear. */
    MATHML("mathml"),
    /** As pictures. */
    IMAGE("image");

    private final String value;

    Equations(String value) {
        this.value = value;
    }

    /** The name kakemono gives it. */
    public String value() {
        return value;
    }
}
