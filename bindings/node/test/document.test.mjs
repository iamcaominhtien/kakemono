// Tests of @kakemono/node on the committed samples: npm test
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { createRequire } from 'node:module'

const { Document, addFont, version } = createRequire(import.meta.url)('../index.js')
const data = (path) => new URL(`../../../tests/data/${path}`, import.meta.url).pathname

test('a PDF is written as Markdown with its pictures', async () => {
  const doc = await Document.fromFile(data('1602.03837v1.pdf'), { jobs: 1 })
  assert.equal(doc.format, 'pdf')
  assert.ok(doc.pageCount > 10)
  assert.match(doc.title, /Gravitational Waves/)
  const md = await doc.toMarkdown()
  assert.match(md.text, /Observation of Gravitational Waves/)
  assert.ok(md.files.length > 0)
  for (const file of md.files) {
    assert.ok(file.path.startsWith('images/'))
    assert.ok(Buffer.isBuffer(file.data) && file.data.length > 0)
  }
  const inline = await doc.toMarkdown({ inlineImages: true })
  assert.equal(inline.files.length, 0)
  assert.match(inline.text, /data:image\//)
})

test('chunks come as objects', async () => {
  const doc = await Document.fromFile(data('1602.03837v1.pdf'), { jobs: 1 })
  const chunks = await doc.toChunks({ maxTokens: 256 })
  assert.ok(chunks.length > 10)
  assert.equal(chunks[0].id, 0)
  assert.ok(chunks.every((chunk) => chunk.text.length > 0 && Array.isArray(chunk.section)))
  assert.ok(chunks.some((chunk) => chunk.pages.length > 0))
})

test('a Word document from bytes: binary outputs and a PDF', async () => {
  const doc = await Document.fromBytes(await readFile(data('word/sample.docx')))
  assert.equal(doc.format, 'docx')
  assert.equal(doc.pageCount, 0)
  const docx = await doc.toDocx()
  assert.equal(docx.data.subarray(0, 2).toString(), 'PK')
  const pdf = await doc.toPdf()
  assert.equal(pdf.data.subarray(0, 4).toString(), '%PDF')
  const epub = await doc.toEpub({ modified: '2026-01-31T12:00:00Z' })
  assert.equal(epub.data.subarray(0, 2).toString(), 'PK')
  const structure = await doc.analyze()
  assert.ok(Array.isArray(structure.blocks) && structure.blocks.length > 0)
})

test('edited content shows in every output', async () => {
  const path = data('word/sample.docx')
  const original = await Document.fromFile(path)
  const json = JSON.parse((await original.toJson()).text)
  const block = json.blocks.find((block) => typeof block.text === 'string')
  block.text = 'Đã dịch'
  const edited = await Document.fromFile(path, { content: json })
  assert.deepEqual(edited.warnings, [])
  assert.match((await edited.toMarkdown()).text, /Đã dịch/)
  // The same content given as text.
  const again = await Document.fromFile(path, { content: JSON.stringify(json) })
  assert.match((await again.toHtml()).text, /Đã dịch/)
})

test('errors carry a code and a message', async () => {
  await assert.rejects(Document.fromBytes(Buffer.from('nothing')), (err) => err.code === 'GenericFailure')
  const doc = await Document.fromFile(data('word/sample.docx'))
  await assert.rejects(doc.toMarkdown({ scale: 9 }), { code: 'InvalidArg', message: /scale must be between/ })
  await assert.rejects(doc.toMarkdown({ imageDir: '../up' }), /imageDir must be a relative folder/)
  await assert.rejects(Document.fromFile('/no/such/file.pdf'), /reading \/no\/such\/file.pdf/)
  await assert.rejects(Document.fromFile(data('word/sample.docx'), { pngCompression: 'max' }), {
    code: 'InvalidArg',
  })
})

test('several outputs run at once without blocking the event loop', async () => {
  const doc = await Document.fromFile(data('1602.03837v1.pdf'), { jobs: 1 })
  let ticks = 0
  const timer = setInterval(() => ticks++, 5)
  const outputs = await Promise.all([doc.toMarkdown(), doc.toJson(), doc.toLatex(), doc.toTypst(), doc.toHtml()])
  clearInterval(timer)
  assert.ok(outputs.every((out) => out.text.length > 1000))
  assert.ok(ticks > 0, 'the event loop kept running')
})

test('the version is the library’s; a font that is not one is refused', () => {
  assert.match(version(), /^\d+\.\d+\.\d+/)
  assert.throws(() => addFont(Buffer.from('nothing')), { code: 'InvalidArg' })
})
