// Tests of @kakemono/wasm in Node (the nodejs build): npm test
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { createRequire } from 'node:module'

const { Document, addFont, version } = createRequire(import.meta.url)('../pkg/node/kakemono_wasm.js')
const data = (path) => readFile(new URL(`../../../tests/data/${path}`, import.meta.url))

test('a PDF is read from bytes and written as Markdown', async () => {
  const doc = Document.fromBytes(await data('1602.03837v1.pdf'))
  assert.equal(doc.format, 'pdf')
  assert.ok(doc.pageCount > 10)
  assert.match(doc.title, /Gravitational Waves/)
  const md = doc.toMarkdown()
  assert.match(md.text, /Observation of Gravitational Waves/)
  assert.ok(md.files.length > 0 && md.files[0].data instanceof Uint8Array)
  const html = doc.toHtml({ mode: 'hard-95' })
  assert.ok(html.text.startsWith('<!DOCTYPE html>'))
  const chunks = doc.toChunks({ maxTokens: 256 })
  assert.ok(chunks.length > 10 && chunks.some((chunk) => chunk.pages.length > 0))
})

test('a Word document: binary outputs, a PDF once a font is given, edited text', async () => {
  const bytes = await data('word/sample.docx')
  const doc = Document.fromBytes(bytes)
  assert.equal(doc.format, 'docx')
  assert.equal(new TextDecoder().decode(doc.toDocx().data.subarray(0, 2)), 'PK')
  // WebAssembly has no installed fonts: a clean error, not an abort.
  assert.throws(() => doc.toPdf(), { name: 'DocumentError', message: /no font is installed/ })
  assert.match(doc.toHtml({ mode: 'medium' }).text, /<html/)
  assert.ok(addFont(await readFile('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf')) >= 1)
  assert.throws(() => addFont(new Uint8Array([1, 2, 3])), { name: 'InvalidArgument' })
  assert.equal(new TextDecoder().decode(doc.toPdf().data.subarray(0, 4)), '%PDF')
  const json = JSON.parse(doc.toJson().text)
  json.blocks.find((block) => typeof block.text === 'string').text = 'Đã dịch'
  const edited = Document.fromBytes(bytes, { content: JSON.stringify(json) })
  assert.match(edited.toMarkdown().text, /Đã dịch/)
  const fromObject = Document.fromBytes(bytes, { content: json })
  assert.match(fromObject.toMarkdown().text, /Đã dịch/)
  assert.ok(Array.isArray(doc.analyze().blocks))
})

test('a workbook prints as a PDF (no system clock in WebAssembly)', async () => {
  const doc = Document.fromBytes(await data('excel/sample.xlsx'))
  assert.equal(doc.format, 'xlsx')
  assert.equal(new TextDecoder().decode(doc.toPdf().data.subarray(0, 4)), '%PDF')
  assert.match(doc.toMarkdown().text, /\|/)
})

test('errors are named by their kind', async () => {
  assert.throws(() => Document.fromBytes(new TextEncoder().encode('nothing')), { name: 'DocumentError' })
  const doc = Document.fromBytes(await data('word/sample.docx'))
  assert.throws(() => doc.toMarkdown({ scale: 9 }), { name: 'InvalidArgument', message: /scale must be between/ })
  assert.throws(() => doc.toEpub({ imageDir: 'x' }), { name: 'InvalidArgument' })
  assert.throws(() => doc.toMarkdown({ imgDir: 'x' }), { name: 'InvalidArgument', message: /unknown field/ })
  assert.match(version(), /^\d+\.\d+\.\d+/)
})
