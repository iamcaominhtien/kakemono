// Screenshots for the review PDF of the web benchmark (report.py runs it).
//
//   node scripts/web-bench/shoot.js jobs.json
//
// jobs.json: [{ "url": "file:///...", "base": "https://...", "out": "x.jpg", "at": "top" | "middle" | "end" }]
// A saved page ("base" set) is shown as at its address (a <base> element is put in its
// head, so that its style sheets and pictures load), with its scripts. Chromium goes
// through the proxy of the environment when there is one.
const fs = require('fs');
const { chromium } = require('playwright');

const WIDTH = 1100;
const HEIGHT = 760;
const WORKERS = 6;

async function shoot(page, job) {
  if (job.base) {
    const html = fs.readFileSync(new URL(job.url), 'utf8');
    const base = `<base href="${job.base.replace(/"/g, '&quot;')}">`;
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
  const y = job.at === 'middle' ? Math.max(0, height / 2 - HEIGHT / 2) : job.at === 'end' ? Math.max(0, height - HEIGHT) : 0;
  await page.evaluate((y) => window.scrollTo(0, y), y).catch(() => {});
  await page.waitForTimeout(300);
  await page.screenshot({ path: job.out, type: 'jpeg', quality: 55 });
  await page.unroute('**/*').catch(() => {});
}

(async () => {
  const jobs = JSON.parse(fs.readFileSync(process.argv[2], 'utf8'));
  const proxy = process.env.HTTPS_PROXY || process.env.https_proxy;
  const browser = await chromium.launch(proxy ? { proxy: { server: proxy } } : {});
  let next = 0;
  let failed = 0;
  async function worker() {
    for (;;) {
      const job = jobs[next++];
      if (!job) return;
      const context = await browser.newContext({ viewport: { width: WIDTH, height: HEIGHT }, javaScriptEnabled: true });
      const page = await context.newPage();
      try {
        await shoot(page, job);
      } catch (error) {
        failed++;
        console.error(`${job.out}: ${String(error.message).slice(0, 160)}`);
      }
      await context.close();
    }
  }
  await Promise.all(Array.from({ length: WORKERS }, worker));
  await browser.close();
  console.log(`${jobs.length - failed} of ${jobs.length} screenshots`);
})();
