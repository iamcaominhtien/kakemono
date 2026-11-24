// Prints HTML files to PDF (A4 landscape) with Chromium (report.py runs it).
//
//   node scripts/web-bench/print.js in.html out.pdf [in2.html out2.pdf ...]
const { chromium } = require('playwright');

(async () => {
  const args = process.argv.slice(2);
  const browser = await chromium.launch();
  const page = await browser.newPage();
  for (let i = 0; i + 1 < args.length; i += 2) {
    await page.goto('file://' + args[i], { waitUntil: 'load', timeout: 120000 });
    await page.pdf({ path: args[i + 1], format: 'A4', landscape: true, printBackground: true, margin: { top: '8mm', bottom: '8mm', left: '8mm', right: '8mm' } });
    console.log(args[i + 1]);
  }
  await browser.close();
})();
