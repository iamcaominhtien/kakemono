// Tests of @kakemono/node on the committed samples: npm test
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { getEventListeners } from 'node:events'

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
  // Counts are whole numbers, 0 or more (napi would wrap -1).
  await assert.rejects(Document.fromFile(data('word/sample.docx'), { headerRows: -1 }), {
    code: 'InvalidArg',
    message: /headerRows must be a whole number/,
  })
  await assert.rejects(doc.toChunks({ maxTokens: 2.5 }), { code: 'InvalidArg' })
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

test('an AbortSignal stops a call, and the document stays usable', async () => {
  const doc = await Document.fromFile(data('1602.03837v1.pdf'), { jobs: 1 })
  const controller = new AbortController()
  const started = Date.now()
  const pending = doc.toHtml({ mode: 'hard', signal: controller.signal })
  setTimeout(() => controller.abort(new Error('closed')), 20)
  await assert.rejects(pending, (err) => {
    assert.equal(err.name, 'AbortError')
    assert.equal(err.code, 'ABORT_ERR')
    assert.equal(err.cause.message, 'closed')
    return true
  })
  assert.ok(Date.now() - started < 2000, 'stopped soon')
  // A signal already aborted rejects at once; its listener is gone after each call.
  await assert.rejects(doc.toMarkdown({ signal: controller.signal }), { name: 'AbortError' })
  await assert.rejects(doc.toPdf({ signal: AbortSignal.abort() }), { name: 'AbortError' })
  const md = await doc.toMarkdown({ signal: new AbortController().signal })
  assert.match(md.text, /Gravitational/)
})

test('a time limit stops a call with a TimeoutError', async () => {
  await assert.rejects(
    Document.fromFile(data('1602.03837v1.pdf'), { jobs: 1, timeout: 1 }),
    (err) => err.name === 'TimeoutError' && err.code === 'ETIMEDOUT',
  )
  const doc = await Document.fromFile(data('word/sample.docx'))
  await assert.rejects(doc.analyze({ timeout: 0 }), { code: 'InvalidArg' })
  assert.throws(() => doc.toChunks({ signal: {} }), /AbortSignal/)
  // AbortSignal.timeout gives an AbortError whose cause is a TimeoutError.
  await assert.rejects(
    (await Document.fromFile(data('1602.03837v1.pdf'), { jobs: 1 })).toHtml({ mode: 'hard', signal: AbortSignal.timeout(10) }),
    (err) => err.name === 'AbortError' && err.cause.name === 'TimeoutError',
  )
})

test('many calls with one long-lived signal leave no listeners behind', async () => {
  const doc = await Document.fromFile(data('word/sample.docx'))
  const controller = new AbortController()
  const warnings = []
  process.on('warning', (warning) => warnings.push(warning))
  await Promise.all(Array.from({ length: 50 }, () => doc.toJson({ signal: controller.signal })))
  for (let i = 0; i < 50; i++) await doc.toMarkdown({ signal: controller.signal })
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(warnings.filter((w) => /listener/i.test(w.message)), [])
  assert.equal(getEventListeners(controller.signal, 'abort').length, 0)
})

test('a worker terminated while its signalled calls run does not crash the process', async () => {
  const { Worker } = await import('node:worker_threads')
  const code = `
const { Document } = require(${JSON.stringify(new URL('../index.js', import.meta.url).pathname)})
const { parentPort } = require('node:worker_threads')
;(async () => {
  const doc = await Document.fromFile(${JSON.stringify(data('1602.03837v1.pdf'))}, { jobs: 1 })
  const controller = new AbortController()
  for (let i = 0; i < 4; i++) doc.toHtml({ mode: 'hard', signal: controller.signal }).catch(() => {})
  parentPort.postMessage('started')
})()
`
  for (let round = 0; round < 2; round++) {
    const worker = new Worker(code, { eval: true })
    await new Promise((resolve) => worker.once('message', resolve))
    assert.equal(typeof (await worker.terminate()), 'number')
    // The calls end on the blocking pool after the worker's environment is gone.
    await new Promise((resolve) => setTimeout(resolve, 1500))
  }
  // Still alive: a call on this thread works.
  const doc = await Document.fromFile(data('word/sample.docx'))
  assert.match((await doc.toMarkdown()).text, /Gradient Descent/)
})
