// Browser smoke test of the whole application, driven by Playwright.
//
// Usage: node e2e/smoke.mjs <admin-invite-url> <some.pdf> <screenshot-dir>
//
// Needs a running server with the built web client (see README) and a fresh
// data directory. It walks through the main flows: admin setup, instruments,
// the guided flow for a new piece, editing parts and pieces from the
// library, invites, groups and grants, programmes, a librarian's and a
// member's view (no management controls), a member reading and annotating,
// the system view (generated part: whole systems across pages, swipe, staff
// size, drawing in a crop).
// The assignment picker (assigned chips + searchable multi-select dropdown)
// is checked on group members and grants, part instruments and programme
// pieces: search, several entries in one go, the three ways of closing
// (outside, Escape, button), removal, persistence after a reload.
//
// BROWSER=firefox runs it in Firefox instead of Chromium.

import { chromium, firefox } from 'playwright';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [inviteUrl, pdfPath, outDir] = process.argv.slice(2);
if (!inviteUrl || !pdfPath || !outDir) {
  console.error('usage: node e2e/smoke.mjs <admin-invite-url> <some.pdf> <screenshot-dir>');
  process.exit(2);
}
mkdirSync(outDir, { recursive: true });
const base = new URL(inviteUrl).origin;

const engine = process.env.BROWSER === 'firefox' ? firefox : chromium;
const browser = await engine.launch(process.env.CHROMIUM && engine === chromium ? { executablePath: process.env.CHROMIUM } : {});
console.log(`browser: ${engine.name()}`);
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

// Opens the picker `button` inside `scope`, ticks `entries` (strings, or
// {search, name}) and closes it with `close`: 'outside' (click next to the
// panel), 'escape' or 'button' (the "Add n" button).
async function pick(page, scope, button, entries, close = 'button') {
  await scope.getByRole('button', { name: button }).click();
  const panel = page.locator('.picker-panel');
  await panel.waitFor();
  for (const e of entries) {
    const { search, name } = typeof e === 'string' ? { search: undefined, name: e } : e;
    if (search !== undefined) await panel.getByRole('combobox').fill(search);
    await panel.getByRole('option', { name }).click();
  }
  if (close === 'outside') await page.locator('.picker-backdrop').click({ position: { x: 4, y: 4 } });
  else if (close === 'escape') await page.keyboard.press('Escape');
  else await panel.getByRole('button', { name: /^Add \d+$/ }).click();
  await panel.waitFor({ state: 'detached' });
}
const chips = (scope) => scope.locator('.chip .chip-text').allInnerTexts();
// "Saved" of the picker inside `scope`.
const saved = (scope) => scope.locator('.assign-actions .ok', { hasText: 'Saved' }).waitFor();

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
for (const [name, family] of [['Trumpet', 'Brass'], ['Flute', 'Woodwind']]) {
  await a.getByPlaceholder('Instrument, e.g. Trumpet').fill(name);
  await a.getByPlaceholder('Family, e.g. Brass').fill(family);
  await a.getByRole('button', { name: '+ Add' }).click();
  await a.locator('.instrument-list .row', { hasText: name }).waitFor();
}
// The family field suggests the known families.
await a.getByPlaceholder('Instrument, e.g. Trumpet').fill('Horn');
// (The family stays filled after adding, for the next instrument of the same family.)
await a.getByPlaceholder('Family, e.g. Brass').fill('');
await a.keyboard.type('wo');
check(JSON.stringify(await a.locator('ul.suggest li').allInnerTexts()) === '["Woodwind"]', 'the family field suggests matching families');
await a.keyboard.press('Escape');
check(await a.locator('ul.suggest').count() === 0, 'Escape closes the suggestions');
await a.getByPlaceholder('Family, e.g. Brass').fill('');
await a.keyboard.type('BRA');
await a.keyboard.press('ArrowDown');
await a.keyboard.press('Enter');
check(await a.getByPlaceholder('Family, e.g. Brass').inputValue() === 'Brass', '↓ and Enter take a suggestion (without submitting)');
await a.getByRole('button', { name: '+ Add' }).click();
await a.locator('.instrument-list .row', { hasText: 'Horn' }).waitFor();
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
// Tags are chips: comma / Enter finish a tag, the input moves between chips.
const tagField = a.locator('.tag-input');
const tagChips = () => tagField.locator('.tag-chip .tag-text').allInnerTexts();
const sameTags = async (expected) => JSON.stringify(await tagChips()) === JSON.stringify(expected);
await tagField.locator('input.tag-entry').click();
await a.keyboard.type('Encore,March');
await a.keyboard.press('Enter');
check(await sameTags(['Encore', 'March']) && await a.getByRole('button', { name: 'Next: parts' }).isVisible(),
  'comma and Enter turn the typed text into tag chips without submitting the form');
await a.keyboard.type('encore,');
check(await sameTags(['Encore', 'March']) && await tagField.locator('.tag-chip.flash').count() === 1,
  'a duplicate tag (other case) is not added; the existing chip flashes');
await a.keyboard.press('Backspace');
check(await sameTags(['Encore']), 'Backspace at the start of the input removes the chip before it');
await a.keyboard.press('ArrowLeft');
// The input is focused again at its new place (next animation frame).
await a.waitForFunction(() => document.activeElement?.classList.contains('tag-entry')
  && document.activeElement.parentElement.nextElementSibling?.classList.contains('tag-chip'));
await a.keyboard.type('Festival');
await a.keyboard.press('Enter');
check(await sameTags(['Festival', 'Encore']), 'the input moves between chips with the arrow keys; a tag is inserted there');
await a.keyboard.press('Delete');
check(await sameTags(['Festival']), 'Delete at the end of the input removes the chip after it');
await a.keyboard.insertText('Christmas, New Year, Encore');
check(await sameTags(['Festival', 'Christmas', 'New Year', 'Encore']), 'pasting "a, b, c" creates three chips');
await tagField.getByRole('button', { name: 'Remove tag Festival' }).click();
check(await sameTags(['Christmas', 'New Year', 'Encore']), 'the × of a chip removes it');
await tagField.locator('input.tag-entry').click();
await a.keyboard.type('Polka');
await a.getByRole('button', { name: 'Next: parts' }).click();
await a.getByRole('heading', { name: 'Parts of “Florentiner Marsch”' }).waitFor();
const newPart = a.locator('form.new-part');
// No custom name: the name comes from instrument, key and voice.
check(await newPart.getByLabel('Key').evaluate((e) => e.tagName) === 'SELECT'
  && (await newPart.getByLabel('Key').locator('option').allInnerTexts()).includes('B♭'), 'the key is chosen from a list (with ♭ glyphs)');
await newPart.getByLabel('Key').selectOption('Bb');
await newPart.getByLabel('Voice').fill('2');
// Keyboard: the search field has the focus, Enter ticks the first match, Escape applies.
await newPart.getByRole('button', { name: '+ Add instruments' }).click();
check(await a.evaluate(() => document.activeElement?.classList.contains('picker-search')), 'the picker focuses its search field on open');
await a.keyboard.type('trum');
await a.keyboard.press('Enter');
check((await a.locator('.picker-foot').innerText()).includes('1 selected'), 'Enter ticks the first match');
await a.keyboard.press('Escape');
await a.locator('.picker-panel').waitFor({ state: 'detached' });
check((await chips(newPart)).some((c) => c.startsWith('Trumpet')), 'Escape closes the picker and applies the ticked entry');
check((await newPart.locator('.part-name-preview').innerText()).includes('Trumpet in B♭ 2')
  && await newPart.getByLabel('Custom name (optional)').getAttribute('placeholder') === 'Trumpet in B♭ 2',
  'the derived name is previewed live (instrument, key, voice)');
await newPart.locator('input[type=file]').setInputFiles(pdfPath);
await newPart.getByRole('button', { name: '+ Add part' }).click();
await a.locator('.added-parts li', { hasText: 'Trumpet in B♭ 2' }).getByText(/Version 1/).waitFor({ timeout: 30000 });
check(true, 'a part without custom name is listed with its derived name');
// A second part right away, this time without a PDF and with a custom name.
check((await chips(newPart)).length === 0 && await newPart.getByLabel('Key').inputValue() === ''
  && await newPart.getByLabel('Voice').inputValue() === '', 'the next part starts empty (instruments, key, voice)');
await newPart.getByLabel('Custom name (optional)').fill('Solo Flute');
await pick(a, newPart, '+ Add instruments', ['Flute'], 'outside');
check((await chips(newPart)).some((c) => c.startsWith('Flute')), 'a click outside the picker applies the selection');
await newPart.getByRole('button', { name: '+ Add part' }).click();
await a.locator('.added-parts li', { hasText: 'Solo Flute' }).getByText('No PDF yet').waitFor();
check(true, 'a custom name overrides the derived name');
await shot(a, 'add-piece-parts');
check(true, 'guided flow created the piece with two parts and uploaded a PDF');
await a.getByRole('button', { name: 'Finish' }).click();
await a.locator('.score.focus', { hasText: 'Florentiner Marsch' }).waitFor();
check(await a.locator('.score.focus .part-cell').count() === 2, 'the new piece is highlighted in the library with both parts');
check(JSON.stringify(await a.locator('.score.focus .part-label').allInnerTexts()) === '["Solo Flute","Trumpet in B♭ 2"]',
  'library tiles show the part names (voice included), sorted');

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
// Instruments of an existing part are saved at once, the typed label is not.
const editor = a.locator('.part-editor');
await editor.getByLabel('Custom name (optional)').fill('Flute (unsaved)');
await pick(a, editor, '+ Add instruments', [{ search: 'tru', name: 'Trumpet' }]);
await saved(editor);
await a.reload();
await a.locator('.part-editor .chip').first().waitFor();
check(JSON.stringify((await chips(a.locator('.part-editor'))).map((c) => c.split(' ')[0]).sort()) === '["Flute","Trumpet"]'
  && await a.locator('.part-editor').getByLabel('Custom name (optional)').inputValue() === 'Solo Flute',
  'part instruments are saved immediately with the stored name (not the unsaved one)');
await a.locator('.part-editor').getByRole('button', { name: 'Remove Trumpet' }).click();
await saved(a.locator('.part-editor'));
await a.reload();
await a.locator('.part-editor .chip').first().waitFor();
check((await chips(a.locator('.part-editor'))).length === 1, 'removing a part instrument is saved immediately');
// Clearing the custom name brings the derived name back; key and voice are saved with "Save part".
await a.locator('.part-editor').getByLabel('Custom name (optional)').fill('');
await a.locator('.part-editor').getByLabel('Key').selectOption('C');
await a.locator('.part-editor').getByLabel('Voice').fill('1a');
check(await a.locator('.part-editor h3.part-title').innerText() === 'Flute in C 1a', 'the part editor previews the name live');
await a.locator('.part-editor').getByRole('button', { name: 'Save part' }).click();
await a.locator('.part-editor .ok', { hasText: 'Saved' }).waitFor();
await a.reload();
await a.getByText('Flute in C 1a · Florentiner Marsch').waitFor();
check(await a.locator('.part-editor').getByLabel('Key').inputValue() === 'C'
  && await a.locator('.part-editor').getByLabel('Voice').inputValue() === '1a', 'key and free-text voice are stored');

// Edit the piece from its card.
await a.getByRole('button', { name: 'Done' }).click();
await a.locator('.score', { hasText: 'Florentiner Marsch' }).locator('.score-head').getByRole('link', { name: 'Edit' }).click();
await a.getByRole('heading', { name: 'Edit piece' }).waitFor();
await a.getByLabel('Genre').fill('March');
check(await sameTags(['Christmas', 'New Year', 'Encore', 'Polka']), 'tags typed without a final comma are kept when leaving the field');
await tagField.getByRole('button', { name: 'Remove tag New Year' }).click();
await a.getByRole('button', { name: 'Save details' }).click();
await a.locator('.ok', { hasText: 'Saved' }).waitFor();
await a.reload();
await tagField.locator('.tag-chip').first().waitFor();
check(await sameTags(['Christmas', 'Encore', 'Polka']), 'tags edited in the piece editor are saved');
// Suggestions of tags used elsewhere in the library.
await a.evaluate(async () => {
  const r = await fetch('/api/scores', { method: 'POST', headers: { 'content-type': 'application/json', 'x-musicus-csrf': '1' },
    body: JSON.stringify({ title: 'Kaiserwalzer', tags: ['New Year', 'Waltz', 'Fête'] }) });
  if (!r.ok) throw new Error(`POST /api/scores: ${r.status}`);
});
await a.reload();
await tagField.locator('.tag-chip').first().waitFor();
await tagField.locator('input.tag-entry').click();
await a.keyboard.type('fete');
check(JSON.stringify(await tagField.locator('ul.suggest li').allInnerTexts()) === '["Fête"]', 'tag suggestions ignore case and diacritics');
await a.keyboard.press('ArrowDown');
await a.keyboard.press('Enter');
check(await sameTags(['Christmas', 'Encore', 'Polka', 'Fête']), 'a suggestion taken with ↓ and Enter becomes a chip');
await a.keyboard.type('w');
check(JSON.stringify(await tagField.locator('ul.suggest li').allInnerTexts()) === '["Waltz","New Year"]', 'tags starting with the text come first');
await tagField.locator('ul.suggest li', { hasText: 'New Year' }).click();
check(await sameTags(['Christmas', 'Encore', 'Polka', 'Fête', 'New Year']), 'a click on a suggestion makes it a chip');
await a.keyboard.type('enc');
check(await tagField.locator('ul.suggest').count() === 0, 'tags already chosen are not suggested');
await a.keyboard.press('Escape');
await shot(a, 'tag-suggestions');
check(JSON.stringify(await a.locator('.part-editor h3.part-title').allInnerTexts()) === '["Flute in C 1a","Trumpet in B♭ 2"]',
  'edit view lists both parts by name');
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
for (const [username, display, role] of [['anna', 'Anna', 'member'], ['carla', 'Carla', 'librarian'],
  ['zoe', 'Zoë Brandt', 'member'], ['ben', 'Ben', 'member']]) {
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
const groupCard = a.locator('section.card', { has: a.getByRole('heading', { name: 'Trumpets' }) });
const membersBox = groupCard.locator('.assign').first();
check((await membersBox.innerText()).includes('No members yet.'), 'empty member list says so');
await groupCard.getByRole('button', { name: '+ Add members' }).click();
const panel = a.locator('.picker-panel');
await panel.getByRole('combobox').fill('zoe');
check(await panel.getByRole('option').count() === 1
  && (await panel.getByRole('option').innerText()).includes('Zoë Brandt'), 'search ignores case and diacritics (zoe finds Zoë)');
await panel.getByRole('option', { name: 'Zoë Brandt' }).click();
await panel.getByRole('combobox').fill('ANN');
await panel.getByRole('option', { name: 'Anna' }).click();
await panel.getByRole('combobox').fill('');
check(await panel.isVisible() && (await panel.locator('.picker-foot').innerText()).includes('2 selected'),
  'ticking does not close the picker; two entries selected in one go');
await shot(a, 'groups-picker');
await a.locator('.picker-backdrop').click({ position: { x: 4, y: 4 } });
await saved(membersBox);
check(JSON.stringify((await chips(membersBox)).map((c) => c.split(' ')[0])) === '["Anna","Zoë"]',
  'closing by clicking outside adds both members');
await pick(a, groupCard, '+ Add instruments', ['Trumpet'], 'escape');
await saved(groupCard.locator('.assign').nth(1));
await a.reload();
await a.locator('.selectable li', { hasText: 'Trumpets' }).click();
await groupCard.locator('.chip').first().waitFor();
check((await chips(membersBox)).length === 2 && (await chips(groupCard.locator('.assign').nth(1)))[0].startsWith('Trumpet'),
  'members and instrument grant are stored (after a reload)');
await membersBox.getByRole('button', { name: 'Remove Zoë Brandt' }).click();
await saved(membersBox);
await a.reload();
await a.locator('.selectable li', { hasText: 'Trumpets' }).click();
await groupCard.locator('.chip').first().waitFor();
check(JSON.stringify((await chips(membersBox)).map((c) => c.split(' ')[0])) === '["Anna"]', 'removing a member is saved');
check(await groupCard.getByRole('button', { name: 'Save', exact: true }).count() === 0, 'the group editor needs no Save button');
await shot(a, 'groups');
check(true, 'group Trumpets gets member anna and instrument Trumpet');

// Two more pieces (without parts) so the programme picker has something to choose from.
for (const [title, composer] of [['Radetzky-Marsch', 'Johann Strauss'], ['Pizzicato-Polka', 'Johann Strauss']]) {
  await a.evaluate(async (body) => {
    const r = await fetch('/api/scores', { method: 'POST', headers: { 'content-type': 'application/json', 'x-musicus-csrf': '1' },
      body: JSON.stringify(body) });
    if (!r.ok) throw new Error(`POST /api/scores: ${r.status}`);
  }, { title, subtitle: null, composer, arranger: null, genre: null, tags: [], notes: null });
}
// A programme with the march.
await nav.getByRole('link', { name: 'Programmes' }).click();
await a.getByRole('link', { name: 'New programme' }).click();
await a.getByRole('heading', { name: 'New programme' }).waitFor();
await a.getByLabel('Title', { exact: true }).fill('Christmas concert');
await a.getByRole('button', { name: '+ Add pieces' }).click();
await a.locator('.picker-panel').getByRole('combobox').fill('strauss');
check(await a.locator('.picker-panel').getByRole('option').count() === 2, 'programme picker searches the composer');
await a.locator('.picker-panel').getByRole('option', { name: /Radetzky/ }).click();
await a.locator('.picker-panel').getByRole('combobox').fill('fucik');
await a.locator('.picker-panel').getByRole('option', { name: /Florentiner/ }).click();
await a.locator('.picker-panel').getByRole('button', { name: 'Add 2' }).click();
check(JSON.stringify(await a.locator('ol.items li .grow').allInnerTexts()) === '["Radetzky-Marsch","Florentiner Marsch"]',
  'several pieces added in one go, in the order they were ticked');
await a.locator('ol.items li').first().getByRole('button', { name: 'Move down' }).click();
check(JSON.stringify(await a.locator('ol.items li .grow').allInnerTexts()) === '["Florentiner Marsch","Radetzky-Marsch"]',
  'the order stays editable with the arrows');
await a.getByRole('button', { name: 'Create programme' }).click();
await a.getByRole('heading', { name: 'Christmas concert' }).waitFor();
await a.getByRole('link', { name: 'Edit' }).first().click();
await a.getByRole('heading', { name: 'Edit programme' }).waitFor();
check(await a.locator('ol.items li').count() === 2, 'programme editor shows the pieces');
await a.getByRole('button', { name: '+ Add pieces' }).click();
await a.locator('.picker-panel').getByRole('combobox').fill('florentiner');
check((await a.locator('.picker-panel').getByRole('option').innerText()).includes('already in the programme'),
  'pieces already in the programme stay choosable and say so');
await a.locator('.picker-panel').getByRole('option', { name: /Florentiner/ }).click();
await a.keyboard.press('Escape');
check(await a.locator('ol.items li').count() === 3, 'a piece can be added a second time (encore)');
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
await l.locator('.selectable li', { hasText: 'Trumpets' }).click();
const lcard = l.locator('section.card', { has: l.getByRole('heading', { name: 'Trumpets' }) });
check(await lcard.getByRole('button', { name: '+ Add members' }).count() === 0
  && await lcard.getByRole('button', { name: '+ Add scores' }).count() === 1, 'librarian edits grants but not members');
const scoreBox = lcard.locator('.assign').last();
await pick(l, lcard, '+ Add scores', [{ search: 'radetzky', name: /Radetzky/ }]);
await saved(scoreBox);
check((await chips(scoreBox))[0].startsWith('Radetzky-Marsch'), 'librarian grants a whole score');
await scoreBox.getByRole('button', { name: 'Remove Radetzky-Marsch' }).click();
await saved(scoreBox);
await l.reload();
await l.locator('.selectable li', { hasText: 'Trumpets' }).click();
await lcard.getByText('No whole scores granted.').waitFor();
check(true, 'removed score grant stays removed after a reload');
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
await m.getByPlaceholder('Search title, composer, tag…').fill('trumpet 2');
await m.waitForTimeout(600);
check(await m.getByText('Florentiner Marsch').count() === 1, 'search finds a piece by the derived name of the own part');
await m.getByPlaceholder('Search title, composer, tag…').fill('flute');
await m.waitForTimeout(600);
check(await m.getByText('Florentiner Marsch').count() === 0, 'search ignores parts the member cannot read');

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
check((await m.locator('.reader-title').innerText()).includes('Trumpet in B♭ 2'), 'the reader title shows the part name with key and voice');
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
// After the reload the reader is in reading mode again (Draw is not active).
check(await m.getByRole('button', { name: /Done/ }).count() === 0, 'a reload starts in reading mode');
if (total > 1) {
  await m.keyboard.press('ArrowRight');
  await m.locator('.page-no', { hasText: `2 / ${total}` }).waitFor();
  check(true, 'arrow key turns to page 2');
  check(await redPixels(m) === 0, 'page 2 has its own (empty) annotation layer');
  await m.keyboard.press('PageUp');
  await m.locator('.page-no', { hasText: `1 / ${total}` }).waitFor();
  // Swipe left / right (mouse drag; touch sends the same pointer events).
  const sb = await m.locator('.stage').boundingBox();
  const swipe = async (from, to) => {
    await m.mouse.move(sb.x + sb.width * from, sb.y + sb.height * 0.5);
    await m.mouse.down();
    await m.mouse.move(sb.x + sb.width * to, sb.y + sb.height * 0.51, { steps: 8 });
    await m.mouse.up();
  };
  await swipe(0.7, 0.3);
  await m.locator('.page-no', { hasText: `2 / ${total}` }).waitFor();
  await swipe(0.3, 0.7);
  await m.locator('.page-no', { hasText: `1 / ${total}` }).waitFor();
  check(true, 'swipes turn pages');
}

// --- System view (whole systems across pages) -----------------------------
// A generated part: 2 A4 pages with 8 single-staff systems each (lines 5 pt apart),
// read on an 8" tablet in landscape (1280 × 800).
function staffPdf(pages, staves) {
  let content = '0.6 w\n250 790 m 345 790 l 4 w S 0.6 w\n';
  for (let k = 0; k < staves; k += 1) {
    const top = 740 - k * 80;
    for (let l = 0; l < 5; l += 1) content += `40 ${top - l * 5} m 555 ${top - l * 5} l S\n`;
    for (let b = 1; b < 4; b += 1) { const x = 40 + b * 128.75; content += `${x} ${top} m ${x} ${top - 20} l S\n`; }
    // A chord symbol above and a dynamic mark below each staff, like real music.
    content += `60 ${top + 12} 14 6 re f 90 ${top - 38} 10 6 re f\n`;
  }
  const objects = ['<< /Type /Catalog /Pages 2 0 R >>',
    `<< /Type /Pages /Kids [${Array.from({ length: pages }, (_, i) => `${i + 4} 0 R`).join(' ')}] /Count ${pages} >>`,
    `<< /Length ${content.length} >>\nstream\n${content}endstream`];
  for (let i = 0; i < pages; i += 1) objects.push('<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Contents 3 0 R >>');
  let pdf = '%PDF-1.7\n';
  const offsets = [];
  objects.forEach((o, i) => { offsets.push(pdf.length); pdf += `${i + 1} 0 obj\n${o}\nendobj\n`; });
  const xref = pdf.length;
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n${offsets.map((o) => `${String(o).padStart(10, '0')} 00000 n \n`).join('')}`;
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return pdf;
}
const systemsPart = await a.evaluate(async (pdf) => {
  const h = { 'content-type': 'application/json', 'x-musicus-csrf': '1' };
  const trumpet = (await (await fetch('/api/instruments')).json()).find((i) => i.name === 'Trumpet');
  const score = await (await fetch('/api/scores', { method: 'POST', headers: h, body: JSON.stringify({ title: 'Etude in systems' }) })).json();
  const part = await (await fetch(`/api/scores/${score.id}/parts`, { method: 'POST', headers: h, body: JSON.stringify({ instrument_ids: [trumpet.id] }) })).json();
  const up = await (await fetch(`/api/parts/${part.id}/file`, { method: 'PUT', headers: { 'x-musicus-csrf': '1' }, body: pdf })).json();
  for (let i = 0; i < 60; i += 1) {
    const l = await (await fetch(`/api/files/${up.file.sha256}/layout`)).json();
    if (l.state !== 'pending') return { id: part.id, layout: l };
    await new Promise((r) => setTimeout(r, 500));
  }
  return { id: part.id, layout: null };
}, staffPdf(2, 8));
const detected = systemsPart.layout.pages.map((p) => p.regions.filter((r) => r.kind === 'system').length);
check(JSON.stringify(detected) === '[8,8]', `the server detected the systems of a new upload in the background (${detected})`);

await m.setViewportSize({ width: 1280, height: 800 });
await m.goto(`${base}/parts/${systemsPart.id}`);
await m.locator('.page-no', { hasText: '1 / 2' }).waitFor();
await m.getByRole('button', { name: /Systems/ }).click();
await m.locator('.system-view .crop').first().waitFor();
const viewInfo = async () => m.locator('.system-view').getAttribute('data-view');
const crops = await m.locator('.system-view .crop').count();
check(crops >= 3 && crops <= 9, `system view shows whole systems (${crops} items: title and systems, of 17)`);
await m.waitForFunction(() => [...document.querySelectorAll('.system-view .crop canvas:first-child')].every((c) =>
  c.style.width !== '' && c.width >= Math.floor(parseFloat(c.style.width) * (window.devicePixelRatio || 1)) - 1),
null, { timeout: 20000 });
check(true, 'crops are rendered at device resolution');
await shot(m, 'system-view');
check((await m.evaluate(async () => (await (await fetch('/api/auth/me')).json()).reader.mode)) === 'systems',
  'the view setting is stored on the server');
// Turn through the sequence: it continues from page 1 to page 2 within the same view sequence.
const [, viewCount] = (await viewInfo()).split('/').map(Number);
let crossed = false;
for (let i = 1; i < viewCount; i += 1) {
  await m.keyboard.press('ArrowRight');
  await m.locator('.system-view').and(m.locator(`[data-view="${i + 1}/${viewCount}"]`)).waitFor();
  crossed ||= (await m.locator('.page-no').innerText()).trim() === '2 / 2';
}
check(crossed && viewCount >= 3, `arrow keys walk through ${viewCount} views across the page boundary`);
// Turning the tablet re-plans the views and keeps the reading position (still page 2).
await m.setViewportSize({ width: 800, height: 1280 });
await m.waitForTimeout(500);
check((await m.locator('.page-no').innerText()).trim() === '2 / 2', 'portrait keeps the reading position');
await m.setViewportSize({ width: 1280, height: 800 });
await m.locator(`[data-view="${viewCount}/${viewCount}"]`).waitFor();
// Swipe right goes back one view.
const stageBox = await m.locator('.stage').boundingBox();
await m.mouse.move(stageBox.x + stageBox.width * 0.3, stageBox.y + stageBox.height * 0.5);
await m.mouse.down();
await m.mouse.move(stageBox.x + stageBox.width * 0.7, stageBox.y + stageBox.height * 0.52, { steps: 8 });
await m.mouse.up();
await m.locator(`[data-view="${viewCount - 1}/${viewCount}"]`).waitFor();
check(true, 'a swipe to the right turns back one view');
// Bigger staff size: fewer systems per view.
await m.getByLabel('Staff size').selectOption('S');
await m.waitForTimeout(300);
const small = Number((await viewInfo()).split('/')[1]);
await m.getByLabel('Staff size').selectOption('XL');
await m.waitForTimeout(300);
const large = Number((await viewInfo()).split('/')[1]);
check(large >= small, `a bigger staff size needs at least as many views (S ${small}, XL ${large})`);
// The size menu keeps the keyboard focus; give it back to the reader (arrows would change the size).
await m.locator('.reader-title').click();
// Draw in the system view; the stroke is stored in page coordinates and appears in the page view.
for (let i = 0; i < 20 && (await m.locator('.system-view').getAttribute('data-view')).split('/')[0] !== '1'; i += 1) {
  await m.keyboard.press('ArrowLeft');
  await m.waitForTimeout(150);
}
await m.getByRole('button', { name: /Draw/ }).click();
// Crop 0 is the title line; draw on the first system.
const cropBox = await m.locator('#crop-ink-1').boundingBox();
await m.mouse.move(cropBox.x + cropBox.width * 0.2, cropBox.y + cropBox.height * 0.5);
await m.mouse.down();
await m.mouse.move(cropBox.x + cropBox.width * 0.6, cropBox.y + cropBox.height * 0.55, { steps: 12 });
await m.mouse.up();
await m.locator('.save-status', { hasText: /^Saved$/ }).waitFor({ timeout: 10000 });
await m.getByRole('button', { name: /Done/ }).click();
await m.getByRole('button', { name: /Systems/ }).click();
await m.locator('#page-canvas').waitFor();
await m.waitForTimeout(800);
check(await redPixels(m) > 50, 'a stroke drawn in the system view shows in the page view');
check((await m.evaluate(async () => (await (await fetch('/api/auth/me')).json()).reader.mode)) === 'page',
  'switching back to the page view is stored too');

// The admin does not see anna's annotations.
const sets = await a.evaluate(async () => {
  const r = await fetch('/api/parts/1/annotation-sets');
  return r.json();
});
check(Array.isArray(sets) && sets.length === 0, 'annotations are private (admin sees no sets of anna)');

// The picker on a phone: a sheet below the top bar, inside the screen.
await a.setViewportSize({ width: 390, height: 844 });
await a.goto(`${base}/groups`);
await a.locator('.selectable li', { hasText: 'Trumpets' }).click();
await a.getByRole('button', { name: '+ Add members' }).click();
await a.locator('.picker-panel').getByRole('combobox').fill('b');
const pbox = await a.locator('.picker-panel').boundingBox();
check(pbox.x >= 0 && pbox.x + pbox.width <= 390 && pbox.y >= 56, 'picker panel fits a phone screen');
await shot(a, 'phone-picker');
await a.keyboard.press('Escape');

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
