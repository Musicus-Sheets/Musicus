// Browser smoke test of the whole application, driven by Playwright.
//
// Usage: node e2e/smoke.mjs <admin-invite-url> <some.pdf> <screenshot-dir>
//
// Needs a running server with the built web client (see README) and a fresh
// data directory. It walks through the main flows: admin setup, instruments,
// the guided flow for a new piece, editing parts and pieces from the
// library, invites, groups and grants, programmes, a librarian's and a
// member's view (no management controls), a member reading and annotating.

import { chromium } from 'playwright';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [inviteUrl, pdfPath, outDir] = process.argv.slice(2);
if (!inviteUrl || !pdfPath || !outDir) {
  console.error('usage: node e2e/smoke.mjs <admin-invite-url> <some.pdf> <screenshot-dir>');
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });
const base = new URL(inviteUrl).origin;

const browser = await chromium.launch(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {});
let step = 0;
async function shot(page, name) {
  step += 1;
  await page.screenshot({ path: `${outDir}/${String(step).padStart(2, '0')}-${name}.png` });
}
function check(condition, message) {
  if (!condition) {
    throw new Error(`check failed: ${message}`);
  }
  console.log(`ok - ${message}`);
}

const errors = [];
function watch(page) {
  page.on('pageerror', (e) => errors.push(`${e}`));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
}

// --- Admin: accept invite, build a small library --------------------------
const admin = await browser.newContext({ viewport: { width: 1280, height: 900 } });
const a = await admin.newPage();
watch(a);
a.on('dialog', (d) => d.accept());
await a.goto(inviteUrl);
await a.getByLabel('New password (at least 8 characters)').fill('admin-password-1');
await a.getByLabel('Repeat password').fill('admin-password-1');
await a.getByRole('button', { name: 'Set password and log in' }).click();
await a.getByRole('heading', { name: 'Library' }).waitFor();
check(true, 'admin set a password via the invite link and is logged in');
const nav = a.locator('header.topbar nav');
for (const name of ['Library', 'Programmes', 'Instruments', 'Groups', 'Users']) {
  check(await nav.getByRole('link', { name, exact: true }).count() === 1, `admin navigation has ${name}`);
}
check(await nav.getByRole('link', { name: 'Manage' }).count() === 0, 'there is no Manage tab');

// Instruments: add, edit, delete.
await nav.getByRole('link', { name: 'Instruments' }).click();
await a.getByRole('heading', { name: 'Instruments' }).waitFor();
for (const [name, family] of [['Trumpet', 'Brass'], ['Flute', 'Woodwind'], ['Horn', 'Brass']]) {
  await a.getByPlaceholder('Instrument, e.g. Trumpet').fill(name);
  await a.getByPlaceholder('Family, e.g. Brass').fill(family);
  await a.getByRole('button', { name: '+ Add' }).click();
  await a.locator('.instrument-list .row', { hasText: name }).waitFor();
}
const horn = a.locator('.instrument-list li', { hasText: 'Horn' });
await horn.getByRole('button', { name: 'Edit' }).click();
await a.locator('.instrument-list li.editing input').first().fill('French horn');
await a.locator('.instrument-list li.editing').getByRole('button', { name: 'Save' }).click();
await a.locator('.instrument-list .row', { hasText: 'French horn' }).waitFor();
await a.locator('.instrument-list li', { hasText: 'French horn' }).getByRole('button', { name: 'Delete' }).click();
await a.locator('.instrument-list .row', { hasText: 'French horn' }).waitFor({ state: 'detached' });
check(await a.locator('section.card h2', { hasText: 'Brass' }).count() === 1, 'instruments are grouped by family');
check(true, 'librarian added, renamed and deleted instruments');

// The guided flow for a new piece.
await nav.getByRole('link', { name: 'Library' }).click();
await a.getByRole('link', { name: 'Add piece' }).click();
await a.getByRole('heading', { name: 'Add piece' }).waitFor();
await a.getByLabel('Title', { exact: true }).fill('Florentiner Marsch');
await a.getByLabel('Composer').fill('Julius Fučík');
await a.getByLabel('Genre').fill('Concert');
await a.getByRole('button', { name: 'Next: parts' }).click();
await a.getByRole('heading', { name: 'Parts of “Florentiner Marsch”' }).waitFor();
const newPart = a.locator('form.new-part');
await newPart.getByLabel('Label').fill('2nd Trumpet in Bb');
await newPart.getByLabel('Voice').fill('2');
await newPart.getByLabel('Key').fill('Bb');
await newPart.locator('label.check', { hasText: 'Trumpet' }).locator('input').check();
await newPart.locator('input[type=file]').setInputFiles(pdfPath);
await newPart.getByRole('button', { name: '+ Add part' }).click();
await a.locator('.added-parts li', { hasText: '2nd Trumpet in Bb' }).getByText(/Version 1/).waitFor({ timeout: 30000 });
// A second part right away, this time without a PDF.
await newPart.getByLabel('Label').fill('Flute');
await newPart.locator('label.check', { hasText: 'Flute' }).locator('input').check();
await newPart.getByRole('button', { name: '+ Add part' }).click();
await a.locator('.added-parts li', { hasText: 'Flute' }).getByText('No PDF yet').waitFor();
await shot(a, 'add-piece-parts');
check(true, 'guided flow created the piece with two parts and uploaded a PDF');
await a.getByRole('button', { name: 'Finish' }).click();
await a.locator('.score.focus', { hasText: 'Florentiner Marsch' }).waitFor();
check(await a.locator('.score.focus .part-cell').count() === 2, 'the new piece is highlighted in the library with both parts');

// Edit a single part from its tile: upload, then replace the PDF.
await a.locator('.part-cell', { hasText: 'Flute' }).getByRole('link', { name: 'Edit' }).click();
await a.getByRole('heading', { name: 'Edit part' }).waitFor();
await a.locator('.part-editor input[type=file]').setInputFiles(pdfPath);
await a.locator('.part-editor').getByText(/Version 1 ·/).waitFor({ timeout: 30000 });
const corrected = join(outDir, 'corrected.pdf');
writeFileSync(corrected, Buffer.concat([readFileSync(pdfPath), Buffer.from('\n% corrected\n')]));
await a.locator('.part-editor input[type=file]').setInputFiles(corrected);
await a.locator('.part-editor').getByText('Uploaded as version 2').waitFor({ timeout: 30000 });
await a.getByText('All versions (2)').waitFor();
await shot(a, 'edit-part');
check(true, 'part edited from its tile: PDF uploaded and replaced, versions listed');

// Edit the piece from its card.
await a.getByRole('button', { name: 'Done' }).click();
await a.locator('.score', { hasText: 'Florentiner Marsch' }).locator('.score-head').getByRole('link', { name: 'Edit' }).click();
await a.getByRole('heading', { name: 'Edit piece' }).waitFor();
await a.getByLabel('Genre').fill('March');
await a.getByRole('button', { name: 'Save details' }).click();
await a.locator('.ok', { hasText: 'Saved' }).waitFor();
check(await a.locator('.part-editor').count() === 2, 'edit view lists both parts');
await shot(a, 'edit-piece');
await a.getByRole('button', { name: 'Done' }).click();
await a.locator('.score.focus .tag.genre', { hasText: 'March' }).waitFor();
check(true, 'piece details edited from the library');

// An instrument in use cannot be deleted.
await nav.getByRole('link', { name: 'Instruments' }).click();
await a.locator('.instrument-list li', { hasText: 'Trumpet' }).getByRole('button', { name: 'Delete' }).click();
await a.locator('.error', { hasText: 'cannot be deleted' }).waitFor();
check(true, 'deleting an instrument in use shows the conflict');

// A member, a librarian and a group granting the trumpet parts.
await nav.getByRole('link', { name: 'Users' }).click();
const invites = {};
for (const [username, display, role] of [['anna', 'Anna', 'member'], ['carla', 'Carla', 'librarian']]) {
  await a.getByPlaceholder('Username').fill(username);
  await a.getByPlaceholder('Display name').fill(display);
  await a.locator('form select').selectOption(role);
  await a.getByRole('button', { name: 'Create and get invite link' }).click();
  await a.locator('.card.highlight', { hasText: display }).waitFor();
  invites[username] = await a.locator('.card.highlight input').inputValue();
}
check(invites.anna.includes('/invite/') && invites.carla.includes('/invite/'), 'admin got invite links for anna and carla');

await nav.getByRole('link', { name: 'Groups' }).click();
await a.getByPlaceholder('New group, e.g. Trumpets').fill('Trumpets');
await a.getByRole('button', { name: 'Add', exact: true }).click();
await a.getByRole('heading', { name: 'Trumpets' }).waitFor();
await a.locator('label.check', { hasText: 'Anna' }).locator('input').check();
await a.locator('label.check', { hasText: 'Trumpet' }).first().locator('input').check();
await a.getByRole('button', { name: 'Save', exact: true }).click();
await a.locator('.ok', { hasText: 'Saved' }).waitFor();
await shot(a, 'groups');
check(true, 'group Trumpets gets member anna and instrument Trumpet');

// A programme with the march.
await nav.getByRole('link', { name: 'Programmes' }).click();
await a.getByRole('link', { name: 'New programme' }).click();
await a.getByRole('heading', { name: 'New programme' }).waitFor();
await a.getByLabel('Title', { exact: true }).fill('Christmas concert');
await a.locator('select').selectOption({ label: 'Florentiner Marsch – Julius Fučík' });
await a.getByRole('button', { name: '+ Add' }).click();
await a.getByRole('button', { name: 'Create programme' }).click();
await a.getByRole('heading', { name: 'Christmas concert' }).waitFor();
await a.getByRole('link', { name: 'Edit' }).first().click();
await a.getByRole('heading', { name: 'Edit programme' }).waitFor();
check(await a.locator('ol.items li').count() === 1, 'programme editor shows the piece');
await a.getByLabel('Notes for the musicians').fill('Meet at 18:30.');
await a.getByRole('button', { name: 'Save programme' }).click();
await a.getByText('Meet at 18:30.').waitFor();
check(true, 'programme created and edited');

// --- Librarian: management controls, but no user administration -----------
const librarian = await browser.newContext({ viewport: { width: 1280, height: 900 } });
const l = await librarian.newPage();
watch(l);
await l.goto(invites.carla);
await l.getByLabel('New password (at least 8 characters)').fill('carla-password-1');
await l.getByLabel('Repeat password').fill('carla-password-1');
await l.getByRole('button', { name: 'Set password and log in' }).click();
await l.getByText('Florentiner Marsch').waitFor();
const lnav = l.locator('header.topbar nav');
check(await lnav.getByRole('link', { name: 'Groups' }).count() === 1
  && await lnav.getByRole('link', { name: 'Users' }).count() === 0, 'librarian sees Groups but not Users');
check(await l.getByRole('link', { name: 'Add piece' }).count() === 1
  && await l.locator('.part-edit').count() === 2, 'librarian gets Add piece and Edit on every part');
await lnav.getByRole('link', { name: 'Groups' }).click();
await l.locator('.selectable li', { hasText: 'Trumpets' }).waitFor();
check(await l.getByPlaceholder('New group, e.g. Trumpets').count() === 0, 'only admins create groups');
await librarian.close();

// --- Member: invite, library, reader, annotations -------------------------
const member = await browser.newContext({ viewport: { width: 1024, height: 1366 }, hasTouch: false });
const m = await member.newPage();
watch(m);
await m.goto(invites.anna);
await m.getByLabel('New password (at least 8 characters)').fill('anna-password-1');
await m.getByLabel('Repeat password').fill('anna-password-1');
await m.getByRole('button', { name: 'Set password and log in' }).click();
await m.getByRole('heading', { name: 'Library' }).waitFor();
await m.getByText('Florentiner Marsch').waitFor();
await shot(m, 'member-library');
check(await m.locator('.part').count() === 1, 'anna sees exactly one part');
const mnav = m.locator('header.topbar nav');
check(await mnav.getByRole('link', { name: 'Instruments' }).count() === 1
  && await mnav.getByRole('link', { name: 'Groups' }).count() === 0
  && await mnav.getByRole('link', { name: 'Users' }).count() === 0, 'member navigation: no Groups, no Users');
check(await m.getByRole('link', { name: 'Add piece' }).count() === 0
  && await m.getByRole('link', { name: 'Edit' }).count() === 0, 'member sees the library without management controls');
await mnav.getByRole('link', { name: 'Instruments' }).click();
await m.locator('.instrument-list .row', { hasText: 'Trumpet' }).waitFor();
check(await m.getByRole('button', { name: 'Delete' }).count() === 0
  && await m.getByPlaceholder('Instrument, e.g. Trumpet').count() === 0, 'member sees the instruments read-only');
await mnav.getByRole('link', { name: 'Library' }).click();

await m.getByPlaceholder('Search title, composer, tag…').fill('fucik');
await m.waitForTimeout(600);
check(await m.getByText('Florentiner Marsch').count() === 1, 'search without diacritics finds Fučík');

await m.getByRole('link', { name: 'Programmes' }).click();
check(await m.getByRole('link', { name: 'New programme' }).count() === 0, 'member cannot create programmes');
await m.getByRole('link', { name: /Christmas concert/ }).click();
await m.getByText(/All 1 sheets ready/).waitFor({ timeout: 20000 });
check(await m.getByRole('link', { name: 'Edit' }).count() === 0, 'member cannot edit the programme');
await shot(m, 'programme-prefetched');
check(true, 'programme sheets were prefetched into memory');

await m.locator('.part').first().click();
await m.locator('#page-canvas').waitFor();
// pdf.js sets the CSS size once the page is drawn.
await m.waitForFunction(() => {
  const c = document.getElementById('page-canvas');
  return c && c.style.width !== '';
}, null, { timeout: 20000 });
await m.locator('.page-no', { hasText: /\d+ \/ \d+/ }).waitFor();
await shot(m, 'reader');
const pages = await m.locator('.page-no').innerText();
check(/^1 \/ \d+$/.test(pages.trim()), `reader shows page counter "${pages.trim()}"`);
const total = Number(pages.split('/')[1]);

// Draw a stroke with the mouse.
await m.getByRole('button', { name: /Draw/ }).click();
const box = await m.locator('#ink-canvas').boundingBox();
await m.mouse.move(box.x + box.width * 0.3, box.y + box.height * 0.3);
await m.mouse.down();
for (let i = 1; i <= 20; i += 1) {
  await m.mouse.move(box.x + box.width * (0.3 + i * 0.015), box.y + box.height * (0.3 + Math.sin(i / 3) * 0.03));
}
await m.mouse.up();
await m.locator('.save-status', { hasText: /^Saved$/ }).waitFor({ timeout: 10000 });
await shot(m, 'drawn');
check(true, 'stroke drawn and saved');

async function redPixels(page) {
  return page.evaluate(() => {
    const c = document.getElementById('ink-canvas');
    const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
    let n = 0;
    for (let i = 0; i < d.length; i += 4) {
      if (d[i + 3] > 0 && d[i] > 150 && d[i + 1] < 80) n += 1;
    }
    return n;
  });
}
check(await redPixels(m) > 50, 'the stroke is on the ink canvas');

// A second stroke, then reload at once: the unsaved stroke must be sent
// on page hide (keepalive) instead of being lost.
await m.mouse.move(box.x + box.width * 0.3, box.y + box.height * 0.6);
await m.mouse.down();
await m.mouse.move(box.x + box.width * 0.6, box.y + box.height * 0.62, { steps: 10 });
await m.mouse.up();
check(await m.locator('.save-status').innerText() === 'Unsaved', 'second stroke is pending');

// Reload: both strokes must come back from the server.
await m.reload();
await m.waitForFunction(() => {
  const c = document.getElementById('page-canvas');
  return c && c.style.width !== '';
}, null, { timeout: 20000 });
await m.waitForTimeout(1500);
const objects = await m.evaluate(async () => {
  const sets = await (await fetch('/api/parts/1/annotation-sets')).json();
  return sets[0].pages.map((p) => p.revision);
});
check(await redPixels(m) > 50, 'the strokes survive a reload (stored on the server)');
check(objects.length === 1 && objects[0] === 2, `the pending stroke was flushed on reload (revisions ${objects})`);

// Keyboard / foot pedal page turn.
await m.getByRole('button', { name: /Done|Draw/ }).first().click().catch(() => {});
if (total > 1) {
  await m.keyboard.press('ArrowRight');
  await m.locator('.page-no', { hasText: `2 / ${total}` }).waitFor();
  check(true, 'arrow key turns to page 2');
  check(await redPixels(m) === 0, 'page 2 has its own (empty) annotation layer');
  await m.keyboard.press('PageUp');
  await m.locator('.page-no', { hasText: `1 / ${total}` }).waitFor();
}

// The admin does not see anna's annotations.
const sets = await a.evaluate(async () => {
  const r = await fetch('/api/parts/1/annotation-sets');
  return r.json();
});
check(Array.isArray(sets) && sets.length === 0, 'annotations are private (admin sees no sets of anna)');

await m.setViewportSize({ width: 390, height: 844 });
await m.goto(`${base}/`);
await m.getByRole('heading', { name: 'Library' }).waitFor();
await shot(m, 'phone-library');

// Ignored: the dev build's hot-reload socket, the expected 401 of the
// session check before logging in, and the expected 409 of deleting an
// instrument in use.
const relevant = errors.filter((e) => !e.includes('favicon') && !e.includes('/_dioxus')
  && !e.includes('status of 401') && !e.includes('status of 409'));
check(relevant.length === 0, `no browser errors${relevant.length ? `: ${relevant.join(' | ')}` : ''}`);
await browser.close();
console.log('smoke test passed');
