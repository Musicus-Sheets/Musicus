//! The reader: one page at a time, fitted to the screen, with the user's
//! private annotation layer drawn on a canvas above the PDF.
//!
//! - Read mode: tap the left/right third, arrow keys, Page Up/Down or a
//!   Bluetooth foot pedal turn pages; past the last page the next piece of
//!   the programme opens.
//! - Draw mode: pen, highlighter, eraser, stamps and text. A pen in use
//!   disables finger drawing (palm rejection).
//! - Page view (one page fitted to the screen) or system view: as many whole
//!   music systems as fit at the user's minimum staff height, cropped to the
//!   content width, continuing across page boundaries (planned by
//!   `musicus_api::layout::plan_views` from the file's system layout). Both
//!   draw annotations in page coordinates, so they appear in either view.
//! - Swipe left / right turns forward / back in both views.
//!
//! Layers are saved per page 1.5 s after the last change and when turning
//! the page or leaving; a conflict with another device is merged by object
//! id (see `musicus_api::ink::merge`).

use crate::{
    Route,
    api::{self, PageBytes, SaveOutcome},
    bridge::{self, Crop},
    ink::{self, PageBox},
    layouts,
    pages::library::part_route,
    ui::{ErrorBox, confirm, prompt},
};
use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use dioxus::web::WebEventExt;
use gloo_net::http::Method;
use musicus_api::{
    AnnotationSet, NewAnnotationSet, Part, Programme, Score, UpdateAnnotationSet,
    ink::{self as model, InkObject, PageLayer, Stamp, StampKind, Stroke, TextNote, Tool},
    layout::{
        self as lay, CSS_PX_PER_MM, FileLayout, Item, LayoutState, ReaderMode, ReaderSettings, Screen, StaffSize, View,
    },
    pixels::device_size,
};
use musicus_i18n::t;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};
use wasm_bindgen::JsCast;

const PAGE_CANVAS: &str = "page-canvas";
const INK_CANVAS: &str = "ink-canvas";
const SAVE_DELAY_MS: u32 = 1500;
const ERASER_RADIUS: f64 = 10.0;
/// Space around the page / the system view inside the stage, CSS px.
const STAGE_MARGIN: f64 = 8.0;
/// A horizontal move of at least this many CSS px (and twice the vertical
/// move) is a swipe.
const SWIPE_MIN_PX: f64 = 60.0;
/// While a file's layout is still pending, ask again after this many ms.
const LAYOUT_RETRY_MS: u32 = 10_000;

type Named<T> = (T, fn() -> String);
const COLORS: [Named<u32>; 4] = [
    (0xd0021bff, t::reader_color_red),
    (0x1f5fd6ff, t::reader_color_blue),
    (0x111111ff, t::reader_color_black),
    (0x1a8f3cff, t::reader_color_green),
];
const HIGHLIGHTS: [Named<u32>; 3] =
    [(0xffd60aff, t::reader_color_yellow), (0x7ee081ff, t::reader_color_green), (0xff8fd8ff, t::reader_color_pink)];
/// Pen widths in page units (1/65535 of the page width).
const PEN_WIDTHS: [Named<u16>; 3] =
    [(110, t::reader_width_fine), (200, t::reader_width_medium), (380, t::reader_width_bold)];
const HIGHLIGHT_WIDTH: u16 = 1400;
const STAMP_SIZE: u16 = 2200;
const TEXT_SIZE: u16 = 1300;

#[component]
pub fn Viewer(id: i64) -> Element {
    rsx! { ViewerPage { key: "{id}", id, pid: None } }
}

#[component]
pub fn ProgrammeViewer(pid: i64, id: i64) -> Element {
    rsx! { ViewerPage { key: "{pid}-{id}", id, pid: Some(pid) } }
}

#[derive(Clone, PartialEq)]
struct Loaded {
    part: Part,
    score: Score,
    sets: Vec<AnnotationSet>,
    programme: Option<Programme>,
}

async fn load(id: i64, pid: Option<i64>) -> api::ApiResult<Loaded> {
    let part: Part = api::get(&format!("/api/parts/{id}")).await?;
    let score: Score = api::get(&format!("/api/scores/{}", part.score_id)).await?;
    let sets: Vec<AnnotationSet> = api::get(&format!("/api/parts/{id}/annotation-sets")).await?;
    let programme = match pid {
        Some(p) => Some(api::get::<Programme>(&format!("/api/programmes/{p}")).await?),
        None => None,
    };
    Ok(Loaded { part, score, sets, programme })
}

#[component]
fn ViewerPage(id: i64, pid: Option<i64>) -> Element {
    let data = use_resource(move || load(id, pid));
    match &*data.read() {
        None => rsx! { div { class: "reader", div { class: "loading-page", {musicus_i18n::t::common_loading()} } } },
        Some(Err(e)) => rsx! {
            main { class: "narrow",
                ErrorBox { message: e.message.clone() }
                Link { to: Route::Library {}, {t::common_back_to_library()} }
            }
        },
        Some(Ok(d)) if d.part.file.is_none() => rsx! {
            main { class: "narrow",
                p { {t::reader_no_file()} }
                Link { to: Route::Library {}, {t::common_back_to_library()} }
            }
        },
        Some(Ok(d)) => rsx! { Reader { data: d.clone() } },
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Read,
    Draw,
}

#[derive(Clone, Copy, PartialEq)]
enum ToolKind {
    Pen,
    Highlighter,
    Eraser,
    Stamp(StampKind),
    Text,
}

#[derive(Clone, PartialEq)]
enum SaveStatus {
    Idle,
    Pending,
    Saving,
    Saved,
    Failed(String),
}

enum Action {
    Add(InkObject),
    /// Removed objects with their former index.
    Remove(Vec<(usize, InkObject)>),
}

/// One page of one annotation set, as known to this tab.
#[derive(Default)]
struct PageState {
    layer: PageLayer,
    /// Server revision `layer` is based on (0: never saved).
    revision: i64,
    /// Object ids of that revision, the base of a three-way merge.
    base_ids: HashSet<u64>,
    loaded: bool,
    /// The saved layer could not be decoded (a newer ink format, R-OPS-041):
    /// shown without it and never changed or saved here, so the stored
    /// annotations stay as they are.
    unreadable: bool,
    /// Bumped by every local change; equal to `saved_generation` when clean.
    generation: u64,
    saved_generation: u64,
    saving: bool,
    undo: Vec<Action>,
    redo: Vec<Action>,
}

/// (annotation set id, page index)
type Key = (i64, u32);

/// Annotation state outside the signal system, so that pending saves can
/// still be flushed when the reader closes.
#[derive(Default)]
struct Store {
    pages: HashMap<Key, PageState>,
}

type SharedStore = Rc<RefCell<Store>>;

fn ids(layer: &PageLayer) -> HashSet<u64> {
    layer.objects.iter().map(InkObject::id).collect()
}

async fn load_page(store: SharedStore, key: Key) -> Result<(), String> {
    if store.borrow().pages.get(&key).is_some_and(|s| s.loaded) {
        return Ok(());
    }
    let (server, revision) = match api::get_page(key.0, key.1 as i64).await.map_err(|e| e.message)? {
        PageBytes::Found { bytes, revision } => match model::decode(&bytes) {
            Ok(layer) => (layer, revision),
            Err(e) => {
                let mut s = store.borrow_mut();
                let st = s.pages.entry(key).or_default();
                // Nothing drawn here may overwrite it (not even on tab close).
                st.unreadable = true;
                st.layer = PageLayer::default();
                st.saved_generation = st.generation;
                st.undo.clear();
                st.redo.clear();
                drop(s);
                bridge::clear_pending(key.0, key.1);
                return Err(crate::errors::ink_error_text(&e));
            }
        },
        PageBytes::Missing => (PageLayer::default(), 0),
    };
    let mut s = store.borrow_mut();
    let st = s.pages.entry(key).or_default();
    if !st.loaded {
        // Anything drawn before the load finished is kept.
        let local = std::mem::take(&mut st.layer);
        st.base_ids = ids(&server);
        st.layer = if local.objects.is_empty() { server } else { model::merge(&HashSet::new(), &local, &server) };
        st.revision = revision;
        st.loaded = true;
    }
    Ok(())
}

/// Saves a page until it is clean, merging on conflicts. Touches no
/// signals, so it may run after the reader was closed.
async fn save_page(store: SharedStore, key: Key) -> Result<(), String> {
    for _ in 0..5 {
        let (layer, revision, generation) = {
            let mut s = store.borrow_mut();
            let Some(st) = s.pages.get_mut(&key) else { return Ok(()) };
            if st.generation == st.saved_generation || st.saving || !st.loaded {
                return Ok(());
            }
            st.saving = true;
            (st.layer.clone(), st.revision, st.generation)
        };
        let outcome = api::put_page(key.0, key.1 as i64, revision, &model::encode(&layer)).await;
        let outcome = match outcome {
            Ok(o) => o,
            Err(e) => {
                if let Some(st) = store.borrow_mut().pages.get_mut(&key) {
                    st.saving = false;
                }
                return Err(e.message);
            }
        };
        match outcome {
            SaveOutcome::Saved(new_revision) => {
                let mut s = store.borrow_mut();
                let st = s.pages.get_mut(&key).expect("page exists");
                st.saving = false;
                st.revision = new_revision;
                st.base_ids = ids(&layer);
                st.saved_generation = generation;
                let clean = st.generation == generation;
                drop(s);
                sync_pending(&store, key);
                if clean {
                    return Ok(());
                }
            }
            SaveOutcome::Conflict => {
                // Another device saved: fetch, merge, then save the merge.
                let server = api::get_page(key.0, key.1 as i64).await.map_err(|e| e.message);
                let mut s = store.borrow_mut();
                let st = s.pages.get_mut(&key).expect("page exists");
                st.saving = false;
                let (server_layer, server_revision) = match server? {
                    PageBytes::Found { bytes, revision } => {
                        (model::decode(&bytes).map_err(|e| crate::errors::ink_error_text(&e))?, revision)
                    }
                    PageBytes::Missing => (PageLayer::default(), 0),
                };
                st.layer = model::merge(&st.base_ids, &st.layer, &server_layer);
                st.base_ids = ids(&server_layer);
                st.revision = server_revision;
                st.generation += 1;
                drop(s);
                sync_pending(&store, key);
            }
        }
    }
    Err("could not save after several attempts".into())
}

/// Keeps the bridge's "send on tab close" copy of a page in sync.
fn sync_pending(store: &SharedStore, key: Key) {
    let s = store.borrow();
    match s.pages.get(&key) {
        Some(st) if st.generation != st.saved_generation => {
            bridge::set_pending(key.0, key.1, st.revision, &model::encode(&st.layer))
        }
        _ => bridge::clear_pending(key.0, key.1),
    }
}

fn dirty_keys(store: &SharedStore) -> Vec<Key> {
    store.borrow().pages.iter().filter(|(_, s)| s.generation != s.saved_generation).map(|(k, _)| *k).collect()
}

/// The set to show first: the one linked to the programme, else the default.
fn choose_set(sets: &[AnnotationSet], programme: Option<i64>) -> Option<i64> {
    programme
        .and_then(|p| sets.iter().find(|s| s.programme_id == Some(p)))
        .or_else(|| sets.iter().find(|s| s.is_default))
        .or_else(|| sets.first())
        .map(|s| s.id)
}

/// Previous and next piece of the programme, preferring the part that
/// shares an instrument with the current one.
fn neighbours(programme: &Programme, part: &Part) -> (Option<Route>, Option<Route>) {
    let Some(pos) = programme.items.iter().position(|i| i.score.parts.iter().any(|p| p.id == part.id)) else {
        return (None, None);
    };
    let pick = |idx: usize| -> Option<Route> {
        let parts: Vec<&Part> = crate::names::sorted(&programme.items.get(idx)?.score.parts)
            .into_iter()
            .filter(|p| p.file.is_some())
            .collect();
        let chosen = parts
            .iter()
            .find(|p| p.instrument_ids.iter().any(|i| part.instrument_ids.contains(i)))
            .or_else(|| parts.first())?;
        Some(part_route(chosen, Some(programme.id)))
    };
    let prev = (0..pos).rev().find_map(pick);
    let next = (pos + 1..programme.items.len()).find_map(pick);
    (prev, next)
}

/// Where a pointer acts: a page shown (wholly or cropped) in an ink canvas.
#[derive(Clone, PartialEq)]
struct Target {
    page: u32,
    bx: PageBox,
    canvas: String,
}

#[derive(Default)]
struct Live {
    drawing: bool,
    erasing: bool,
    pointer: i32,
    /// Page CSS coordinates (see `PageBox::canvas_to_page`) and pressure.
    points: Vec<(f64, f64, u8)>,
    pen_seen: bool,
    target: Option<Target>,
}

/// The crops of one view: ONE contiguous crop of the original page per page
/// the view touches (`View::segments`), with what to render and the box for
/// its ink canvas. All crops of a document share the horizontal range and
/// the scale, so strokes across systems stay straight.
///
/// Sharpness: each crop gets whole device pixels at `ratio`
/// (`musicus_api::pixels::device_size`); its page box is derived from them
/// (page width = crop pixels / ratio / crop width), so the PDF canvas, the
/// ink canvas and the segment box have exactly the same CSS size.
fn view_crops(items: &[Item], v: &View, ratio: f64) -> Vec<(u32, Crop, PageBox)> {
    v.segments
        .iter()
        .filter_map(|g| {
            let it = items.iter().find(|i| i.page == g.page)?;
            let (w, h) = (it.page_w_mm * v.scale, it.page_h_mm * v.scale);
            let size = device_size((g.x1 - g.x0) * w, (g.y1 - g.y0) * h, ratio);
            let w = size.css_width() / (g.x1 - g.x0).max(1e-9);
            let h = w * it.page_h_mm / it.page_w_mm.max(1e-9);
            let crop = Crop { page: g.page, x0: g.x0, y0: g.y0, x1: g.x1, y1: g.y1, size };
            let mut b = PageBox::crop(w, h, g.x0, g.y0, g.x1, g.y1);
            (b.vw, b.vh) = (size.css_width(), size.css_height());
            Some((g.page, crop, b))
        })
        .collect()
}

fn crop_canvas(k: usize) -> String {
    format!("crop-{k}")
}

fn crop_ink_canvas(k: usize) -> String {
    format!("crop-ink-{k}")
}

#[component]
fn Reader(data: Loaded) -> Element {
    let nav = navigator();
    let part = data.part.clone();
    let current_version = part.file.as_ref().map(|f| f.version).unwrap_or(0);
    let programme_id = data.programme.as_ref().map(|p| p.id);
    let (prev_piece, next_piece) = data.programme.as_ref().map(|p| neighbours(p, &part)).unwrap_or((None, None));
    let prev_piece = use_signal(|| prev_piece);
    let next_piece = use_signal(|| next_piece);

    let mut sets = use_signal(|| data.sets.clone());
    let mut active = use_signal(|| choose_set(&data.sets, programme_id));
    let mut page = use_signal(|| 0u32);
    let mut page_total = use_signal(|| 0u32);
    let mut stage = use_signal(|| (0.0f64, 0.0f64));
    // Device pixels per CSS pixel to render at (devicePixelRatio × pinch zoom).
    let mut ratio = use_signal(bridge::pixel_ratio);
    let mut page_box = use_signal(|| None::<PageBox>);
    let mut mode = use_signal(|| Mode::Read);
    let mut tool = use_signal(|| ToolKind::Pen);
    let mut color = use_signal(|| COLORS[0].0);
    let mut highlight = use_signal(|| HIGHLIGHTS[0].0);
    let mut pen_width = use_signal(|| PEN_WIDTHS[1].0);
    let mut show_ink = use_signal(|| true);
    let mut status = use_signal(|| SaveStatus::Idle);
    let mut error = use_signal(|| None::<String>);
    let mut menu = use_signal(|| false);
    // Bumped whenever the store changes, to redraw.
    let mut version = use_signal(|| 0u64);
    let store: SharedStore = use_hook(|| Rc::new(RefCell::new(Store::default())));
    let live = use_hook(|| Rc::new(RefCell::new(Live::default())));
    // Page last drawn on (undo/redo target in the system view).
    let mut edit_page = use_signal(|| 0u32);

    // --- System view -----------------------------------------------------
    let mut session = crate::session();
    let mut settings = use_signal(|| session.user.peek().as_ref().map(|u| u.reader).unwrap_or_default());
    let mut layout = use_signal(|| None::<FileLayout>);
    // Reading position in the system view: the first item of the current view.
    let mut anchor = use_signal(|| None::<usize>);
    let swipe = use_hook(|| Rc::new(Cell::new(None::<(f64, f64)>)));
    let last_swipe = use_hook(|| Rc::new(Cell::new(0.0f64)));

    let current_sha = part.file.as_ref().map(|f| f.sha256.clone());
    let url = use_memo(move || {
        let sets = sets.read();
        active()
            .and_then(|a| sets.iter().find(|s| s.id == a))
            .map(|s| s.file_sha256.clone())
            .or_else(|| current_sha.clone())
            .map(|sha| api::file_url(&sha))
    });

    // The system layout of the displayed file (asked again while pending).
    use_effect(move || {
        let Some(u) = url() else { return };
        let sha = u.trim_start_matches("/api/files/").to_string();
        layout.set(None);
        spawn(async move {
            for _ in 0..30 {
                match layouts::load(&sha).await {
                    Ok(l) => {
                        let pending = l.state == LayoutState::Pending;
                        layout.set(Some(l));
                        if !pending {
                            return;
                        }
                    }
                    Err(_) => return,
                }
                gloo_timers::future::TimeoutFuture::new(LAYOUT_RETRY_MS).await;
            }
        });
    });
    // Reading sequence and views; empty if the system view is off or unavailable.
    let items = use_memo(move || match &*layout.read() {
        Some(l) if l.state == LayoutState::Done && page_total() > 0 && l.pages.len() == page_total() as usize => {
            lay::reading_items(&l.pages)
        }
        _ => Vec::new(),
    });
    let views = use_memo(move || {
        let s = settings();
        let (w, h) = stage();
        if s.mode != ReaderMode::Systems || items.read().is_empty() {
            return Vec::new();
        }
        let screen = Screen { width: w - STAGE_MARGIN, height: h - STAGE_MARGIN, px_per_mm: CSS_PX_PER_MM };
        lay::plan_views(&items.read(), screen, s.staff_size.min_staff_mm())
    });
    let systems_on = move || !views.read().is_empty();
    let view_idx = use_memo(move || {
        let v = views.read();
        match anchor() {
            Some(a) if !v.is_empty() => lay::view_for_item(&v, a),
            _ => 0,
        }
    });
    let crops = use_memo(move || {
        let v = views.read();
        match v.get(view_idx()) {
            Some(view) => view_crops(&items.read(), view, ratio()),
            None => Vec::new(),
        }
    });
    // Entering the system view starts at the current page; the current page
    // follows the view (page number, annotation loading, back to page view).
    use_effect(move || {
        if !systems_on() {
            return;
        }
        if anchor().is_none() {
            anchor.set(Some(lay::first_item_of_page(&items.read(), *page.peek())));
            return;
        }
        if let Some((p, _, _)) = crops.read().first()
            && *page.peek() != *p
        {
            page.set(*p);
        }
    });

    // Keep the screen on and prefetch the next piece.
    let next_url = data.programme.as_ref().and_then(|p| {
        let Some(Route::ProgrammeViewer { id, .. }) = next_piece.peek().clone() else { return None };
        p.items
            .iter()
            .flat_map(|i| i.score.parts.iter())
            .find(|q| q.id == id)?
            .file
            .as_ref()
            .map(|f| api::file_url(&f.sha256))
    });
    use_hook(move || {
        spawn(bridge::keep_awake(true));
        // Browser zoom, another screen, pinch zoom: render again at the new resolution.
        spawn(async move {
            loop {
                let r = bridge::pixel_ratio_changed().await;
                ratio.set(r);
            }
        });
        if let Some(u) = next_url {
            spawn(async move {
                let _ = bridge::prefetch(&u).await;
            });
        }
    });

    // Flush unsaved pages when the reader closes.
    use_drop({
        let store = store.clone();
        move || {
            let keys = dirty_keys(&store);
            let store = store.clone();
            spawn_forever(async move {
                for key in keys {
                    let _ = save_page(store.clone(), key).await;
                }
                bridge::keep_awake(false).await;
            });
        }
    });

    // Page count of the displayed file.
    use_effect(move || {
        let Some(u) = url() else { return };
        spawn(async move {
            match bridge::page_count(&u).await {
                Ok(n) => {
                    page_total.set(n);
                    if *page.peek() >= n {
                        page.set(n.saturating_sub(1));
                    }
                }
                Err(e) => error.set(Some(t::reader_open_failed(e.to_string()))),
            }
        });
    });

    // Render the PDF page whenever file, page or available space change.
    use_effect(move || {
        let (Some(u), (w, h), p, r) = (url(), stage(), page(), ratio()) else { return };
        if w < 20.0 || h < 20.0 || systems_on() {
            return;
        }
        spawn(async move {
            match bridge::render_page(&u, p, PAGE_CANVAS, w - STAGE_MARGIN, h - STAGE_MARGIN, r).await {
                Ok(Some((width, height))) => page_box.set(Some(PageBox::page(width, height))),
                Ok(None) => {}
                Err(e) => error.set(Some(t::reader_systems_render_failed(e.to_string()))),
            }
        });
    });

    // Render the crops of the current view; then the next view into memory.
    use_effect(move || {
        let Some(u) = url() else { return };
        let current = crops();
        if current.is_empty() {
            return;
        }
        let next = views.read().get(view_idx() + 1).map(|v| view_crops(&items.read(), v, ratio())).unwrap_or_default();
        spawn(async move {
            for (k, (_, crop, _)) in current.iter().enumerate() {
                if let Err(e) = bridge::render_crop(&u, &crop_canvas(k), *crop).await {
                    error.set(Some(t::reader_systems_render_failed(e.to_string())));
                    return;
                }
            }
            for (_, crop, _) in next {
                bridge::prerender_crop(&u, crop);
            }
        });
    });

    // Load the annotation layers of the shown pages.
    use_effect({
        let store = store.clone();
        move || {
            let Some(set) = active() else { return };
            let mut pages: Vec<u32> = crops().iter().map(|(p, _, _)| *p).collect();
            if pages.is_empty() {
                pages.push(page());
            }
            pages.dedup();
            for p in pages {
                let store = store.clone();
                spawn(async move {
                    match load_page(store, (set, p)).await {
                        Ok(()) => version += 1,
                        Err(e) => error.set(Some(t::reader_annotations_load_failed(e.to_string()))),
                    }
                });
            }
        }
    });

    // Redraw the annotation layers (one canvas per page, or per crop).
    use_effect({
        let store = store.clone();
        move || {
            let _ = version();
            let empty = PageLayer::default();
            let s = store.borrow();
            let layer = |p: u32| match (show_ink(), active()) {
                (true, Some(set)) => s.pages.get(&(set, p)).map(|st| &st.layer).unwrap_or(&empty),
                _ => &empty,
            };
            let shown = crops();
            if shown.is_empty() {
                if let Some(b) = page_box() {
                    ink::draw_layer(INK_CANVAS, b, layer(page()), ratio());
                }
            } else {
                for (k, (p, c, b)) in shown.iter().enumerate() {
                    ink::draw_layer(&crop_ink_canvas(k), *b, layer(*p), c.size.ratio);
                }
            }
        }
    });

    // Saves one page now or after the debounce delay.
    let save = {
        let store = store.clone();
        move |key: Key, delay: bool| {
            let store = store.clone();
            let generation = store.borrow().pages.get(&key).map(|s| s.generation);
            status.set(SaveStatus::Pending);
            spawn(async move {
                if delay {
                    gloo_timers::future::TimeoutFuture::new(SAVE_DELAY_MS).await;
                    // A newer change scheduled its own save.
                    if store.borrow().pages.get(&key).map(|s| s.generation) != generation {
                        return;
                    }
                }
                status.set(SaveStatus::Saving);
                match save_page(store.clone(), key).await {
                    Ok(()) => status.set(SaveStatus::Saved),
                    Err(e) => status.set(SaveStatus::Failed(e)),
                }
                version += 1;
            });
        }
    };

    // Applies a change to the current page and schedules its save.
    let change = {
        let store = store.clone();
        let mut save = save.clone();
        move |key: Key, f: &dyn Fn(&mut PageState) -> Option<Action>| {
            let changed = {
                let mut s = store.borrow_mut();
                let st = s.pages.entry(key).or_default();
                if st.unreadable {
                    drop(s);
                    error.set(Some(t::reader_annotations_page_locked()));
                    version += 1; // redraw without the live stroke
                    return;
                }
                match f(st) {
                    Some(action) => {
                        st.undo.push(action);
                        st.redo.clear();
                        st.generation += 1;
                        true
                    }
                    None => false,
                }
            };
            if changed {
                sync_pending(&store, key);
                version += 1;
                save(key, true);
            }
        }
    };

    // Adds an object; creates a first annotation set if there is none.
    let add_object = {
        let mut change = change.clone();
        let store = store.clone();
        let programme_title = data.programme.as_ref().map(|p| p.title.clone());
        let part_id = part.id;
        move |p: u32, object: InkObject| {
            edit_page.set(p);
            if let Some(set) = active() {
                change((set, p), &|st| {
                    st.layer.objects.push(object.clone());
                    Some(Action::Add(object.clone()))
                });
                return;
            }
            // Unnamed (None) unless read from a programme: shown as "My annotations".
            let name = programme_title.clone();
            let mut change = change.clone();
            let store = store.clone();
            spawn(async move {
                let req = NewAnnotationSet { name, programme_id, copy_from: None };
                match api::post::<_, AnnotationSet>(&format!("/api/parts/{part_id}/annotation-sets"), &req).await {
                    Ok(set) => {
                        let id = set.id;
                        sets.write().push(set);
                        store.borrow_mut().pages.entry((id, p)).or_default().loaded = true;
                        active.set(Some(id));
                        change((id, p), &|st| {
                            st.layer.objects.push(object.clone());
                            Some(Action::Add(object.clone()))
                        });
                    }
                    Err(e) => error.set(Some(t::reader_set_create_failed(&e.message))),
                }
            });
        }
    };

    let erase_at = {
        let mut change = change.clone();
        let store = store.clone();
        move |t: &Target, x: f64, y: f64| {
            let Some(set) = active() else { return };
            let key = (set, t.page);
            edit_page.set(t.page);
            let hits = store.borrow().pages.get(&key).map(|st| ink::hit(t.bx, &st.layer, (x, y), ERASER_RADIUS));
            let Some(hits) = hits.filter(|h| !h.is_empty()) else { return };
            change(key, &|st| {
                let mut removed = Vec::new();
                let mut i = 0;
                st.layer.objects.retain(|o| {
                    let keep = !hits.contains(&o.id());
                    if !keep {
                        removed.push((i, o.clone()));
                    }
                    i += 1;
                    keep
                });
                Some(Action::Remove(removed))
            });
        }
    };

    let undo_redo = {
        let store = store.clone();
        let mut save = save.clone();
        move |redo: bool| {
            let Some(set) = active() else { return };
            let key = (set, if systems_on() { edit_page() } else { page() });
            {
                let mut s = store.borrow_mut();
                let Some(st) = s.pages.get_mut(&key) else { return };
                let Some(action) = (if redo { st.redo.pop() } else { st.undo.pop() }) else { return };
                let inverse = match action {
                    Action::Add(o) => {
                        let id = o.id();
                        let index = st.layer.objects.iter().position(|x| x.id() == id);
                        match index {
                            Some(i) => Action::Remove(vec![(i, st.layer.objects.remove(i))]),
                            None => return,
                        }
                    }
                    Action::Remove(list) => {
                        for (i, o) in &list {
                            let at = (*i).min(st.layer.objects.len());
                            st.layer.objects.insert(at, o.clone());
                        }
                        // Re-adding several objects is undone by removing them all.
                        match list.len() {
                            1 => Action::Add(list.into_iter().next().expect("one").1),
                            _ => Action::Remove(
                                list.into_iter()
                                    .map(|(_, o)| {
                                        let i = st.layer.objects.iter().position(|x| x.id() == o.id()).unwrap_or(0);
                                        (i, o)
                                    })
                                    .collect(),
                            ),
                        }
                    }
                };
                if redo {
                    st.undo.push(inverse)
                } else {
                    st.redo.push(inverse)
                }
                st.generation += 1;
            }
            sync_pending(&store, key);
            version += 1;
            save(key, true);
        }
    };

    // Page turning, also across pieces of a programme.
    let turn = {
        let store = store.clone();
        let mut save = save.clone();
        move |delta: i32| {
            for key in dirty_keys(&store) {
                save(key, false);
            }
            if systems_on() {
                let v = views();
                let target = view_idx() as i32 + delta;
                if target < 0 {
                    if let Some(r) = prev_piece() {
                        nav.push(r);
                    }
                } else if target as usize >= v.len() {
                    if let Some(r) = next_piece() {
                        nav.push(r);
                    }
                } else {
                    anchor.set(Some(v[target as usize].first));
                }
                return;
            }
            let target = page() as i32 + delta;
            if target < 0 {
                if let Some(r) = prev_piece() {
                    nav.push(r);
                }
            } else if target >= page_total() as i32 {
                if let Some(r) = next_piece() {
                    nav.push(r);
                }
            } else {
                page.set(target as u32);
            }
        }
    };

    // Keyboard and foot pedals (which send arrow / page keys).
    use_future({
        let turn = turn.clone();
        move || {
            let mut turn = turn.clone();
            async move {
                let mut keys = document::eval(
                    r#"
                    const wanted = ['ArrowRight', 'ArrowLeft', 'ArrowDown', 'ArrowUp', 'PageDown', 'PageUp', ' '];
                    const handler = (e) => {
                        const tag = e.target && e.target.tagName;
                        if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || !wanted.includes(e.key)) return;
                        e.preventDefault();
                        dioxus.send(e.key);
                    };
                    window.__musicusKeys && window.removeEventListener('keydown', window.__musicusKeys);
                    window.__musicusKeys = handler;
                    window.addEventListener('keydown', handler);
                    await new Promise(() => {});
                    "#,
                );
                while let Ok(key) = keys.recv::<String>().await {
                    match key.as_str() {
                        "ArrowLeft" | "ArrowUp" | "PageUp" => turn(-1),
                        _ => turn(1),
                    }
                }
            }
        }
    });
    use_drop(|| {
        document::eval(
            "window.__musicusKeys && window.removeEventListener('keydown', window.__musicusKeys); window.__musicusKeys = null;",
        );
    });

    // --- Pointer input on the ink canvas --------------------------------

    let pointer_down = {
        let live = live.clone();
        let mut add_object = add_object.clone();
        let mut erase_at = erase_at.clone();
        move |t: Target, e: PointerEvent| {
            let kind = e.pointer_type();
            let mut l = live.borrow_mut();
            if kind == "pen" {
                l.pen_seen = true;
            } else if kind == "touch" && l.pen_seen {
                return; // palm rejection
            }
            let web = e.as_web_event();
            if let Some(target) = web.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
                let _ = target.set_pointer_capture(e.pointer_id());
            }
            web.prevent_default();
            let p = e.element_coordinates();
            let (x, y) = t.bx.canvas_to_page(p.x, p.y);
            let b = t.bx;
            let page_index = t.page;
            l.target = Some(t.clone());
            match tool() {
                ToolKind::Pen | ToolKind::Highlighter => {
                    let pressure =
                        if kind == "mouse" { 0 } else { (e.pressure() * 255.0).round().clamp(1.0, 255.0) as u8 };
                    l.drawing = true;
                    l.pointer = e.pointer_id();
                    l.points = vec![(x, y, pressure)];
                }
                ToolKind::Eraser => {
                    l.erasing = true;
                    l.pointer = e.pointer_id();
                    drop(l);
                    erase_at(&t, x, y);
                }
                ToolKind::Stamp(kind) => {
                    drop(l);
                    let at = b.to_page(x, y);
                    add_object(
                        page_index,
                        InkObject::Stamp(Stamp {
                            id: ink::new_id(),
                            kind,
                            at: [at.0, at.1],
                            size: STAMP_SIZE,
                            color: color(),
                        }),
                    );
                }
                ToolKind::Text => {
                    drop(l);
                    if let Some(text) = prompt(&t::reader_text_prompt(), "") {
                        let at = b.to_page(x, y);
                        add_object(
                            page_index,
                            InkObject::Text(TextNote {
                                id: ink::new_id(),
                                at: [at.0, at.1],
                                size: TEXT_SIZE,
                                color: color(),
                                text: text.chars().take(model::MAX_TEXT_CHARS).collect(),
                            }),
                        );
                    }
                }
            }
        }
    };

    let pointer_move = {
        let live = live.clone();
        let mut erase_at = erase_at.clone();
        move |e: PointerEvent| {
            let mut l = live.borrow_mut();
            if e.pointer_id() != l.pointer || !(l.drawing || l.erasing) {
                return;
            }
            let Some(t) = l.target.clone() else { return };
            let p = e.element_coordinates();
            let (x, y) = t.bx.canvas_to_page(p.x, p.y);
            if l.erasing {
                drop(l);
                erase_at(&t, x, y);
                return;
            }
            let b = t.bx;
            let kind = e.pointer_type();
            let pressure = if kind == "mouse" { 0 } else { (e.pressure() * 255.0).round().clamp(1.0, 255.0) as u8 };
            let last = *l.points.last().expect("stroke has a first point");
            if (last.0 - x).abs() < 0.5 && (last.1 - y).abs() < 0.5 {
                return;
            }
            l.points.push((x, y, pressure));
            if let Some(ctx) = ink::context(&t.canvas) {
                ctx.set_line_cap("round");
                if tool() == ToolKind::Highlighter {
                    ctx.set_global_alpha(0.35);
                    ctx.set_stroke_style_str(&ink::css_color(highlight(), 1.0));
                    ink::segment(&ctx, (last.0, last.1), (x, y), b.len_css(HIGHLIGHT_WIDTH));
                    ctx.set_global_alpha(1.0);
                } else {
                    ctx.set_stroke_style_str(&ink::css_color(color(), 1.0));
                    ink::segment(&ctx, (last.0, last.1), (x, y), ink::pen_width_css(b, pen_width(), pressure));
                }
            }
        }
    };

    let pointer_up = {
        let live = live.clone();
        let mut add_object = add_object.clone();
        move |e: PointerEvent| {
            let mut l = live.borrow_mut();
            if e.pointer_id() != l.pointer {
                return;
            }
            l.erasing = false;
            if !l.drawing {
                return;
            }
            l.drawing = false;
            let points = std::mem::take(&mut l.points);
            let Some(t) = l.target.clone() else { return };
            drop(l);
            let b = t.bx;
            let mut page_points: Vec<(u16, u16, u8)> = Vec::with_capacity(points.len());
            for (x, y, pr) in points.into_iter().take(model::MAX_POINTS_PER_STROKE) {
                let (px, py) = b.to_page(x, y);
                if page_points.last().is_none_or(|q| (q.0, q.1) != (px, py)) {
                    page_points.push((px, py, pr));
                }
            }
            let (stroke_tool, c, w) = match tool() {
                ToolKind::Highlighter => (Tool::Highlighter, highlight(), HIGHLIGHT_WIDTH),
                _ => (Tool::Pen, color(), pen_width()),
            };
            add_object(t.page, InkObject::Stroke(Stroke::from_points(ink::new_id(), stroke_tool, c, w, &page_points)));
        }
    };

    // --- Annotation set management ---------------------------------------

    let mut replace_set = move |updated: AnnotationSet| {
        let mut list = sets.write();
        if updated.is_default {
            for s in list.iter_mut() {
                s.is_default = false;
            }
        }
        match list.iter_mut().find(|s| s.id == updated.id) {
            Some(s) => *s = updated,
            None => list.push(updated),
        }
    };
    let part_id = part.id;
    let active_set = active().and_then(|a| sets.read().iter().find(|s| s.id == a).cloned());

    let new_set = move |copy: bool| async move {
        let source = if copy { active() } else { None };
        let suggestion = if copy { t::reader_sets_suggest_copy() } else { t::reader_sets_suggest_new() };
        let Some(name) = prompt(&t::reader_sets_name_prompt(), &suggestion) else { return };
        let req = NewAnnotationSet { name: Some(name), programme_id, copy_from: source };
        match api::post::<_, AnnotationSet>(&format!("/api/parts/{part_id}/annotation-sets"), &req).await {
            Ok(set) => {
                let id = set.id;
                replace_set(set);
                active.set(Some(id));
                menu.set(false);
            }
            Err(e) => error.set(Some(e.message)),
        }
    };
    let update_set = move |body: UpdateAnnotationSet| async move {
        let Some(id) = active() else { return };
        match api::patch::<_, AnnotationSet>(&format!("/api/annotation-sets/{id}"), &body).await {
            Ok(set) => replace_set(set),
            Err(e) => error.set(Some(e.message)),
        }
    };
    let delete_set = move |_| async move {
        let Some(id) = active() else { return };
        if !confirm(&t::reader_sets_delete_confirm()) {
            return;
        }
        match api::call(Method::DELETE, &format!("/api/annotation-sets/{id}")).await {
            Ok(()) => {
                sets.write().retain(|s| s.id != id);
                active.set(choose_set(&sets.read(), programme_id));
                menu.set(false);
            }
            Err(e) => error.set(Some(e.message)),
        }
    };
    let migrate_set = move |_| async move {
        let Some(id) = active() else { return };
        match api::post::<_, AnnotationSet>(&format!("/api/annotation-sets/{id}/migrate"), &serde_json::json!({})).await
        {
            Ok(set) => replace_set(set),
            Err(e) => error.set(Some(e.message)),
        }
    };

    // --- View ------------------------------------------------------------

    let title = format!("{} — {}", data.score.title, crate::names::part_name(&part));
    let back = match programme_id {
        Some(id) => Route::ProgrammeView { id },
        None => Route::Library {},
    };
    let at_end = if systems_on() {
        view_idx() + 1 >= views.read().len()
    } else {
        page_total() > 0 && page() + 1 >= page_total()
    };
    let reader = settings();
    let systems_wanted = reader.mode == ReaderMode::Systems;
    let layout_note = match (&*layout.read(), systems_wanted && !systems_on()) {
        (_, false) => None,
        (Some(l), true) if l.state == LayoutState::Failed => Some(t::reader_layout_failed()),
        (Some(l), true) if l.state == LayoutState::Done => None,
        _ => Some(t::reader_layout_pending()),
    };
    // Changes the reader settings here and on the server (R-SEC-001: not on the device).
    let mut change_settings = move |new: ReaderSettings| {
        if new.mode != settings.peek().mode {
            anchor.set(None);
        }
        settings.set(new);
        if let Some(u) = session.user.write().as_mut() {
            u.reader = new;
        }
        spawn(async move {
            if let Err(e) = api::put::<_, ReaderSettings>("/api/auth/reader-settings", &new).await {
                error.set(Some(t::reader_settings_failed(&e.message)));
            }
        });
    };
    let status_text = match status() {
        SaveStatus::Idle => String::new(),
        SaveStatus::Pending => t::reader_save_pending(),
        SaveStatus::Saving => t::common_saving(),
        SaveStatus::Saved => t::common_saved(),
        SaveStatus::Failed(e) => t::reader_save_failed(e),
    };
    let drawing = mode() == Mode::Draw;
    let ink_class = if drawing { "ink active" } else { "ink" };

    rsx! {
        div { class: "reader",
            header { class: "reader-bar",
                Link { class: "icon", to: back, title: t::reader_back(), "‹" }
                span { class: "reader-title", title: "{title}", "{title}" }
                span { class: "page-no", if page_total() > 0 { "{page() + 1} / {page_total()}" } }
                button {
                    class: if systems_wanted { "toggle on" } else { "toggle" },
                    title: t::reader_system_view_hint(),
                    "aria-pressed": "{systems_wanted}",
                    onclick: move |_| change_settings(ReaderSettings {
                        mode: if systems_wanted { ReaderMode::Page } else { ReaderMode::Systems },
                        ..reader
                    }),
                    {t::reader_system_view()}
                }
                if systems_wanted {
                    select {
                        class: "staff-size",
                        title: t::reader_staff_size(),
                        "aria-label": t::reader_staff_size(),
                        onchange: move |e| {
                            if let Some(z) = StaffSize::parse(&e.value()) {
                                change_settings(ReaderSettings { staff_size: z, ..reader });
                            }
                        },
                        for z in StaffSize::ALL {
                            option { value: z.as_str(), selected: z == reader.staff_size, "{z.as_str()}" }
                        }
                    }
                }
                button {
                    class: if drawing { "toggle on" } else { "toggle" },
                    title: t::reader_draw_hint(),
                    onclick: move |_| mode.set(if drawing { Mode::Read } else { Mode::Draw }),
                    if drawing { {t::reader_draw_done()} } else { {t::reader_draw()} }
                }
                button { class: "icon", title: t::reader_sets_title(), onclick: move |_| menu.toggle(), "☰" }
                button {
                    class: "icon",
                    title: if show_ink() { t::reader_hide_annotations() } else { t::reader_show_annotations() },
                    onclick: move |_| show_ink.toggle(),
                    if show_ink() { "◉" } else { "○" }
                }
                button { class: "icon", title: t::reader_full_screen(), onclick: |_| bridge::toggle_fullscreen(), "⛶" }
            }
            if drawing {
                div { class: "tool-bar",
                    ToolButton { label: t::reader_tool_pen(), icon: "✒", active: tool() == ToolKind::Pen, onclick: move |_| tool.set(ToolKind::Pen) }
                    ToolButton { label: t::reader_tool_highlighter(), icon: "▮", active: tool() == ToolKind::Highlighter, onclick: move |_| tool.set(ToolKind::Highlighter) }
                    ToolButton { label: t::reader_tool_eraser(), icon: "⌫", active: tool() == ToolKind::Eraser, onclick: move |_| tool.set(ToolKind::Eraser) }
                    ToolButton { label: t::reader_tool_text(), icon: "T", active: tool() == ToolKind::Text, onclick: move |_| tool.set(ToolKind::Text) }
                    select {
                        class: if matches!(tool(), ToolKind::Stamp(_)) { "stamps on" } else { "stamps" },
                        title: t::reader_tool_stamp(),
                        onchange: move |e| {
                            if let Ok(i) = e.value().parse::<usize>() {
                                tool.set(ToolKind::Stamp(StampKind::ALL[i]));
                            }
                        },
                        option { value: "", {t::reader_tool_stamp_choose()} }
                        for (i, k) in StampKind::ALL.iter().enumerate() {
                            option { value: "{i}", selected: tool() == ToolKind::Stamp(*k), "{k.glyph()}" }
                        }
                    }
                    span { class: "sep" }
                    if tool() == ToolKind::Highlighter {
                        for (c, name) in HIGHLIGHTS {
                            button { class: if highlight() == c { "swatch on" } else { "swatch" }, title: name(),
                                style: "background: {ink::css_color(c, 0.6)}", onclick: move |_| highlight.set(c) }
                        }
                    } else {
                        for (c, name) in COLORS {
                            button { class: if color() == c { "swatch on" } else { "swatch" }, title: name(),
                                style: "background: {ink::css_color(c, 1.0)}", onclick: move |_| color.set(c) }
                        }
                        span { class: "sep" }
                        for (w, name) in PEN_WIDTHS {
                            button { class: if pen_width() == w { "width on" } else { "width" }, title: name(),
                                onclick: move |_| pen_width.set(w),
                                span { class: "dot", style: "width: {w / 30}px; height: {w / 30}px" }
                            }
                        }
                    }
                    span { class: "sep" }
                    button { class: "icon", title: t::reader_undo(), onclick: { let mut u = undo_redo.clone(); move |_| u(false) }, "↶" }
                    button { class: "icon", title: t::reader_redo(), onclick: { let mut u = undo_redo.clone(); move |_| u(true) }, "↷" }
                    span { class: "save-status", "{status_text}" }
                }
            }
            if menu() {
                div { class: "set-menu card",
                    h3 { {t::reader_sets_title()} }
                    if sets.read().is_empty() {
                        p { class: "muted small", {t::reader_sets_none()} }
                    }
                    for s in sets.read().iter().cloned() {
                        label { key: "{s.id}", class: "set-option",
                            input { r#type: "radio", name: "set", checked: active() == Some(s.id),
                                onchange: move |_| active.set(Some(s.id)) }
                            span { {crate::names::set_name(&s)} }
                            if s.is_default { span { class: "tag", {t::reader_sets_default_tag()} } }
                            if s.file_version != current_version { span { class: "tag warn", {t::reader_sets_version_tag(s.file_version)} } }
                        }
                    }
                    label { class: "set-option",
                        input { r#type: "radio", name: "set", checked: active().is_none(), onchange: move |_| active.set(None) }
                        span { class: "muted", {t::reader_sets_no_annotations()} }
                    }
                    div { class: "menu-actions",
                        button { onclick: move |_| new_set(false), {t::reader_sets_new()} }
                        if let Some(s) = active_set.clone() {
                            button { onclick: move |_| new_set(true), {t::reader_sets_duplicate()} }
                            button {
                                onclick: move |_| {
                                    let current = crate::names::set_name(&s);
                                    async move {
                                        if let Some(name) = prompt(&t::reader_sets_rename_prompt(), &current) {
                                            update_set(UpdateAnnotationSet { name: Some(name), ..Default::default() }).await;
                                        }
                                    }
                                },
                                {t::reader_sets_rename()}
                            }
                            if !s.is_default {
                                button { onclick: move |_| update_set(UpdateAnnotationSet { is_default: Some(true), ..Default::default() }), {t::reader_sets_make_default()} }
                            }
                            if let Some(pid) = programme_id {
                                if s.programme_id != Some(pid) {
                                    button { onclick: move |_| update_set(UpdateAnnotationSet { programme_id: Some(Some(pid)), ..Default::default() }), {t::reader_sets_use_for_programme()} }
                                }
                            }
                            button { class: "danger", onclick: delete_set, {t::common_delete()} }
                        }
                    }
                    button { class: "link close", onclick: move |_| menu.set(false), {t::common_close()} }
                }
            }
            if let Some(s) = active_set.clone().filter(|s| s.file_version != current_version) {
                div { class: "banner",
                    {t::reader_sets_outdated(s.file_version, current_version)}
                    button { onclick: migrate_set, {t::reader_sets_migrate()} }
                }
            }
            if let Some(note) = layout_note {
                div { class: "banner info", "{note}" }
            }
            if let Some(e) = error() {
                div { class: "banner error", "{e} " button { class: "link", onclick: move |_| error.set(None), {t::reader_dismiss()} } }
            }
            div {
                class: "stage",
                onresize: move |e| {
                    if let Ok(size) = e.get_content_box_size() {
                        stage.set((size.width, size.height));
                    }
                },
                onpointerdown: {
                    let swipe = swipe.clone();
                    move |e: PointerEvent| {
                        if mode() == Mode::Read {
                            let p = e.client_coordinates();
                            swipe.set(Some((p.x, p.y)));
                        }
                    }
                },
                onpointerup: {
                    let swipe = swipe.clone();
                    let last_swipe = last_swipe.clone();
                    let mut turn = turn.clone();
                    move |e: PointerEvent| {
                        let Some((x0, y0)) = swipe.take() else { return };
                        let p = e.client_coordinates();
                        let (dx, dy) = (p.x - x0, p.y - y0);
                        if mode() == Mode::Read && dx.abs() >= SWIPE_MIN_PX && dx.abs() > 2.0 * dy.abs() {
                            last_swipe.set(js_sys::Date::now());
                            turn(if dx < 0.0 { 1 } else { -1 });
                        }
                    }
                },
                onclick: {
                    let mut turn = turn.clone();
                    let last_swipe = last_swipe.clone();
                    move |e: MouseEvent| {
                        // The click that ends a swipe is not a tap.
                        if js_sys::Date::now() - last_swipe.get() < 400.0 {
                            return;
                        }
                        if mode() == Mode::Read {
                            let x = e.client_coordinates().x;
                            let w = stage().0.max(1.0);
                            if x < w / 3.0 {
                                turn(-1);
                            } else if x > w * 2.0 / 3.0 {
                                turn(1);
                            }
                        }
                    }
                },
                if systems_on() {
                    div {
                        class: "system-view",
                        "data-view": "{view_idx() + 1}/{views.read().len()}",
                        "data-items": "{views.read().get(view_idx()).map_or(0, |v| v.last + 1 - v.first)}",
                        for (k, (p, _, b)) in crops().into_iter().enumerate() {
                            div { key: "{k}", class: "page-segment", style: "width: {b.vw}px; height: {b.vh}px",
                                canvas { id: crop_canvas(k) }
                                canvas {
                                    id: crop_ink_canvas(k),
                                    class: ink_class,
                                    onpointerdown: {
                                        let mut down = pointer_down.clone();
                                        move |e| down(Target { page: p, bx: b, canvas: crop_ink_canvas(k) }, e)
                                    },
                                    onpointermove: pointer_move.clone(),
                                    onpointerup: pointer_up.clone(),
                                    onpointercancel: pointer_up.clone(),
                                }
                            }
                        }
                    }
                } else {
                    div { class: "sheet",
                        canvas { id: PAGE_CANVAS }
                        canvas {
                            id: INK_CANVAS,
                            class: ink_class,
                            onpointerdown: {
                                let mut down = pointer_down.clone();
                                move |e| {
                                    if let Some(b) = page_box() {
                                        down(Target { page: page(), bx: b, canvas: INK_CANVAS.into() }, e)
                                    }
                                }
                            },
                            onpointermove: pointer_move.clone(),
                            onpointerup: pointer_up.clone(),
                            onpointercancel: pointer_up.clone(),
                        }
                    }
                }
                if at_end && mode() == Mode::Read {
                    if let Some(r) = next_piece() {
                        Link { class: "next-piece", to: r, onclick: move |e: MouseEvent| e.stop_propagation(), {t::reader_next_piece()} }
                    }
                }
            }
        }
    }
}

#[component]
fn ToolButton(label: String, icon: &'static str, active: bool, onclick: EventHandler<MouseEvent>) -> Element {
    rsx! {
        button { class: if active { "tool on" } else { "tool" }, title: label, onclick: move |e| onclick.call(e),
            span { class: "tool-icon", "{icon}" }
            span { class: "tool-label", "{label}" }
        }
    }
}
