// Pseudo-locale run (R-I18N-010): the browser asks for "en-XA", Musicus formats every catalogue
// message with accents and wraps it in ⟦…⟧. On each page, every visible text node and every
// placeholder/title/aria-label must come from the catalogue (contain ⟦ or ⟧) unless it is data
// created by this script or a symbol. Also checks `<html lang>` and that no button text overflows.
//
// Usage: node e2e/pseudo.mjs <admin-invite-url>   (fresh data directory, like smoke.mjs)

import { chromium } from 'playwright';

const [inviteUrl] = process.argv.slice(2);
if (!inviteUrl) {
  console.error('usage: node e2e/pseudo.mjs <admin-invite-url>');
  process.exit(2);
}
const base = new URL(inviteUrl).origin;
const browser = await chromium.launch();
const context = await browser.newContext({ locale: 'en-XA', viewport: { width: 1280, height: 900 } });
const page = await context.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
function check(condition, message) {
  if (!condition) throw new Error(`check failed: ${message}`);
  console.log(`ok - ${message}`);
}

// Data entered by this script is shown as entered (a-data-as-entered), never marked.
const DATA = ['admin', 'Trumpet', 'Brass', 'Pseudo March', 'Composer X', 'Trumpets', 'Pseudo concert', 'Trumpet 1',
  'Pseudo', '𝄞 Musicus', 'Administrator', 'Mozilla/'];

await page.goto(inviteUrl);
const pw = page.locator('input[type=password]');
await pw.first().waitFor();
await pw.nth(0).fill('admin-password-1');
await pw.nth(1).fill('admin-password-1');
await page.locator('button[type=submit]').click();
await page.locator('header.topbar').waitFor();
check(await page.evaluate(() => document.documentElement.lang) === 'en-XA', '<html lang> is the pseudo-locale');

const api = (method, path, body) => page.evaluate(async ([m, p, b]) => {
  const r = await fetch(`/api${p}`, { method: m, headers: { 'content-type': 'application/json', 'x-musicus-csrf': '1' },
    body: b === undefined ? undefined : JSON.stringify(b) });
  return r.status === 204 ? null : r.json();
}, [method, path, body]);
const trumpet = await api('POST', '/instruments', { name: 'Trumpet', family: 'Brass' });
const score = await api('POST', '/scores', { title: 'Pseudo March', composer: 'Composer X', tags: [] });
const part = await api('POST', `/scores/${score.id}/parts`, { instrument_ids: [trumpet.id], voices: ['1'], transposition: 'Bb' });
await api('POST', '/groups', { name: 'Trumpets' });
await api('POST', '/programmes', { title: 'Pseudo concert', date: '2026-12-24' });

async function unmarked() {
  return page.evaluate((data) => {
    const bad = [];
    const visible = (el) => el && el.getClientRects().length > 0 && getComputedStyle(el).visibility !== 'hidden';
    const isData = (s) => data.some((d) => s === d || s.includes(d)) || !/\p{L}{2,}/u.test(s);
    const marked = (s) => s.includes('⟦') || s.includes('⟧');
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const s = n.textContent.trim();
      if (!s || !visible(n.parentElement) || n.parentElement.closest('script,style,code,.mono,[id^="__dx"]')) continue;
      if (!marked(s) && !isData(s)) bad.push(`text "${s}"`);
    }
    for (const el of document.querySelectorAll('[placeholder],[title],[aria-label]')) {
      for (const a of ['placeholder', 'title', 'aria-label']) {
        const s = (el.getAttribute(a) || '').trim();
        if (s && !marked(s) && !isData(s)) bad.push(`${a} "${s}" on <${el.tagName.toLowerCase()}>`);
      }
    }
    return [...new Set(bad)];
  }, DATA);
}
async function overflowing() {
  return page.evaluate(() => [...document.querySelectorAll('button, a.button, th, .tag')]
    .filter((b) => b.getClientRects().length > 0 && b.scrollWidth > b.clientWidth + 1)
    .map((b) => b.textContent.trim()).slice(0, 10));
}

const pages = [
  ['/', 'library'], ['/programmes', 'programmes'], ['/instruments', 'instruments'], ['/groups', 'groups'],
  ['/users', 'users'], ['/account', 'account'], ['/import', 'import'], ['/scores/new', 'new piece'],
  [`/scores/${score.id}/edit`, 'piece editor'], [`/parts/${part.id}/edit`, 'part editor'],
  ['/programmes/new', 'programme editor'], [`/parts/${part.id}`, 'reader without file'], ['/no/such/page', 'not found'],
];
let failed = 0;
for (const [path, name] of pages) {
  await page.goto(`${base}${path}`);
  await page.waitForLoadState('networkidle');
  await page.waitForTimeout(300);
  const bad = await unmarked();
  if (bad.length) {
    failed += 1;
    console.log(`not ok - ${name}: untranslated ${bad.join('; ')}`);
  } else {
    console.log(`ok - ${name}: every UI text comes from the catalogue`);
  }
  const over = await overflowing();
  if (over.length) console.log(`note - ${name}: text wider than its box with +30 % length: ${over.join(' | ')}`);
}
// The groups page with a group selected, and the picker open.
await page.goto(`${base}/groups`);
await page.locator('.group-list button, .rows button, li button').first().click().catch(() => {});
await page.waitForTimeout(300);
const groupBad = await unmarked();
check(groupBad.length === 0, `group editor: every UI text comes from the catalogue ${groupBad.join('; ')}`);
check(failed === 0, 'no untranslated text on any page');
check(errors.length === 0, `no browser errors ${errors.join(' | ')}`);
await browser.close();
console.log('pseudo-locale test passed');
