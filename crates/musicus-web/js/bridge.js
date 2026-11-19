// Thin bridge between the Rust client and browser APIs that have no good
// Rust equivalent: pdf.js page rendering, the in-memory prefetch cache,
// the screen wake lock and fullscreen.
//
// Nothing here persists: downloaded sheets live in memory until the tab
// is closed (the client must not store data on the device for now).

const PDFJS = '/vendor/pdfjs/pdf.min.mjs';
const PDFJS_WORKER = '/vendor/pdfjs/pdf.worker.min.mjs';
const PDFJS_WASM = '/vendor/pdfjs/wasm/';

/** url -> Promise<Uint8Array>: the raw files of tonight's programme. */
const bytes = new Map();
/** url -> Promise<PDFDocumentProxy> */
const docs = new Map();
/** canvas id -> running RenderTask */
const tasks = new Map();
let pdfjsLib = null;

function pdfjs() {
  if (!pdfjsLib) {
    pdfjsLib = import(PDFJS).then((lib) => {
      lib.GlobalWorkerOptions.workerSrc = PDFJS_WORKER;
      return lib;
    });
  }
  return pdfjsLib;
}

async function download(url) {
  const response = await fetch(url, { credentials: 'same-origin' });
  if (!response.ok) {
    throw new Error(`download failed: HTTP ${response.status}`);
  }
  return new Uint8Array(await response.arrayBuffer());
}

/** Downloads a file into memory unless it is already there or underway. */
export function prefetch(url) {
  if (!bytes.has(url)) {
    const p = download(url);
    bytes.set(url, p);
    p.catch(() => bytes.delete(url));
  }
  return bytes.get(url).then(() => undefined);
}

export function isCached(url) {
  return bytes.has(url);
}

function open(url) {
  if (!docs.has(url)) {
    const p = (async () => {
      const lib = await pdfjs();
      await prefetch(url);
      const data = await bytes.get(url);
      // pdf.js hands the buffer to its worker; give it a copy so the
      // in-memory cache keeps its bytes.
      // wasmUrl: decoders for JBIG2 / JPEG 2000 scans (else those pages stay blank).
      return lib.getDocument({ data: data.slice(), wasmUrl: PDFJS_WASM }).promise;
    })();
    docs.set(url, p);
    p.catch(() => docs.delete(url));
  }
  return docs.get(url);
}

export async function pageCount(url) {
  return (await open(url)).numPages;
}

// --- Sharp rendering -----------------------------------------------------
//
// A canvas is sharp only if the browser shows its bitmap 1:1. Browsers put a
// canvas on whole device pixels themselves (also when it is centred on a half
// pixel) and paint it round(left + width) - round(left) device pixels wide; if
// that differs from the backing store, the whole bitmap is resampled, which
// blurs every staff line. So sizes are chosen in whole device pixels and the
// CSS size is derived from them (musicus_api::pixels::device_size, same rule
// as `deviceSize` below): CSS width x ratio is then a whole number and the
// painted size equals the backing size wherever the canvas lands.

/** Keep equal to musicus_api::pixels::MAX_CANVAS_PIXELS. */
const MAX_CANVAS_PIXELS = 16777216;

/**
 * Device pixels per CSS pixel the reader renders at: devicePixelRatio times
 * the pinch zoom (rounded up to quarter steps, so a pinch does not re-render
 * at every intermediate scale).
 */
export function pixelRatio() {
  const dpr = window.devicePixelRatio || 1;
  const pinch = window.visualViewport ? window.visualViewport.scale : 1;
  return pinch > 1.01 ? dpr * Math.ceil(pinch * 4) / 4 : dpr;
}

let ratioChange = null;

/**
 * Resolves with the new pixelRatio() once it changed and settled for 250 ms:
 * browser zoom, a window moved to another screen, a pinch zoom. One shared
 * set of listeners, however many readers wait.
 */
export function pixelRatioChanged() {
  if (!ratioChange) {
    ratioChange = new Promise((resolve) => {
      const start = pixelRatio();
      const media = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
      let timer = 0;
      const stop = () => {
        clearTimeout(timer);
        media.removeEventListener('change', check);
        window.removeEventListener('resize', check);
        window.visualViewport?.removeEventListener('resize', check);
      };
      function check() {
        clearTimeout(timer);
        timer = setTimeout(() => {
          const now = pixelRatio();
          if (now !== start) {
            stop();
            ratioChange = null;
            resolve(now);
          }
        }, 250);
      }
      media.addEventListener('change', check);
      window.addEventListener('resize', check);
      window.visualViewport?.addEventListener('resize', check);
    });
  }
  return ratioChange;
}

/** Same rule as musicus_api::pixels::device_size: whole pixels, capped. */
function deviceSize(cssW, cssH, ratio) {
  cssW = Math.max(0, cssW);
  cssH = Math.max(0, cssH);
  let r = Number.isFinite(ratio) && ratio > 0 ? ratio : 1;
  const area = cssW * cssH * r * r;
  if (area > MAX_CANVAS_PIXELS) r *= Math.sqrt(MAX_CANVAS_PIXELS / area);
  let width = Math.max(1, Math.round(cssW * r));
  let height = Math.max(1, Math.round(cssH * r));
  if (width * height > MAX_CANVAS_PIXELS) {
    width = Math.max(1, Math.floor(cssW * r));
    height = Math.max(1, Math.floor(cssH * r));
  }
  return { width, height, ratio: r };
}

/** Shows `bitmap` in the canvas: same backing size, CSS size = pixels / ratio. */
function show(canvas, bitmap, width, height, ratio) {
  canvas.width = width;
  canvas.height = height;
  canvas.style.width = `${width / ratio}px`;
  canvas.style.height = `${height / ratio}px`;
  canvas.getContext('2d').drawImage(bitmap, 0, 0);
}

/**
 * Renders page `index` (0-based) as large as fits into boxWidth x boxHeight
 * CSS pixels at `ratio` device pixels per CSS pixel (pixelRatio()). Returns
 * [cssWidth, cssHeight] of the drawn page (exactly its backing pixels /
 * ratio), or null if a newer render replaced this one.
 */
export async function renderPage(url, index, canvasId, boxWidth, boxHeight, ratio) {
  const pdf = await open(url);
  const page = await pdf.getPage(index + 1);
  const base = page.getViewport({ scale: 1 });
  const fit = Math.max(0.1, Math.min(boxWidth / base.width, boxHeight / base.height));
  const size = deviceSize(base.width * fit, base.height * fit, ratio);
  // Device pixels per pt; the width fills `size.width` exactly.
  const viewport = page.getViewport({ scale: size.width / base.width });

  // Render off-screen first so the visible page never flashes blank.
  const off = document.createElement('canvas');
  off.width = size.width;
  off.height = size.height;
  tasks.get(canvasId)?.cancel();
  const task = page.render({ canvas: off, canvasContext: off.getContext('2d'), viewport });
  tasks.set(canvasId, task);
  try {
    await task.promise;
  } catch (e) {
    if (e && e.name === 'RenderingCancelledException') {
      return null;
    }
    throw e;
  } finally {
    if (tasks.get(canvasId) === task) {
      tasks.delete(canvasId);
    }
  }
  const canvas = document.getElementById(canvasId);
  if (!canvas) {
    return null;
  }
  show(canvas, off, size.width, size.height, size.ratio);
  return [size.width / size.ratio, size.height / size.ratio];
}

/**
 * System view: rendered crops, keyed by file, page, crop and pixel size. Holds
 * the current and the next view (pre-rendered), so a page turn shows a
 * finished bitmap at once. Bounded by count and pixels; memory only.
 */
const crops = new Map();
const MAX_CROPS = 24;
const MAX_CROP_PIXELS = 3 * MAX_CANVAS_PIXELS;
/** canvas id -> key of the crop it should show (newest request wins). */
const wantedCrop = new Map();

function cropKey(url, index, x0, y0, x1, y1, width, height) {
  return [url, index, x0, y0, x1, y1, width, height].join('|');
}

/** Pixels of the bitmap a key names (its last two fields). */
function keyPixels(key) {
  const f = key.split('|');
  return Number(f[f.length - 2]) * Number(f[f.length - 1]);
}

function forget(key) {
  const p = crops.get(key);
  crops.delete(key);
  // Free the bitmap now (Safari reclaims canvas memory late).
  p?.then((b) => { if (![...wantedCrop.values()].includes(key)) b.canvas.width = 0; }).catch(() => {});
}

/**
 * Renders only the crop (0..1 of the page) into exactly width x height device
 * pixels: pdf.js viewport with offset, scale = width / crop width in pt.
 */
function cropBitmap(url, index, x0, y0, x1, y1, width, height) {
  const key = cropKey(url, index, x0, y0, x1, y1, width, height);
  if (crops.has(key)) {
    // Most recently used last.
    const p = crops.get(key);
    crops.delete(key);
    crops.set(key, p);
    return p;
  }
  const p = (async () => {
    const pdf = await open(url);
    const page = await pdf.getPage(index + 1);
    const base = page.getViewport({ scale: 1 });
    const scale = width / ((x1 - x0) * base.width);
    const viewport = page.getViewport({ scale, offsetX: -x0 * base.width * scale, offsetY: -y0 * base.height * scale });
    const off = document.createElement('canvas');
    off.width = width;
    off.height = height;
    await page.render({ canvas: off, canvasContext: off.getContext('2d'), viewport }).promise;
    return { canvas: off };
  })();
  crops.set(key, p);
  p.catch(() => crops.delete(key));
  let pixels = 0;
  for (const k of crops.keys()) pixels += keyPixels(k);
  while (crops.size > 1 && (crops.size > MAX_CROPS || pixels > MAX_CROP_PIXELS)) {
    const oldest = crops.keys().next().value;
    pixels -= keyPixels(oldest);
    forget(oldest);
  }
  return p;
}

/**
 * Shows a crop of page `index` in the canvas `canvasId`: width x height device
 * pixels at `ratio` (from musicus_api::pixels::device_size). Returns
 * [cssWidth, cssHeight], or null if a newer request for the same canvas
 * replaced this one.
 */
export async function renderCrop(url, index, canvasId, x0, y0, x1, y1, width, height, ratio) {
  const key = cropKey(url, index, x0, y0, x1, y1, width, height);
  wantedCrop.set(canvasId, key);
  let bitmap = await cropBitmap(url, index, x0, y0, x1, y1, width, height);
  if (bitmap.canvas.width === 0) {
    // Evicted and freed while this request waited: render again.
    bitmap = await cropBitmap(url, index, x0, y0, x1, y1, width, height);
  }
  const canvas = document.getElementById(canvasId);
  if (!canvas || wantedCrop.get(canvasId) !== key) {
    return null;
  }
  show(canvas, bitmap.canvas, width, height, ratio);
  return [width / ratio, height / ratio];
}

/** Renders a crop into the memory cache only (the next view). */
export function prerenderCrop(url, index, x0, y0, x1, y1, width, height) {
  cropBitmap(url, index, x0, y0, x1, y1, width, height).catch(() => {});
}

/** Drops everything not in `keep` (array of urls) from memory. */
export function retainOnly(keep) {
  const wanted = new Set(keep);
  for (const url of [...bytes.keys()]) {
    if (!wanted.has(url)) {
      bytes.delete(url);
      for (const key of [...crops.keys()]) {
        if (key.startsWith(`${url}|`)) forget(key);
      }
      const doc = docs.get(url);
      docs.delete(url);
      doc?.then((d) => d.destroy()).catch(() => {});
    }
  }
}

/**
 * key -> { url, revision, bytes }: the latest unsaved annotation pages. The
 * reader saves them 1.5 s after the last stroke; if the tab is closed or
 * reloaded before that, they are sent with a keepalive request (the browser
 * completes those even while the page unloads).
 */
const pendingPages = new Map();

export function setPending(key, url, revision, bytes) {
  // `bytes` is a view into WebAssembly memory, valid only during this call.
  pendingPages.set(key, { url, revision, bytes: bytes.slice() });
}

export function clearPending(key) {
  pendingPages.delete(key);
}

window.addEventListener('pagehide', () => {
  for (const { url, revision, bytes } of pendingPages.values()) {
    fetch(url, {
      method: 'PUT',
      keepalive: true,
      credentials: 'same-origin',
      headers: {
        'content-type': 'application/octet-stream',
        'if-match': `"${revision}"`,
        'x-musicus-csrf': '1',
      },
      body: bytes,
    }).catch(() => {});
  }
  pendingPages.clear();
});

let wakeLock = null;

/** Keeps the screen on while reading (re-acquired after tab switches). */
export async function keepAwake(on) {
  try {
    if (on && !wakeLock && 'wakeLock' in navigator) {
      wakeLock = await navigator.wakeLock.request('screen');
      wakeLock.addEventListener('release', () => { wakeLock = null; });
    } else if (!on && wakeLock) {
      await wakeLock.release();
      wakeLock = null;
    }
  } catch (_) {
    // Not allowed (e.g. no user gesture yet or insecure context): ignore.
  }
}

document.addEventListener('visibilitychange', () => {
  if (document.visibilityState === 'visible' && document.body.dataset.keepAwake === 'on') {
    keepAwake(true);
  }
});

export function toggleFullscreen() {
  if (document.fullscreenElement) {
    document.exitFullscreen?.();
  } else {
    document.documentElement.requestFullscreen?.().catch(() => {});
  }
}

/** Copies text to the clipboard; resolves to false if not allowed. */
export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch (_) {
    return false;
  }
}

// --- Mass import: files of a selected or dropped folder ----------------------
// The File objects stay in this array (memory only, R-SEC-001); Rust gets
// their paths and sizes, reads slices for hashing and asks for uploads by index.
let importFiles = [];

function keepFiles(list) {
  importFiles = list;
  return list.map((e) => [e.path, e.file.size]);
}

// Files chosen with <input type=file webkitdirectory>: [[path, size], …].
export function pickFolder(inputId) {
  const input = document.getElementById(inputId);
  const files = input && input.files ? Array.from(input.files) : [];
  const out = keepFiles(files.map((f) => ({ path: f.webkitRelativePath || f.name, file: f })));
  if (input) input.value = '';
  return out;
}

function readAllEntries(reader) {
  return new Promise((resolve, reject) => {
    const all = [];
    const next = () => reader.readEntries((batch) => {
      if (batch.length === 0) resolve(all);
      else { all.push(...batch); next(); }
    }, reject);
    next();
  });
}

async function walk(entry, out) {
  if (entry.isFile) {
    const file = await new Promise((resolve, reject) => entry.file(resolve, reject));
    out.push({ path: entry.fullPath.replace(/^\/+/, ''), file });
  } else if (entry.isDirectory) {
    for (const child of await readAllEntries(entry.createReader())) await walk(child, out);
  }
}

// Resolves with [[path, size], …] at the next drop of a folder (or files)
// onto the element; dragging over it marks it with the class `drag`.
export function dropFolder(zoneId) {
  const zone = document.getElementById(zoneId);
  if (!zone) return Promise.resolve([]);
  if (!zone.dataset.dropReady) {
    zone.dataset.dropReady = '1';
    zone.addEventListener('dragover', (e) => { e.preventDefault(); zone.classList.add('drag'); });
    zone.addEventListener('dragleave', () => zone.classList.remove('drag'));
  }
  return new Promise((resolve) => {
    zone.addEventListener('drop', async (e) => {
      e.preventDefault();
      zone.classList.remove('drag');
      const entries = Array.from(e.dataTransfer.items || [])
        .map((i) => (i.webkitGetAsEntry ? i.webkitGetAsEntry() : null))
        .filter(Boolean);
      const out = [];
      for (const entry of entries) await walk(entry, out);
      if (entries.length === 0) {
        for (const f of Array.from(e.dataTransfer.files || [])) out.push({ path: f.name, file: f });
      }
      resolve(keepFiles(out));
    }, { once: true });
  });
}

// Bytes start..end of file `index` (for hashing in WebAssembly).
export async function importSlice(index, start, end) {
  return new Uint8Array(await importFiles[index].file.slice(start, end).arrayBuffer());
}

// Streams file `index` to `url` (PUT); resolves with [status, body text].
export async function uploadImportFile(index, url) {
  const r = await fetch(url, {
    method: 'PUT',
    credentials: 'same-origin',
    headers: { 'x-musicus-csrf': '1', 'content-type': 'application/pdf' },
    body: importFiles[index].file,
  });
  return [r.status, await r.text()];
}

export function clearImportFiles() {
  importFiles = [];
}
