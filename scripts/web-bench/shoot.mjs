// Screenshots for the review PDF of the web benchmark (report.py runs it).
//
//   node scripts/web-bench/shoot.mjs jobs.json
//
// jobs.json: [{ "url": "file:///...", "base": "https://...", "out": "x.jpg", "at": "top" | "middle" | "end" }]
// A saved page ("base" set) is shown as at its address (a <base> element is put in its
// head, so that its style sheets and pictures load), with its scripts. Chromium goes
// through the proxy of the environment when there is one.
// Every file it reads or writes is in the repository: the jobs, the saved pages and the
// screenshots.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

// Playwright as CommonJS finds it (the one installed for the whole machine too).
const { chromium } = createRequire(import.meta.url)('playwright');

const WIDTH = 1100;
const HEIGHT = 760;
const WORKERS = 6;
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

/** `file` resolved, when it is in the repository; an error otherwise. */
function inRepository(file) {
  const resolved = path.resolve(file);
  if (resolved !== ROOT && !resolved.startsWith(ROOT + path.sep)) {
    throw new Error(`not in the repository: ${resolved}`);
  }
  return resolved;
}

/** Where to scroll a page `height` high to show its part `at`. */
function scrollTarget(at, height) {
  if (at === 'middle') return Math.max(0, height / 2 - HEIGHT / 2);
  if (at === 'end') return Math.max(0, height - HEIGHT);
  return 0;
}

async function shoot(page, job) {
  const out = inRepository(job.out);
  if (job.base) {
    const html = fs.readFileSync(inRepository(fileURLToPath(job.url)), 'utf8');
    const base = `<base href="${job.base.replaceAll('"', '&quot;')}">`;
    const withBase = /<head[^>]*>/i.test(html) ? html.replace(/<head[^>]*>/i, (head) => head + base) : base + html;
    await page.route('**/*', (route) => {
      const url = route.request().url();
      if (route.request().resourceType() === 'document' && url === job.base) {
        return route.fulfill({ status: 200, contentType: 'text/html; charset=utf-8', body: withBase });
      }
      return route.continue();
    });
    await page.goto(job.base, { waitUntil: 'domcontentloaded', timeout: 30000 }).catch(() => {});
  } else {
    await page.goto(job.url, { waitUntil: 'load', timeout: 30000 }).catch(() => {});
  }
  await page.waitForTimeout(job.base ? 2500 : 600);
  const height = await page.evaluate(() => document.documentElement.scrollHeight).catch(() => HEIGHT);
  await page.evaluate((y) => window.scrollTo(0, y), scrollTarget(job.at, height)).catch(() => {});
  await page.waitForTimeout(300);
  await page.screenshot({ path: out, type: 'jpeg', quality: 55 });
  await page.unroute('**/*').catch(() => {});
}

async function main() {
  const jobs = JSON.parse(fs.readFileSync(inRepository(process.argv[2]), 'utf8'));
  const proxy = process.env.HTTPS_PROXY || process.env.https_proxy;
  const browser = await chromium.launch(proxy ? { proxy: { server: proxy } } : {});
  let next = 0;
  // Each worker takes the next job when it is done with one.
  async function worker() {
    const number = next++;
    const job = jobs[number];
    if (!job) return;
    const context = await browser.newContext({ viewport: { width: WIDTH, height: HEIGHT }, javaScriptEnabled: true });
    try {
      await shoot(await context.newPage(), job);
    } catch (error) {
      console.error(`job ${number + 1}: no screenshot (${error.name})`);
    }
    await context.close();
    await worker();
  }
  try {
    await Promise.all(Array.from({ length: WORKERS }, worker));
  } finally {
    await browser.close();
  }
}

try {
  await main();
} catch (error) {
  console.error(`shoot.mjs: ${error.name}`);
  process.exitCode = 1;
}
