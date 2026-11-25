// Writes the sample PDF that e2e/smoke.mjs uploads (its <some.pdf> argument), so the end-to-end
// tests need no file from outside the repository: A4 pages with a few staff-like lines and a page
// mark (filled boxes, no fonts), every page different.
//
// Usage: node e2e/sample-pdf.mjs <out.pdf> [pages, default 3]

import { writeFileSync } from 'node:fs';

const [out, pagesArg] = process.argv.slice(2);
if (!out) {
  console.error('usage: node e2e/sample-pdf.mjs <out.pdf> [pages]');
  process.exit(2);
}
const pages = Number(pagesArg ?? 3);
if (!Number.isInteger(pages) || pages < 1) {
  console.error(`pages must be a positive whole number, not ${pagesArg}`);
  process.exit(2);
}

function content(n) {
  let c = '0.6 w\n';
  for (let k = 0; k < 6; k += 1) {
    const top = 760 - k * 120;
    for (let l = 0; l < 5; l += 1) c += `50 ${top - l * 6} m 545 ${top - l * 6} l S\n`;
  }
  // n boxes in the corner: the pages differ.
  for (let i = 0; i < n; i += 1) c += `${50 + i * 14} 40 10 10 re f\n`;
  return c;
}

// Objects: 1 catalog, 2 pages, then per page a page object and its content stream.
const objects = ['<< /Type /Catalog /Pages 2 0 R >>',
  `<< /Type /Pages /Kids [${Array.from({ length: pages }, (_, i) => `${3 + 2 * i} 0 R`).join(' ')}] /Count ${pages} >>`];
for (let i = 0; i < pages; i += 1) {
  const c = content(i + 1);
  objects.push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << >> /Contents ${4 + 2 * i} 0 R >>`);
  objects.push(`<< /Length ${c.length} >>\nstream\n${c}endstream`);
}
let pdf = '%PDF-1.7\n';
const offsets = [];
objects.forEach((o, i) => { offsets.push(pdf.length); pdf += `${i + 1} 0 obj\n${o}\nendobj\n`; });
const xref = pdf.length;
pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n${offsets.map((o) => `${String(o).padStart(10, '0')} 00000 n \n`).join('')}`;
pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
writeFileSync(out, pdf, 'latin1');
console.log(`wrote ${out} (${pages} pages)`);
