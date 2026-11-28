// Checks that the TypeScript declarations type the API as documented:
// npm run typecheck (compiled with tsc --noEmit, never run).
import { Document, type Chunk, type TextOutput, version } from '../index.js'

async function main(): Promise<void> {
  const doc: Document = await Document.fromFile('paper.pdf', { jobs: 1, pngCompression: 'fast' })
  const md: TextOutput = await doc.toMarkdown({ mode: 'medium', imageDir: 'img' })
  const text: string = md.text
  const bytes: Buffer = md.files[0].data
  const html = await doc.toHtml({ mode: 'hard-95', pageImage: 'svg-only' })
  const chunks: Chunk[] = await doc.toChunks({ maxTokens: 256, tokenizer: 'cl100k_base' })
  const pages: number[] = chunks[0].pages
  const pdf: Buffer = (await doc.toPdf({ signal: AbortSignal.timeout(1000) })).data
  await doc.toJson({ timeout: 5000, signal: new AbortController().signal })
  // @ts-expect-error: a signal is an AbortSignal
  await doc.analyze({ signal: true })
  const format: 'pdf' | 'docx' | 'doc' | 'xlsx' | 'epub' | 'html' = doc.format
  const title: string | null = doc.title
  // @ts-expect-error: not a mode
  await doc.toHtml({ mode: 'pretty' })
  // @ts-expect-error: Markdown has no hard mode
  await doc.toMarkdown({ mode: 'hard' })
  // @ts-expect-error: an unknown option
  await doc.toEpub({ imageDir: 'x' })
  console.log(text, bytes, html, pages, pdf, format, title, version())
}

void main
