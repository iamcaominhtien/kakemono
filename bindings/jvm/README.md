# kakemono for Java and Kotlin

Convert PDF papers, Word documents (`.docx`, `.doc`), Excel workbooks, EPUB books and web pages to
Markdown, HTML, Word, LaTeX, Typst, EPUB, editable JSON and chunks for RAG, from the JVM (Java 17+,
Kotlin). The work is done by [kakemono](https://github.com/iamcaominhtien/kakemono), a Rust
library, through JNI.

```xml
<dependency>
  <groupId>io.github.iamcaominhtien</groupId>
  <artifactId>kakemono</artifactId>
  <version>0.20.0</version>
</dependency>
<!-- The native library of each platform you run on: linux-x86_64, linux-aarch64,
     linux-musl-x86_64, linux-musl-aarch64, macos-x86_64, macos-aarch64,
     windows-x86_64, windows-aarch64. -->
<dependency>
  <groupId>io.github.iamcaominhtien</groupId>
  <artifactId>kakemono-native</artifactId>
  <version>0.20.0</version>
  <classifier>linux-x86_64</classifier>
  <scope>runtime</scope>
</dependency>
```

```java
try (Document doc = Document.open(Path.of("paper.pdf"))) {
    doc.toMarkdown().writeTo(Path.of("out/paper.md"));             // and out/images/
    List<Chunk> chunks = doc.toChunks(new ChunkOptions().maxTokens(512));
    byte[] docx = doc.toDocx().data();
    String html = doc.toHtml(new HtmlOptions().mode(Mode.HARD_95)).text();
}
```

```kotlin
Document.open(Path.of("paper.pdf")).use { doc ->
    val md = doc.toMarkdown(MarkdownOptions().mode(Mode.MEDIUM)).text()
}
```

A `Document` is read and analysed once; it can be written from several threads at once, and
`close()` waits for the calls running on it. Errors are `KakemonoException`s with a `kind()`
(`INVALID_ARGUMENT`, `DOCUMENT`, `INTERNAL`). `jobs` (default: one thread per core) is per
document: a server converting several documents at once should set `new OpenOptions().jobs(1)`.

## LangChain4j

`io.github.iamcaominhtien:kakemono-langchain4j` adds a `DocumentParser` (Markdown, with `format`,
`title` and `page_count` metadata) and `KakemonoSegments`, chunks as `TextSegment`s with their
`section` and `pages`:

```java
Document doc = FileSystemDocumentLoader.loadDocument(path, new KakemonoDocumentParser());
List<TextSegment> segments = KakemonoSegments.of(path, new ChunkOptions().maxTokens(512));
```

## Native library

It is looked for at `-Dkakemono.library.path`, then on the class path (the `kakemono-native`
artifact, copied once to `-Dkakemono.cache.dir`, by default the temporary folder: set it when that
folder is `noexec`), then on `java.library.path`. Build it with
`cargo build --release -p kakemono-jni` in `bindings/`; `mvn verify` here tests with it.
