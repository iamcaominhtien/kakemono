package io.github.iamcaominhtien.kakemono;

/** The layout of the Typst output. */
public enum TypstLayout {
    /** A book when the document has chapters, an article otherwise. */
    AUTO("auto"),
    /** An article. */
    ARTICLE("article"),
    /** A printed book: title page, chapters on right-hand pages, running heads. */
    BOOK("book");

    private final String value;

    TypstLayout(String value) {
        this.value = value;
    }

    /** The name kakemono gives it. */
    public String value() {
        return value;
    }
}
