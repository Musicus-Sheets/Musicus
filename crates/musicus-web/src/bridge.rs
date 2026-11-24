//! Typed access to `js/bridge.js` (pdf.js, in-memory prefetch, wake lock).

use musicus_api::pixels::DeviceSize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/js/bridge.js")]
extern "C" {
    #[wasm_bindgen(js_name = prefetch)]
    fn js_prefetch(url: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = isCached)]
    pub fn is_cached(url: &str) -> bool;
    #[wasm_bindgen(js_name = pageCount)]
    fn js_page_count(url: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = renderPage)]
    fn js_render_page(
        url: &str,
        index: u32,
        canvas_id: &str,
        box_width: f64,
        box_height: f64,
        ratio: f64,
    ) -> js_sys::Promise;
    #[wasm_bindgen(js_name = renderCrop)]
    fn js_render_crop(
        url: &str,
        index: u32,
        canvas_id: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        width: u32,
        height: u32,
        ratio: f64,
    ) -> js_sys::Promise;
    #[wasm_bindgen(js_name = prerenderCrop)]
    fn js_prerender_crop(url: &str, index: u32, x0: f64, y0: f64, x1: f64, y1: f64, width: u32, height: u32);
    #[wasm_bindgen(js_name = pixelRatio)]
    pub fn pixel_ratio() -> f64;
    #[wasm_bindgen(js_name = pixelRatioChanged)]
    fn js_pixel_ratio_changed() -> js_sys::Promise;
    #[wasm_bindgen(js_name = retainOnly)]
    fn js_retain_only(keep: js_sys::Array);
    #[wasm_bindgen(js_name = keepAwake)]
    fn js_keep_awake(on: bool) -> js_sys::Promise;
    #[wasm_bindgen(js_name = toggleFullscreen)]
    pub fn toggle_fullscreen();
    #[wasm_bindgen(js_name = copyText)]
    fn js_copy_text(text: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = setPending)]
    fn js_set_pending(key: &str, url: &str, revision: f64, bytes: &[u8]);
    #[wasm_bindgen(js_name = clearPending)]
    fn js_clear_pending(key: &str);
    #[wasm_bindgen(js_name = hasPending)]
    pub fn has_pending() -> bool;
    #[wasm_bindgen(js_name = setUnloadWarning)]
    pub fn set_unload_warning(reason: &str, on: bool);
    #[wasm_bindgen(js_name = openEvents)]
    pub fn open_events(url: &str);
    #[wasm_bindgen(js_name = closeEvents)]
    pub fn close_events();
    #[wasm_bindgen(js_name = nextServerEvent)]
    fn js_next_server_event() -> js_sys::Promise;
    #[wasm_bindgen(js_name = unsavedInput)]
    pub fn unsaved_input() -> bool;
    #[wasm_bindgen(js_name = clearUnsavedInput)]
    pub fn clear_unsaved_input();
    #[wasm_bindgen(js_name = pickFolder)]
    fn js_pick_folder(input_id: &str) -> js_sys::Array;
    #[wasm_bindgen(js_name = dropFolder)]
    fn js_drop_folder(zone_id: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = importSlice)]
    fn js_import_slice(index: u32, start: f64, end: f64) -> js_sys::Promise;
    #[wasm_bindgen(js_name = uploadImportFile)]
    fn js_upload_import_file(index: u32, url: &str) -> js_sys::Promise;
    #[wasm_bindgen(js_name = clearImportFiles)]
    pub fn clear_import_files();
}

/// A file of a selected folder: index in the bridge's list, path, size.
#[derive(Clone, Debug, PartialEq)]
pub struct FolderFile {
    pub index: u32,
    pub path: String,
    pub size: f64,
}

fn folder_files(list: js_sys::Array) -> Vec<FolderFile> {
    list.iter()
        .enumerate()
        .filter_map(|(i, entry)| {
            let pair = js_sys::Array::from(&entry);
            Some(FolderFile { index: i as u32, path: pair.get(0).as_string()?, size: pair.get(1).as_f64()? })
        })
        .collect()
}

/// The files of the folder chosen in `<input type=file webkitdirectory id=…>`.
pub fn pick_folder(input_id: &str) -> Vec<FolderFile> {
    folder_files(js_pick_folder(input_id))
}

/// Waits for the next folder dropped onto the element.
pub async fn drop_folder(zone_id: &str) -> Vec<FolderFile> {
    match JsFuture::from(js_drop_folder(zone_id)).await {
        Ok(v) => folder_files(js_sys::Array::from(&v)),
        Err(_) => Vec::new(),
    }
}

/// SHA-256 (hex) of a selected file, read in slices of 4 MiB so even large
/// scans never sit in memory as a whole.
pub async fn hash_import_file(index: u32, size: f64) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    const CHUNK: f64 = 4.0 * 1024.0 * 1024.0;
    let mut hasher = Sha256::new();
    let mut start = 0.0;
    while start < size {
        let end = (start + CHUNK).min(size);
        let bytes = JsFuture::from(js_import_slice(index, start, end)).await.map_err(|e| format!("{e:?}"))?;
        hasher.update(js_sys::Uint8Array::new(&bytes).to_vec());
        start = end;
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Uploads a selected file (streamed by the browser): (HTTP status, body).
pub async fn upload_import_file(index: u32, url: &str) -> Result<(u16, String), String> {
    let r = JsFuture::from(js_upload_import_file(index, url)).await.map_err(|e| format!("{e:?}"))?;
    let pair = js_sys::Array::from(&r);
    Ok((pair.get(0).as_f64().unwrap_or(0.0) as u16, pair.get(1).as_string().unwrap_or_default()))
}

/// Registers the newest unsaved state of an annotation page, to be sent if
/// the tab closes before the regular (debounced) save.
pub fn set_pending(set: i64, page: u32, revision: i64, bytes: &[u8]) {
    let url = format!("/api/annotation-sets/{set}/pages/{page}");
    js_set_pending(&format!("{set}/{page}"), &url, revision as f64, bytes);
}

pub fn clear_pending(set: i64, page: u32) {
    js_clear_pending(&format!("{set}/{page}"));
}

/// The next message of the event stream (`openEvents`): the JSON of a
/// `ServerState`, `"lost"` (no state for a while, the browser reconnects) or
/// `"closed"` (the browser gave up; open it again).
pub async fn next_server_event() -> String {
    JsFuture::from(js_next_server_event()).await.ok().and_then(|v| v.as_string()).unwrap_or_default()
}

fn message(e: JsValue) -> String {
    e.dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| e.as_string())
        .unwrap_or_else(|| "unknown error".into())
}

/// Downloads a file into memory (no-op if already there). The download
/// starts immediately, not when the future is first polled.
pub fn prefetch(url: &str) -> impl std::future::Future<Output = Result<(), String>> + use<> {
    let download = JsFuture::from(js_prefetch(url));
    async move { download.await.map(|_| ()).map_err(message) }
}

pub async fn page_count(url: &str) -> Result<u32, String> {
    let n = JsFuture::from(js_page_count(url)).await.map_err(message)?;
    Ok(n.as_f64().unwrap_or(0.0) as u32)
}

/// Waits until [`pixel_ratio`] changes (browser zoom, another screen, pinch
/// zoom; settled for 250 ms) and returns the new ratio.
pub async fn pixel_ratio_changed() -> f64 {
    JsFuture::from(js_pixel_ratio_changed()).await.ok().and_then(|v| v.as_f64()).unwrap_or_else(pixel_ratio)
}

/// Renders a page into the canvas with id `canvas_id`, fitted into the box,
/// at `ratio` device pixels per CSS pixel (whole device pixels, see
/// `musicus_api::pixels`). Returns the CSS size of the drawn page (= its
/// backing pixels / ratio), `None` if superseded.
pub async fn render_page(
    url: &str,
    index: u32,
    canvas_id: &str,
    box_w: f64,
    box_h: f64,
    ratio: f64,
) -> Result<Option<(f64, f64)>, String> {
    let v = JsFuture::from(js_render_page(url, index, canvas_id, box_w, box_h, ratio)).await.map_err(message)?;
    if v.is_null() || v.is_undefined() {
        return Ok(None);
    }
    let a = js_sys::Array::from(&v);
    Ok(Some((a.get(0).as_f64().unwrap_or(0.0), a.get(1).as_f64().unwrap_or(0.0))))
}

/// A crop of a page (0..1 of the page) for the system view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub page: u32,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    /// Canvas size in device pixels; on screen `size.css_width()` CSS pixels wide.
    pub size: DeviceSize,
}

/// Renders only `crop` of a page into the canvas, exactly `c.size` device
/// pixels. Returns the CSS size, `None` if superseded by a newer request.
pub async fn render_crop(url: &str, canvas_id: &str, c: Crop) -> Result<Option<(f64, f64)>, String> {
    let s = c.size;
    let v = JsFuture::from(js_render_crop(url, c.page, canvas_id, c.x0, c.y0, c.x1, c.y1, s.width, s.height, s.ratio))
        .await
        .map_err(message)?;
    if v.is_null() || v.is_undefined() {
        return Ok(None);
    }
    let a = js_sys::Array::from(&v);
    Ok(Some((a.get(0).as_f64().unwrap_or(0.0), a.get(1).as_f64().unwrap_or(0.0))))
}

/// Renders a crop into memory only, so the next view appears at once.
pub fn prerender_crop(url: &str, c: Crop) {
    js_prerender_crop(url, c.page, c.x0, c.y0, c.x1, c.y1, c.size.width, c.size.height);
}

/// Frees memory of files no longer needed.
pub fn retain_only(urls: &[String]) {
    js_retain_only(urls.iter().map(|u| JsValue::from_str(u)).collect());
}

pub async fn keep_awake(on: bool) {
    if let Some(body) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body()) {
        let _ = body.set_attribute("data-keep-awake", if on { "on" } else { "off" });
    }
    let _ = JsFuture::from(js_keep_awake(on)).await;
}

pub async fn copy_text(text: &str) -> bool {
    JsFuture::from(js_copy_text(text)).await.ok().and_then(|v| v.as_bool()).unwrap_or(false)
}
