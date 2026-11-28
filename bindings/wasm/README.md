# @kakemono/wasm (experimental)

[kakemono](https://github.com/iamcaominhtien/kakemono) in WebAssembly: convert PDF papers, Word
documents, Excel workbooks, EPUB books and web pages to Markdown, HTML, Word, LaTeX, Typst, EPUB,
editable JSON and chunks for RAG, in the browser, in a worker or at the edge, without a server.

```js
import init, { Document, addFont } from '@kakemono/wasm'

await init()                                   // in browsers (Node needs no init)
const doc = Document.fromBytes(new Uint8Array(await file.arrayBuffer()))
const { text, files } = doc.toMarkdown()       // files: [{ path: 'images/…', data: Uint8Array }]
const chunks = doc.toChunks({ maxTokens: 512 })
```

How it differs from `@kakemono/node`:

- Synchronous and single-threaded: run it in a Web Worker to keep a page responsive. A call cannot
  be stopped once started (no `signal` or `timeout`): to stop one, terminate its Worker.
- Documents come from bytes only: no files, no network (fetch them yourself).
- No installed fonts. Laying out a Word document or a workbook as a PDF (`toPdf()`) needs one:
  give it with `addFont(bytes)` first, or `toPdf()` throws a `DocumentError`. Text in SVG pictures
  drawn as PNG uses the fonts given so too.
- Tokens of chunks are estimated (no tokenizers).
- It needs WebAssembly SIMD: every browser since 2023 (Safari 16.4, Chrome 91, Firefox 89), Node
  16.4+, Deno, Cloudflare Workers. It runs about 1.25 times slower than `@kakemono/node` on one
  thread (8 test papers: 8.4 s against 6.8 s).
- The module is about 19 MB (7 MB compressed). A panic of kakemono aborts it with a
  `RuntimeError` (its message is logged to `console.error`): load the module again after one.

Errors are `Error`s named by their kind: `InvalidArgument` (a wrong option), `DocumentError`,
`InternalError`.

Build: `./build.sh` (needs the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` of the
version in `Cargo.toml`); test: `npm test`.
