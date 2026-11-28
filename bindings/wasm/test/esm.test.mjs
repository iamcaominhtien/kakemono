// The same import as in browsers works in Node: npm test
import { test } from 'node:test'
import assert from 'node:assert/strict'
import init, { Document, version } from '@kakemono/wasm'

test('the ES module entry works with init()', async () => {
  await init()
  assert.match(version(), /^\d+\.\d+\.\d+/)
  assert.equal(typeof Document.fromBytes, 'function')
})
