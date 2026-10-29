// Renders design/icon/icon.svg to the app icon set with headless Chromium (Playwright),
// then downsizes with sips. Needs Playwright with its Chromium installed, for example:
//   NODE_PATH=<a node_modules folder that has playwright> node design/icon/render-icon.cjs
const { chromium } = require('playwright');
const fs = require('fs'), path = require('path'), { execFileSync } = require('child_process');
(async () => {
  const svg = fs.readFileSync(path.join(__dirname, 'icon.svg'), 'utf8');
  const set = path.join(__dirname, '../../mac/OpenPainter/Assets.xcassets/AppIcon.appiconset');
  const big = path.join(set, 'icon_1024.png');
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1024, height: 1024 } });
  await page.setContent(`<body style="margin:0">${svg}</body>`);
  await page.locator('svg').screenshot({ path: big, omitBackground: true });
  await browser.close();
  for (const z of [16, 32, 64, 128, 256, 512]) execFileSync('sips', ['-z', String(z), String(z), big, '--out', path.join(set, `icon_${z}.png`)], { stdio: 'ignore' });
  console.log('wrote', set);
})().catch(e => { console.error(e); process.exit(1); });
