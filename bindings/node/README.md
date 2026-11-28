# @kakemono/node

Convert PDF papers, Word documents (`.docx`, `.doc`), Excel workbooks (`.xlsx`), EPUB books and
web pages to Markdown, HTML, Word, LaTeX, Typst, EPUB, editable JSON and chunks for RAG, from
Node.js. The work is done by [kakemono](https://github.com/iamcaominhtien/kakemono), a Rust
library, on threads of its own: the event loop and libuv's pool stay free.

```bash
npm install @kakemono/node
```

```js
const { Document } = require('@kakemono/node')

const doc = await Document.fromFile('paper.pdf')          // or fromBytes, fromUrl, fromArxiv
const { text, files } = await doc.toMarkdown()             // files: [{ path: 'images/…', data: Buffer }]
const chunks = await doc.toChunks({ maxTokens: 512 })      // [{ id, text, tokens, section, pages, … }]
const { data } = await doc.toDocx()                        // a Word document (Buffer)
const html = (await doc.toHtml({ mode: 'hard-95' })).text  // 'hard' | 'hard-95' | 'medium' | 'friendly'
```

A `Document` is read and analysed once; every output written from it shares that analysis, and
several can be written at once. To translate a document, edit the text of `toJson()` and open the
document again with `{ content }`: every output then shows the edited text.

Options are typed (see `index.d.ts`); a wrong one rejects with an error whose `code` is
`InvalidArg`. `jobs` (default: one thread per core) is per document: on a server converting
several documents at once, set it to 1 or 2.

Prebuilt for Linux (x64, arm64; glibc and musl), macOS (x64, arm64) and Windows (x64, arm64).
For browsers, see `@kakemono/wasm`.
