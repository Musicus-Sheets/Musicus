// German run (R-I18N-001, R-I18N-005, R-I18N-006, R-I18N-007, R-I18N-008): the browser asks for
// German ("de-DE"), so Musicus shows German. Checks `<html lang>`, German texts on the invite page,
// the navigation and a few key pages, derived part names with German key names ("Klarinette in B
// 1 & 2"), the German key list, a German error text, dates in German format, the library search
// with a German key name ("Horn Es") and sorting with umlauts. Reports (does not fail on) buttons
// whose German text is wider than its box.
//
// Usage: node e2e/german.mjs <admin-invite-url> [screenshot-dir]   (fresh data directory, like smoke.mjs)
// With a screenshot directory, every visited page is saved there at desktop and phone width (review aid).

import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const [inviteUrl, outDir] = process.argv.slice(2);
if (outDir) mkdirSync(outDir, { recursive: true });
if (!inviteUrl) {
  console.error('usage: node e2e/german.mjs <admin-invite-url> [screenshot-dir]');
  process.exit(2);
}
const base = new URL(inviteUrl).origin;
const browser = await chromium.launch();
const context = await browser.newContext({ locale: 'de-DE', viewport: { width: 1280, height: 900 } });
const page = await context.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
page.on('dialog', (d) => d.accept());
function check(condition, message) {
  if (!condition) throw new Error(`check failed: ${message}`);
  console.log(`ok - ${message}`);
}
async function overflowing() {
  return page.evaluate(() => [...document.querySelectorAll('button, a.button, th, .tag')]
    .filter((b) => b.getClientRects().length > 0 && b.scrollWidth > b.clientWidth + 1 && /\p{L}{2,}/u.test(b.textContent))
    .map((b) => b.textContent.trim()).slice(0, 10));
}
async function visit(path, heading) {
  await page.goto(`${base}${path}`);
  if (heading) await page.getByRole('heading', { name: heading, exact: true }).first().waitFor();
  await page.waitForLoadState('networkidle');
  const over = await overflowing();
  if (over.length) console.log(`note - ${path}: German text wider than its box: ${over.join(' | ')}`);
  if (outDir) {
    const name = path.replace(/[^a-z0-9]+/gi, '-').replace(/^-|-$/g, '') || 'library';
    await page.screenshot({ path: `${outDir}/${name}.png`, fullPage: true });
    await page.setViewportSize({ width: 390, height: 780 });
    await page.waitForTimeout(200);
    const narrow = await overflowing();
    if (narrow.length) console.log(`note - ${path} (phone width): German text wider than its box: ${narrow.join(' | ')}`);
    await page.screenshot({ path: `${outDir}/${name}-phone.png`, fullPage: true });
    await page.setViewportSize({ width: 1280, height: 900 });
  }
}

// Invite page in German, then logged in.
await page.goto(inviteUrl);
await page.getByRole('heading', { name: /^Willkommen, / }).waitFor();
check(await page.evaluate(() => document.documentElement.lang) === 'de-DE', '<html lang> is the German browser language');
await page.getByLabel('Neues Passwort (mindestens 8 Zeichen)').fill('admin-password-1');
await page.getByLabel('Passwort wiederholen').fill('admin-password-1');
await page.getByRole('button', { name: 'Passwort festlegen und anmelden' }).click();
await page.getByRole('heading', { name: 'Bibliothek' }).waitFor();
check(true, 'invite page in German; logged in, library heading "Bibliothek"');
const nav = page.locator('header.topbar nav');
for (const name of ['Bibliothek', 'Programme', 'Instrumente', 'Gruppen', 'Import', 'Benutzer']) {
  check(await nav.getByRole('link', { name, exact: true }).count() === 1, `navigation has "${name}"`);
}
check(await page.getByRole('button', { name: 'Abmelden' }).count() === 1, 'log-out button is "Abmelden"');

// Data as a German ensemble enters it (never translated).
const api = (method, path, body) => page.evaluate(async ([m, p, b]) => {
  const r = await fetch(`/api${p}`, { method: m, headers: { 'content-type': 'application/json', 'x-musicus-csrf': '1' },
    body: b === undefined ? undefined : JSON.stringify(b) });
  return { status: r.status, body: r.status === 204 ? null : await r.json() };
}, [method, path, body]);
const ok = async (...a) => (await api(...a)).body;
const klarinette = await ok('POST', '/instruments', { name: 'Klarinette', family: 'Holz' });
const horn = await ok('POST', '/instruments', { name: 'Horn', family: 'Blech' });
for (const name of ['Posaune', 'Ölberg', 'Oboe']) await ok('POST', '/instruments', { name, family: 'Gemischt' });
const marsch = await ok('POST', '/scores', { title: 'Zwiefacher', tags: [] });
const polka = await ok('POST', '/scores', { title: 'Ärger-Polka', tags: [] });
await ok('POST', '/scores', { title: 'Abendlied', tags: [] });
const kl = await ok('POST', `/scores/${marsch.id}/parts`, { instrument_ids: [klarinette.id], voices: ['1', '2'], transposition: 'Bb' });
await ok('POST', `/scores/${polka.id}/parts`, { instrument_ids: [horn.id], transposition: 'Eb' });
await ok('POST', `/scores/${polka.id}/parts`, { instrument_ids: [], voices: ['3'], transposition: 'F' });
await ok('POST', '/programmes', { title: 'Weihnachtskonzert', date: '2026-12-24' });

// Library: derived part names in German, sorting with umlauts, search by a German key name.
await visit('/', 'Bibliothek');
const labels = await page.locator('.part-label').allInnerTexts();
check(labels.includes('Klarinette in B 1 & 2'), `German part name "Klarinette in B 1 & 2" (${labels.join(' | ')})`);
check(labels.includes('Horn in Es'), 'German key name: "Horn in Es"');
check(labels.includes('Stimme in F 3'), 'part without instrument: "Stimme in F 3"');
const titles = await page.locator('.score-title h2').allInnerTexts();
const order = ['Abendlied', 'Ärger-Polka', 'Zwiefacher'].map((t) => titles.findIndex((x) => x.includes(t)));
check(order.every((i, k) => i >= 0 && (k === 0 || i > order[k - 1])), `pieces sorted Abendlied, Ärger-Polka, Zwiefacher (${titles.join(' | ')})`);
await page.getByPlaceholder('Titel, Komponist, Schlagwort, Nummer suchen …').fill('Horn Es');
await page.waitForTimeout(600);
await page.waitForLoadState('networkidle');
const found = await page.locator('.score-title').allInnerTexts();
check(found.length === 1 && found[0].includes('Ärger-Polka'), `search "Horn Es" finds the horn part in E♭ (${found.join(' | ')})`);
await page.getByPlaceholder('Titel, Komponist, Schlagwort, Nummer suchen …').fill('Klarinette in B 2');
await page.waitForTimeout(600);
await page.waitForLoadState('networkidle');
const found2 = await page.locator('.score-title').allInnerTexts();
check(found2.length === 1 && found2[0].includes('Zwiefacher'), 'search "Klarinette in B 2" finds the clarinet part');

// Part editor: German labels and the key list in German note names.
await visit(`/parts/${kl.id}/edit`, 'Stimme bearbeiten');
check(await page.locator('label.key', { hasText: 'Stimmung' }).count() === 1, 'field "Stimmung" (key)');
check(await page.getByText('Stimmnummern', { exact: true }).count() >= 1, 'field "Stimmnummern" (voices)');
const keys = await page.locator('select option').allInnerTexts();
for (const k of ['B', 'Es', 'As', 'H', '— (klingend)']) check(keys.includes(k), `key list has "${k}"`);
check(!keys.some((k) => k.includes('♭') || k.includes('♮')), 'no English key glyphs in the German key list');

// Instruments sorted with the German collation.
await visit('/instruments', 'Instrumente');
const rows = await page.locator('.instrument-list .row').allInnerTexts();
const pos = ['Oboe', 'Ölberg', 'Posaune'].map((n) => rows.findIndex((r) => r.includes(n)));
check(pos.every((i, k) => i >= 0 && (k === 0 || i > pos[k - 1])), `instruments sorted Oboe, Ölberg, Posaune (${pos})`);

// Programmes: German date format and plural.
await visit('/programmes', 'Programme');
const list = await page.locator('.programme-list').innerText();
check(list.includes('24.12.2026'), `programme date in German format (${list.replace(/\s+/g, ' ')})`);
check(list.includes('0 Stücke'), 'German plural "0 Stücke"');

// Account: German texts, date and time in German format; a German error text.
await visit('/account', 'Konto');
check(await page.getByRole('heading', { name: 'Passwort ändern' }).count() === 1, 'account page "Passwort ändern"');
const devices = await page.locator('body').innerText();
check(/Zuletzt verwendet \d{1,2}\.\d{1,2}\.\d{4}/.test(devices), 'last use shown as a German date (d.m.yyyy)');
await page.getByLabel('Aktuelles Passwort').fill('wrong-password-1');
await page.getByLabel('Neues Passwort (mindestens 8 Zeichen)').fill('another-password-1');
await page.getByRole('button', { name: 'Passwort ändern' }).click();
await page.getByText('Das aktuelle Passwort ist falsch').waitFor();
check(true, 'error code rendered in German: "Das aktuelle Passwort ist falsch"');

// Other key pages in German (headings), with the overflow report.
for (const [path, heading] of [['/groups', 'Gruppen'], ['/users', 'Benutzer'], ['/import', 'Import'],
  ['/scores/new', 'Stück hinzufügen'], [`/scores/${marsch.id}/edit`, 'Stück bearbeiten'],
  ['/programmes/new', 'Neues Programm'], ['/about', 'Über Musicus']]) {
  await visit(path, heading);
  check(true, `${path}: heading "${heading}"`);
}
await page.goto(`${base}/no/such/page`);
await page.getByRole('heading', { name: 'Seite nicht gefunden' }).waitFor();
check(true, 'not-found page in German');

// A browser whose first shipped language is English stays English (R-I18N-002).
const fr = await browser.newContext({ locale: 'fr-FR' });
const frPage = await fr.newPage();
await frPage.goto(`${base}/login`);
await frPage.getByRole('button', { name: 'Log in' }).waitFor();
check(await frPage.evaluate(() => document.documentElement.lang) === 'en', 'French browser: English login page, lang "en"');
const deLogin = await (await browser.newContext({ locale: 'de-AT' })).newPage();
await deLogin.goto(`${base}/login`);
await deLogin.getByRole('button', { name: 'Anmelden' }).waitFor();
check(true, 'Austrian German browser: German login page');

check(errors.length === 0, `no browser errors ${errors.join(' | ')}`);
await browser.close();
console.log('German test passed');
