// Open clients during shutdown, start and update (F-OPS-008, a-client-outage),
// in a real browser against a real server process that this script starts,
// stops (SIGTERM) and starts again.
//
// Usage: node e2e/outage.mjs <musicus-server binary> <web dir> <port> [screenshot-dir]
//
// Uses a fresh temporary data directory. Checks, in the reader:
// - a stroke drawn just before the stop (still in the 1.5 s debounce) is sent
//   at once when "shutting_down" arrives and is stored (revision 2);
// - the banner "Server is shutting down", drawing blocked (no new stroke);
// - after the restart the banner disappears by itself, drawing works again;
// - network loss (browser offline): banner "Server not reachable", drawing
//   still works, the stroke is sent when the connection is back;
// - a server announcing another version: banner "Musicus has been updated"
//   with "Reload now"; no reload while the reader is open; reload after
//   leaving it.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const [binary, webDir, port, outDir] = process.argv.slice(2);
if (!binary || !webDir || !port) {
  console.error('usage: node e2e/outage.mjs <musicus-server binary> <web dir> <port> [screenshot-dir]');
  process.exit(2);
}
if (outDir) mkdirSync(outDir, { recursive: true });
const data = mkdtempSync(join(tmpdir(), 'musicus-outage-'));
const base = `http://127.0.0.1:${port}`;

function check(condition, message) {
  if (!condition) throw new Error(`check failed: ${message}`);
  console.log(`ok - ${message}`);
}

let server = null;
let invite = null;
const killServer = () => { if (server && server.exitCode === null) server.kill('SIGKILL'); };
function start() {
  return new Promise((resolve, reject) => {
    server = spawn(binary, ['serve', '--data-dir', data, '--listen', `127.0.0.1:${port}`, '--web-dir', webDir,
      '--insecure-cookies', '--shutdown-grace-secs', '3'], { env: { ...process.env, RUST_LOG: 'info', NO_COLOR: '1' } });
    let out = '';
    const onData = (chunk) => {
      out += chunk.toString();
      const m = out.match(/\/invite\/([A-Za-z0-9_-]+)/);
      if (m) invite = m[1];
      if (/\bready\b/.test(out)) resolve();
    };
    server.stdout.on('data', onData);
    server.stderr.on('data', () => {});
    server.on('exit', (code) => { if (!/\bready\b/.test(out)) reject(new Error(`server exited (${code}): ${out}`)); });
  });
}
function stop() {
  return new Promise((resolve) => {
    server.once('exit', (code) => resolve(code));
    server.kill('SIGTERM');
  });
}

// A valid one-page PDF (A4, a few lines), built here.
function pdf() {
  const content = 'q 1 w 72 700 m 523 700 l S 72 650 m 523 650 l S Q';
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 4 0 R /Resources << >> >>',
    `<< /Length ${content.length} >>\nstream\n${content}\nendstream`,
  ];
  let body = '%PDF-1.4\n';
  const offsets = [];
  objects.forEach((o, i) => { offsets.push(body.length); body += `${i + 1} 0 obj\n${o}\nendobj\n`; });
  const xref = body.length;
  body += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const o of offsets) body += `${String(o).padStart(10, '0')} 00000 n \n`;
  body += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(body, 'latin1');
}

// Whatever fails, the server started here does not outlive the script.
try {
await start();
const browser = await chromium.launch(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {});
const context = await browser.newContext({ locale: 'en-US', viewport: { width: 1100, height: 900 } });
const page = await context.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(`${e}`));
let loads = 0;
page.on('load', () => { loads += 1; });
async function shot(name) {
  if (outDir) await page.screenshot({ path: `${outDir}/${name}.png` });
}

await page.goto(`${base}/invite/${invite}`);
await page.getByLabel('New password (at least 8 characters)').fill('admin-password-1');
await page.getByLabel('Repeat password').fill('admin-password-1');
await page.getByRole('button', { name: 'Set password and log in' }).click();
await page.getByRole('heading', { name: 'Library' }).waitFor();

// A piece with one part and its PDF, through the API with the page's session.
const api = page.request;
const H = { 'x-musicus-csrf': '1' };
const score = await (await api.post(`${base}/api/scores`, { headers: H, data: { title: 'Outage March' } })).json();
const part = await (await api.post(`${base}/api/scores/${score.id}/parts`,
  { headers: H, data: { label: 'Solo', instrument_ids: [] } })).json();
const up = await api.put(`${base}/api/parts/${part.id}/file`,
  { headers: { ...H, 'content-type': 'application/pdf' }, data: pdf() });
check(up.ok(), 'part with a PDF created');

async function revision() {
  const sets = await (await api.get(`${base}/api/parts/${part.id}/annotation-sets`)).json();
  if (sets.length === 0) return 0;
  const r = await api.get(`${base}/api/annotation-sets/${sets[0].id}/pages/0`);
  return Number((r.headers().etag || '"0"').replaceAll('"', ''));
}
async function stroke(y) {
  const box = await page.locator('#ink-canvas').boundingBox();
  await page.mouse.move(box.x + box.width * 0.3, box.y + box.height * y);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.6, box.y + box.height * (y + 0.01), { steps: 8 });
  await page.mouse.up();
}
const banner = page.locator('.conn-banner');

await page.goto(`${base}/parts/${part.id}`);
await page.locator('#ink-canvas').waitFor();
loads = 0;
await page.waitForFunction(() => document.querySelector('.page-no')?.textContent?.includes('/'));
await page.getByRole('button', { name: /Draw/ }).click();
await stroke(0.3);
await page.locator('.save-status', { hasText: /^Saved$/ }).waitFor({ timeout: 10000 });
check(await revision() === 1, 'first stroke saved (revision 1)');

// --- Planned shutdown: block, then flush -------------------------------------
await stroke(0.4);
check(await page.locator('.save-status').innerText() === 'Unsaved', 'second stroke waits for the debounce');
const exited = stop();
await banner.filter({ hasText: 'Server is shutting down' }).waitFor({ timeout: 5000 });
check(true, 'banner "Server is shutting down"');
check(await page.locator('#ink-canvas.active').count() === 0, 'the ink canvas takes no input');
check(await page.locator('.save-status').innerText() === 'Drawing paused', 'tool bar says drawing is paused');
await shot('shutting-down');
const code = await exited;
check(code === 0, `server exited cleanly (code ${code})`);
await stroke(0.5); // refused: blocked
await page.waitForTimeout(500);
check(await banner.innerText().then((t) => t.includes('shutting down')), 'still "shutting down" after the server is gone');

await start();
await banner.waitFor({ state: 'detached', timeout: 15000 });
check(true, 'banner gone by itself once the server is back');
check(await revision() === 2, 'the stroke drawn before the stop was stored by the flush (revision 2)');
check(loads === 0, 'no reload (same version)');

// --- Network loss: drawing goes on --------------------------------------------
await context.setOffline(true);
await stroke(0.6);
await banner.filter({ hasText: 'Server not reachable' }).waitFor({ timeout: 10000 });
check(await page.locator('#ink-canvas.active').count() === 1, 'drawing still possible while not reachable');
await stroke(0.7);
await page.locator('.banner', { hasText: 'Annotations not saved yet are kept in this tab' }).waitFor();
check(true, 'reader says the unsaved annotations are kept');
await shot('unreachable');
await context.setOffline(false);
await banner.waitFor({ state: 'detached', timeout: 15000 });
await page.locator('.save-status', { hasText: /^Saved$/ }).waitFor({ timeout: 10000 });
check(await revision() === 3, 'strokes drawn while offline were sent when back (revision 3)');

// --- Another version: notice, reload only after leaving the reader -------------
// A stream that announces another version and stays open (fulfill cannot stream).
const fake = createServer((req, res) => {
  res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
  res.write('event: state\ndata: {"state":"ready","version":"99.0.0"}\n\n');
});
await new Promise((r) => fake.listen(Number(port) + 1, '127.0.0.1', r));
let fakes = 0;
await page.route('**/api/events', (route) => {
  fakes += 1;
  if (fakes > 1) return route.continue();
  return route.continue({ url: `http://127.0.0.1:${Number(port) + 1}/api/events` });
});
// Polling during the restart asks for the version too: the new one.
let newVersion = true;
await page.route('**/api/version', (route) => (newVersion
  ? route.fulfill({ status: 200, contentType: 'application/json', body: '{"version":"99.0.0"}' })
  : route.continue()));
// The server is updated in between: stopped and started again.
await stop();
await start();
await banner.filter({ hasText: 'Musicus has been updated' }).waitFor({ timeout: 20000 });
check(await banner.getByRole('button', { name: 'Reload now' }).count() === 1, 'notice with "Reload now"');
await shot('updated');
await page.waitForTimeout(2500);
check(loads === 0, 'no reload while the reader is open');
const reloaded = page.waitForEvent('load', { timeout: 15000 });
await page.getByTitle('Back').click();
await reloaded;
newVersion = false;
check(loads === 1, 'reloaded after leaving the reader');
await page.getByRole('heading', { name: 'Library' }).waitFor();

check(errors.length === 0, `no page errors ${JSON.stringify(errors)}`);
await browser.close();
fake.closeAllConnections();
fake.close();
await stop();
console.log('outage: all checks passed');
} finally {
  killServer();
}
