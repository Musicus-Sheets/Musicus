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

/**
 * Renders page `index` (0-based) as large as fits into boxWidth x boxHeight
 * CSS pixels, sharp on high-density screens. Returns [cssWidth, cssHeight]
 * of the drawn page, or null if a newer render replaced this one.
 */
export async function renderPage(url, index, canvasId, boxWidth, boxHeight) {
  const pdf = await open(url);
  const page = await pdf.getPage(index + 1);
  const base = page.getViewport({ scale: 1 });
  const scale = Math.max(0.1, Math.min(boxWidth / base.width, boxHeight / base.height));
  const viewport = page.getViewport({ scale });
  const dpr = window.devicePixelRatio || 1;

  // Render off-screen first so the visible page never flashes blank.
  const off = document.createElement('canvas');
  off.width = Math.floor(viewport.width * dpr);
  off.height = Math.floor(viewport.height * dpr);
  tasks.get(canvasId)?.cancel();
  const task = page.render({
    canvas: off,
    canvasContext: off.getContext('2d'),
    viewport,
    transform: dpr === 1 ? undefined : [dpr, 0, 0, dpr, 0, 0],
  });
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
  canvas.width = off.width;
  canvas.height = off.height;
  canvas.style.width = `${viewport.width}px`;
  canvas.style.height = `${viewport.height}px`;
  canvas.getContext('2d').drawImage(off, 0, 0);
  return [viewport.width, viewport.height];
}

/**
 * System view: rendered crops, keyed by file, page, crop and size. Holds the
 * current and the next view (pre-rendered), so a page turn shows a finished
 * bitmap at once. Memory only, like everything here.
 */
const crops = new Map();
const MAX_CROPS = 24;
/** canvas id -> key of the crop it should show (newest request wins). */
const wantedCrop = new Map();

function cropKey(url, index, x0, y0, x1, y1, cssWidth) {
  const dpr = window.devicePixelRatio || 1;
  return [url, index, x0, y0, x1, y1, Math.round(cssWidth), dpr].join('|');
}

/** Renders only the crop (0..1 of the page) at `cssWidth` CSS px, sharp at devicePixelRatio. */
function cropBitmap(url, index, x0, y0, x1, y1, cssWidth) {
  const key = cropKey(url, index, x0, y0, x1, y1, cssWidth);
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
    const scale = cssWidth / ((x1 - x0) * base.width);
    const viewport = page.getViewport({ scale, offsetX: -x0 * base.width * scale, offsetY: -y0 * base.height * scale });
    const dpr = window.devicePixelRatio || 1;
    const cssHeight = (y1 - y0) * base.height * scale;
    const off = document.createElement('canvas');
    off.width = Math.max(1, Math.round(cssWidth * dpr));
    off.height = Math.max(1, Math.round(cssHeight * dpr));
    await page.render({
      canvas: off,
      canvasContext: off.getContext('2d'),
      viewport,
      transform: dpr === 1 ? undefined : [dpr, 0, 0, dpr, 0, 0],
    }).promise;
    return { canvas: off, cssWidth, cssHeight };
  })();
  crops.set(key, p);
  p.catch(() => crops.delete(key));
  while (crops.size > MAX_CROPS) {
    crops.delete(crops.keys().next().value);
  }
  return p;
}

/**
 * Shows a crop of page `index` in the canvas `canvasId`, `cssWidth` CSS px
 * wide. Returns [cssWidth, cssHeight], or null if a newer request for the
 * same canvas replaced this one.
 */
export async function renderCrop(url, index, canvasId, x0, y0, x1, y1, cssWidth) {
  const key = cropKey(url, index, x0, y0, x1, y1, cssWidth);
  wantedCrop.set(canvasId, key);
  const bitmap = await cropBitmap(url, index, x0, y0, x1, y1, cssWidth);
  const canvas = document.getElementById(canvasId);
  if (!canvas || wantedCrop.get(canvasId) !== key) {
    return null;
  }
  canvas.width = bitmap.canvas.width;
  canvas.height = bitmap.canvas.height;
  canvas.style.width = `${bitmap.cssWidth}px`;
  canvas.style.height = `${bitmap.cssHeight}px`;
  canvas.getContext('2d').drawImage(bitmap.canvas, 0, 0);
  return [bitmap.cssWidth, bitmap.cssHeight];
}

/** Renders a crop into the memory cache only (the next view). */
export function prerenderCrop(url, index, x0, y0, x1, y1, cssWidth) {
  cropBitmap(url, index, x0, y0, x1, y1, cssWidth).catch(() => {});
}

/** Drops everything not in `keep` (array of urls) from memory. */
export function retainOnly(keep) {
  const wanted = new Set(keep);
  for (const url of [...bytes.keys()]) {
    if (!wanted.has(url)) {
      bytes.delete(url);
      for (const key of [...crops.keys()]) {
        if (key.startsWith(`${url}|`)) crops.delete(key);
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
