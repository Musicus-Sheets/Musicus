//! Mass import (librarians, F-LIB-004): the list of import batches and a new
//! import — select or drop a folder, choose the pattern with a live preview
//! (the same rules the server applies, `musicus_api::import`), then hash and
//! upload the files. The review of a batch is `import_review.rs`.
//! Nothing is stored on the device: the selected files live only in the
//! bridge's memory while the page is open (R-SEC-001).

use crate::{
    Route, api,
    bridge::{self, FolderFile},
    ui::{ErrorBox, Loading, NoAccess, can_manage, confirm, format_size, format_time},
};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::{
    Id, ImportBatch, ImportFileInput, Instrument, InstrumentAlias, NewImportBatch, RegisterImportFiles,
    RegisteredImportFiles,
    import::{
        Language, Level, detect, is_pdf_name,
        parts::suggest_language,
        pattern::{PRESETS, Pattern},
        review::{
            Alias, FileState, ItemState, Library, PartInput, PartOverrides, PieceOverrides, ReviewPiece, StagedFile,
            propose, review_piece,
        },
        strip_common_root,
    },
};

/// Where an upload of a folder stands.
#[derive(Clone, PartialEq, Debug)]
pub enum Progress {
    Idle,
    Hashing { done: usize, total: usize },
    Registering,
    Uploading { done: usize, total: usize, failed: usize, name: String },
    Finished { uploaded: usize, failed: usize, skipped: usize },
    Failed(String),
}

/// The PDFs of a selected folder, paths relative to it.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Selection {
    pub root: Option<String>,
    pub files: Vec<FolderFile>,
    /// Other files (not PDF), ignored.
    pub ignored: usize,
}

impl Selection {
    pub fn new(all: Vec<FolderFile>) -> Self {
        let paths: Vec<String> = all.iter().map(|f| f.path.clone()).collect();
        let (root, relative) = strip_common_root(&paths);
        let total = all.len();
        let files: Vec<FolderFile> = all
            .into_iter()
            .zip(relative)
            .filter(|(_, p)| is_pdf_name(p))
            .map(|(f, path)| FolderFile { path, ..f })
            .collect();
        Self { root, ignored: total - files.len(), files }
    }

    pub fn paths(&self) -> Vec<String> {
        self.files.iter().map(|f| f.path.clone()).collect()
    }
}

/// Hashes the selected files, registers them with the batch and uploads
/// what the server does not have yet (one file at a time, streamed).
pub async fn send_folder(batch_id: Id, selection: Selection, mut progress: Signal<Progress>) {
    let total = selection.files.len();
    let mut list = Vec::with_capacity(total);
    for (done, f) in selection.files.iter().enumerate() {
        progress.set(Progress::Hashing { done, total });
        match bridge::hash_import_file(f.index, f.size).await {
            Ok(sha256) => list.push(ImportFileInput { path: f.path.clone(), size_bytes: f.size as i64, sha256 }),
            Err(e) => return progress.set(Progress::Failed(format!("Could not read {}: {e}", f.path))),
        }
    }
    progress.set(Progress::Registering);
    let registered: RegisteredImportFiles =
        match api::post(&format!("/api/import/batches/{batch_id}/files"), &RegisterImportFiles { files: list }).await {
            Ok(r) => r,
            Err(e) => return progress.set(Progress::Failed(e.message)),
        };
    let todo = registered.upload.len();
    let mut failed = 0;
    for (done, u) in registered.upload.iter().enumerate() {
        progress.set(Progress::Uploading { done, total: todo, failed, name: u.path.clone() });
        let Some(f) = selection.files.iter().find(|f| f.path == u.path) else {
            failed += 1;
            continue;
        };
        match bridge::upload_import_file(f.index, &format!("/api/import/files/{}/content", u.id)).await {
            Ok((status, _)) if (200..300).contains(&status) => {}
            Ok((401, _)) => {
                let _ = web_sys::window().map(|w| w.location().set_href("/login"));
                return;
            }
            _ => failed += 1,
        }
    }
    progress.set(Progress::Finished { uploaded: todo - failed, failed, skipped: registered.files.duplicate as usize });
}

/// Progress of [`send_folder`] as text and bar.
#[component]
pub fn ProgressView(progress: Progress) -> Element {
    let (text, value) = match &progress {
        Progress::Idle => return rsx! {},
        Progress::Hashing { done, total } => (format!("Checking files… {done} of {total}"), Some((*done, *total))),
        Progress::Registering => ("Comparing with the library…".to_owned(), None),
        Progress::Uploading { done, total, failed, name } => {
            let failed = if *failed > 0 { format!(" ({failed} failed)") } else { String::new() };
            (format!("Uploading {done} of {total}{failed}: {name}"), Some((*done, *total)))
        }
        Progress::Finished { uploaded, failed, skipped } => {
            let mut t = format!("Upload finished: {uploaded} uploaded");
            if *skipped > 0 {
                t.push_str(&format!(", {skipped} already in the library or twice in the folder (skipped)"));
            }
            if *failed > 0 {
                t.push_str(&format!(", {failed} failed — select the folder again to retry"));
            }
            (t, None)
        }
        Progress::Failed(e) => return rsx! { ErrorBox { message: e.clone() } },
    };
    rsx! {
        div { class: "import-progress", role: "status", "aria-live": "polite",
            match value {
                Some((done, total)) => rsx! { progress { max: "{total.max(1)}", value: "{done}" } },
                None if matches!(progress, Progress::Finished { .. }) => rsx! {},
                None => rsx! { progress {} },
            }
            span { "{text}" }
        }
    }
}

/// "Select folder…" plus a drop zone. Calls `onfiles` with every file of the
/// chosen folder (all types; [`Selection::new`] keeps the PDFs).
#[component]
pub fn FolderPicker(onfiles: EventHandler<Vec<FolderFile>>, #[props(default)] disabled: bool) -> Element {
    let ids = use_hook(|| {
        let n = crate::picker::unique_id();
        (format!("folder-input-{n}"), format!("folder-drop-{n}"))
    });
    let (input_id, zone_id) = ids.clone();
    // Waits for drops as long as the picker is shown.
    use_future(move || {
        let zone = ids.1.clone();
        async move {
            loop {
                let files = bridge::drop_folder(&zone).await;
                if !files.is_empty() {
                    onfiles.call(files);
                }
            }
        }
    });
    let input_for_change = input_id.clone();
    rsx! {
        div { class: "dropzone", id: "{zone_id}",
            label { class: if disabled { "button primary disabled" } else { "button primary" },
                "Select folder…"
                input { id: "{input_id}", r#type: "file", hidden: true, multiple: true, disabled,
                    "webkitdirectory": "true", "directory": "true",
                    onchange: move |_| {
                        let files = bridge::pick_folder(&input_for_change);
                        if !files.is_empty() {
                            onfiles.call(files);
                        }
                    } }
            }
            span { class: "muted", "or drop the folder here" }
        }
    }
}

/// `/import`: batches and a new import.
#[component]
pub fn Imports() -> Element {
    if !can_manage() {
        return rsx! { NoAccess {} };
    }
    let mut reload = use_signal(|| 0u32);
    let batches = use_resource(move || {
        reload();
        api::get::<Vec<ImportBatch>>("/api/import/batches")
    });
    rsx! {
        main {
            div { class: "page-head",
                h1 { "Import" }
            }
            p { class: "muted",
                "Import a whole folder of PDF parts: Musicus reads number, title, composer and the parts from the folder and file names. "
                "Nothing reaches the library until you have checked and imported it."
            }
            NewImport {}
            h2 { "Earlier imports" }
            match &*batches.read() {
                None => rsx! { Loading {} },
                Some(Err(e)) => rsx! { ErrorBox { message: e.message.clone() } },
                Some(Ok(list)) if list.is_empty() => rsx! { p { class: "muted", "No imports yet." } },
                Some(Ok(list)) => rsx! {
                    ul { class: "batch-list",
                        for b in list.iter() {
                            BatchRow { key: "{b.id}", batch: b.clone(), ondelete: move |_| reload += 1 }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn BatchRow(batch: ImportBatch, ondelete: EventHandler<()>) -> Element {
    let id = batch.id;
    let mut error = use_signal(|| None::<String>);
    let delete = move |_| async move {
        if !confirm("Forget this import? Pieces already imported stay in the library.") {
            return;
        }
        match api::call(Method::DELETE, &format!("/api/import/batches/{id}")).await {
            Ok(()) => ondelete.call(()),
            Err(e) => error.set(Some(e.message)),
        }
    };
    let f = &batch.files;
    let who = batch.created_by.clone().map(|w| format!(" by {w}")).unwrap_or_default();
    rsx! {
        li { class: "row",
            div { class: "grow",
                Link { to: Route::ImportReview { id }, strong { "{batch.name}" } }
                div { class: "muted small", "{format_time(batch.created_at)}{who} · {batch.pieces} pieces, {batch.committed} imported · {f.total} files" }
                if f.pending + f.failed > 0 {
                    div { class: "warn-text small", "{f.pending + f.failed} files not uploaded yet" }
                }
                if let Some(e) = error() { div { class: "error-text small", "{e}" } }
            }
            Link { class: "button", to: Route::ImportReview { id }, "Review" }
            button { class: "link danger", onclick: delete, "Forget" }
        }
    }
}

/// Preview of a pattern on the selected files: the same proposal and colours
/// the server will compute (without the comparison with existing pieces).
pub struct Preview {
    pub pieces: Vec<ReviewPiece>,
    pub unmatched: Vec<String>,
    pub error: Option<String>,
}

pub fn preview(
    paths: &[String],
    pattern: &str,
    language: Language,
    instruments: &[Instrument],
    aliases: &[Alias],
) -> Preview {
    let pattern = match Pattern::parse(pattern) {
        Ok(p) => p,
        Err(e) => return Preview { pieces: vec![], unmatched: vec![], error: Some(e.to_string()) },
    };
    let (proposed, unmatched) = propose(&pattern, language, paths);
    let lib = Library { instruments, aliases, scores: &[], groups: &[] };
    let none = PartOverrides::default();
    let pieces = proposed
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let parts = p
                .parts
                .iter()
                .enumerate()
                .map(|(k, g)| PartInput {
                    id: k as Id,
                    guess: g,
                    overrides: &none,
                    file: StagedFile {
                        id: k as Id,
                        path: g.path.clone(),
                        size_bytes: 0,
                        sha256: String::new(),
                        state: FileState::Uploaded,
                        page_count: None,
                        has_thumbnail: false,
                        staves: None,
                        message: None,
                    },
                    state: ItemState::Pending,
                    part_id: None,
                })
                .collect();
            review_piece(i as Id, ItemState::Pending, &p.piece, &PieceOverrides::default(), None, parts, &lib)
        })
        .collect();
    Preview { pieces, unmatched, error: None }
}

/// Counts of green, yellow and red pieces.
pub fn level_counts(pieces: &[ReviewPiece]) -> (usize, usize, usize) {
    let n = |l| pieces.iter().filter(|p| p.state == ItemState::Pending && p.level == l).count();
    (n(Level::Green), n(Level::Yellow), n(Level::Red))
}

/// The name a part will get, or its file-name text while an instrument is unknown.
pub fn part_label(part: &musicus_api::import::review::ReviewPart) -> String {
    if part.instruments.iter().any(|i| i.instrument_id.is_none()) || part.instruments.is_empty() {
        format!("“{}”", part.text)
    } else {
        part.display_name.clone()
    }
}

pub fn level_class(level: Level) -> &'static str {
    match level {
        Level::Green => "lvl-green",
        Level::Yellow => "lvl-yellow",
        Level::Red => "lvl-red",
    }
}

/// A new import: folder, pattern, language, preview, start.
#[component]
fn NewImport() -> Element {
    let nav = navigator();
    let mut selection = use_signal(Selection::default);
    let mut pattern = use_signal(|| PRESETS[0].pattern.to_owned());
    let mut language = use_signal(Language::default);
    let mut suggested = use_signal(|| None::<Language>);
    let progress = use_signal(|| Progress::Idle);
    let mut error = use_signal(|| None::<String>);
    let instruments = use_resource(|| api::get::<Vec<Instrument>>("/api/instruments"));
    let aliases = use_resource(|| api::get::<Vec<InstrumentAlias>>("/api/import/aliases"));

    let choose = move |all: Vec<FolderFile>| {
        let sel = Selection::new(all);
        let paths = sel.paths();
        let fits = detect(&paths);
        if let Some(best) = fits.first().filter(|f| f.matched > 0) {
            pattern.set(best.preset.pattern.to_owned());
            let p = Pattern::parse(best.preset.pattern).expect("presets are valid");
            let texts: Vec<String> = paths.iter().filter_map(|x| p.matches(x)).map(|f| f.part).collect();
            let lang = suggest_language(texts.iter().map(String::as_str));
            suggested.set(lang);
            language.set(lang.unwrap_or_default());
        }
        error.set(None);
        selection.set(sel);
    };

    let busy = !matches!(progress(), Progress::Idle | Progress::Failed(_));
    let sel = selection();
    let fits = detect(&sel.paths());
    let instruments_list = match &*instruments.read() {
        Some(Ok(l)) => l.clone(),
        _ => vec![],
    };
    let alias_list: Vec<Alias> = match &*aliases.read() {
        Some(Ok(l)) => l.iter().map(|a| Alias { alias: a.alias.clone(), instrument_id: a.instrument_id }).collect(),
        _ => vec![],
    };
    let shown = preview(&sel.paths(), &pattern(), language(), &instruments_list, &alias_list);
    let (green, yellow, red) = level_counts(&shown.pieces);

    let start = move |_| async move {
        let sel = selection();
        let name = sel.root.clone().unwrap_or_else(|| "Import".into());
        let input = NewImportBatch { name, pattern: pattern(), language: language() };
        let batch: ImportBatch = match api::post("/api/import/batches", &input).await {
            Ok(b) => b,
            Err(e) => return error.set(Some(e.message)),
        };
        send_folder(batch.id, sel, progress).await;
        bridge::clear_import_files();
        if !matches!(progress(), Progress::Failed(_)) {
            nav.push(Route::ImportReview { id: batch.id });
        }
    };

    rsx! {
        section { class: "card import-new",
            h2 { "New import" }
            FolderPicker { onfiles: choose, disabled: busy }
            if !sel.files.is_empty() {
                p { class: "small",
                    strong { "{sel.files.len()} PDF files" }
                    if let Some(r) = &sel.root { " in “{r}”" }
                    if sel.ignored == 1 { span { class: "muted", " (1 other file is ignored)" } }
                    if sel.ignored > 1 { span { class: "muted", " ({sel.ignored} other files are ignored)" } }
                }
                div { class: "grid",
                    label { "Structure"
                        select {
                            onchange: move |e: FormEvent| pattern.set(e.value()),
                            for f in fits.iter() {
                                option { value: "{f.preset.pattern}", selected: pattern() == f.preset.pattern,
                                    "{f.preset.name} — {f.matched} of {sel.files.len()} files" }
                            }
                            if !PRESETS.iter().any(|p| p.pattern == pattern()) {
                                option { value: "{pattern}", selected: true, "Own pattern" }
                            }
                        }
                    }
                    label { "Naming language"
                        select {
                            onchange: move |e: FormEvent| language.set(Language::parse(&e.value()).unwrap_or_default()),
                            option { value: "de", selected: language() == Language::De,
                                if suggested() == Some(Language::De) { "German (suggested): B = B♭, Es = E♭, H = B♮" } else { "German: B = B♭, Es = E♭, H = B♮" } }
                            option { value: "en", selected: language() == Language::En,
                                if suggested() == Some(Language::En) { "English (suggested): B is not read as a key" } else { "English: B is not read as a key" } }
                        }
                    }
                    label { class: "wide", "Pattern"
                        input { class: "mono", value: "{pattern}", spellcheck: false,
                            oninput: move |e| pattern.set(e.value()) }
                        span { class: "muted small",
                            "{{number}} {{title}} {{composer}} {{arranger}} {{genre}} {{part}}, {{*}} = ignore, [ … ] = optional, / = folder"
                        }
                    }
                }
                if let Some(e) = &shown.error {
                    div { class: "error", role: "alert", "{e}" }
                } else {
                    p { class: "import-summary",
                        "{shown.pieces.len()} pieces: "
                        span { class: "count lvl-green", "{green} ready" } " · "
                        span { class: "count lvl-yellow", "{yellow} to check" } " · "
                        span { class: "count lvl-red", "{red} to complete" }
                        if !shown.unmatched.is_empty() {
                            " · "
                            span { class: "count lvl-red", "{shown.unmatched.len()} files do not fit the pattern" }
                        }
                    }
                    PreviewTable { pieces: shown.pieces.iter().take(12).cloned().collect::<Vec<_>>(), more: shown.pieces.len().saturating_sub(12) }
                    if !shown.unmatched.is_empty() {
                        details { class: "small",
                            summary { "Files that do not fit the pattern" }
                            ul { for p in shown.unmatched.iter().take(50) { li { class: "mono", "{p}" } } }
                        }
                    }
                }
                ProgressView { progress: progress() }
                if let Some(e) = error() { ErrorBox { message: e } }
                div { class: "actions",
                    span { class: "muted small", "{format_size(sel.files.iter().map(|f| f.size as i64).sum())} to check. You can review everything before anything is imported." }
                    button { class: "primary", disabled: busy || shown.error.is_some() || shown.pieces.is_empty(), onclick: start,
                        "Upload and review"
                    }
                }
            }
        }
    }
}

/// A few proposed pieces with their parts, coloured.
#[component]
fn PreviewTable(pieces: Vec<ReviewPiece>, more: usize) -> Element {
    rsx! {
        // Fixed widths for number, title, composer and genre; the parts take
        // the rest and wrap onto several lines.
        div { class: "table-scroll",
        table { class: "import-preview",
            colgroup { col { class: "no" } col { class: "title" } col { class: "composer" } col { class: "genre" } col { class: "preview-parts" } }
            thead { tr { th { "No." } th { "Title" } th { "Composer" } th { "Genre" } th { "Parts" } } }
            tbody {
                for p in pieces.iter() {
                    tr { key: "{p.id}", class: level_class(p.level),
                        td { class: level_class(p.number.level), {p.number.value.clone().unwrap_or_default()} }
                        td { class: level_class(p.title.level), {p.title.value.clone().unwrap_or_else(|| "— title missing —".into())} }
                        td { {p.composer.value.clone().unwrap_or_default()} }
                        td { {p.genre.value.clone().unwrap_or_default()} }
                        td { class: "preview-parts",
                            div { class: "ichips",
                                for (i, part) in p.parts.iter().enumerate() {
                                    span { key: "{i}", class: "ichip {level_class(part.level)}", title: "{part.text}", {part_label(part)} }
                                }
                            }
                        }
                    }
                }
            }
        }
        }
        if more > 0 { p { class: "muted small", "… and {more} more pieces." } }
    }
}
