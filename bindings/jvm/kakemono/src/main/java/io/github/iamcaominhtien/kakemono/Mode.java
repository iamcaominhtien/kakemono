package io.github.iamcaominhtien.kakemono;

/** An HTML mode, from the most faithful to the most readable (Markdown: MEDIUM or FRIENDLY). */
public enum Mode {
    /** A copy of every page, with a selectable text layer. */
    HARD("hard"),
    /** Like HARD, but all text is real HTML text placed as in the PDF. */
    HARD_95("hard-95"),
    /** Page and column layout kept, text reflowed (Markdown: tables kept as HTML). */
    MEDIUM("medium"),
    /** A single-column article. */
    FRIENDLY("friendly");

    private final String value;

    Mode(String value) {
        this.value = value;
    }

    /** The name kakemono gives it. */
    public String value() {
        return value;
    }
}
