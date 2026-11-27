// Prints HTML files to PDF (A4 landscape) with Chromium (report.py runs it).
//
//   node scripts/web-bench/print.mjs in.html out.pdf [in2.html out2.pdf ...]
//
// The files are in the repository.
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

// Playwright as CommonJS finds it (the one installed for the whole machine too).
const { chromium } = createRequire(import.meta.url)('playwright');

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

/** `file` resolved, when it is in the repository; an error otherwise. */
function inRepository(file) {
  const resolved = path.resolve(file);
  if (!resolved.startsWith(ROOT + path.sep)) {
    throw new Error(`not in the repository: ${resolved}`);
  }
  return resolved;
}

async function printOne(page, html, pdf) {
  await page.goto('file://' + inRepository(html), { waitUntil: 'load', timeout: 120000 });
  await page.pdf({
    path: inRepository(pdf),
    format: 'A4',
    landscape: true,
    printBackground: true,
    margin: { top: '8mm', bottom: '8mm', left: '8mm', right: '8mm' },
  });
  console.log(pdf);
}

async function main() {
  const args = process.argv.slice(2);
  const pairs = [];
  for (let i = 0; i + 1 < args.length; i += 2) pairs.push([args[i], args[i + 1]]);
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    // One after the other, on the same page.
    await pairs.reduce((done, [html, pdf]) => done.then(() => printOne(page, html, pdf)), Promise.resolve());
  } finally {
    await browser.close();
  }
}

try {
  await main();
} catch (error) {
  console.error(`print.mjs: ${error.name}`);
  process.exitCode = 1;
}
