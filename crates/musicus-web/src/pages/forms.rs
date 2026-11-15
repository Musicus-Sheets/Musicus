//! Form pieces shared by the editing views of the library: score details,
//! the instrument picker, the editor of an existing part and the form that
//! adds a part together with its PDF.

use crate::{
    Route, api,
    picker::{AssignList, Choice},
    ui::{Status, confirm, format_size, format_time, none_if_empty, tags_from},
};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::{FileVersion, Instrument, Part, PartInput, Score, ScoreInput};

/// Score details as typed into the form (tags as comma separated text).
#[derive(Clone, PartialEq, Default)]
pub struct ScoreDraft {
    pub title: String,
    pub subtitle: String,
    pub composer: String,
    pub arranger: String,
    pub genre: String,
    pub tags: String,
    pub notes: String,
}

impl ScoreDraft {
    pub fn from_score(s: &Score) -> Self {
        let text = |v: &Option<String>| v.clone().unwrap_or_default();
        Self {
            title: s.title.clone(),
            subtitle: text(&s.subtitle),
            composer: text(&s.composer),
            arranger: text(&s.arranger),
            genre: text(&s.genre),
            tags: s.tags.join(", "),
            notes: text(&s.notes),
        }
    }

    pub fn to_input(&self) -> ScoreInput {
        ScoreInput {
            title: self.title.trim().to_owned(),
            subtitle: none_if_empty(self.subtitle.clone()),
            composer: none_if_empty(self.composer.clone()),
            arranger: none_if_empty(self.arranger.clone()),
            genre: none_if_empty(self.genre.clone()),
            tags: tags_from(&self.tags),
            notes: none_if_empty(self.notes.clone()),
        }
    }
}

/// The input fields of a score. The surrounding form submits them.
#[component]
pub fn ScoreFields(draft: Signal<ScoreDraft>) -> Element {
    let mut draft = draft;
    let d = draft();
    rsx! {
        div { class: "grid",
            label { "Title"
                input { required: true, value: "{d.title}", placeholder: "e.g. Florentiner Marsch",
                    oninput: move |e| draft.write().title = e.value() } }
            label { "Subtitle"
                input { value: "{d.subtitle}", oninput: move |e| draft.write().subtitle = e.value() } }
            label { "Composer"
                input { value: "{d.composer}", oninput: move |e| draft.write().composer = e.value() } }
            label { "Arranger"
                input { value: "{d.arranger}", oninput: move |e| draft.write().arranger = e.value() } }
            label { "Genre"
                input { placeholder: "March, polka, concert…", value: "{d.genre}",
                    oninput: move |e| draft.write().genre = e.value() } }
            label { "Tags (comma separated)"
                input { placeholder: "Christmas, Encore", value: "{d.tags}",
                    oninput: move |e| draft.write().tags = e.value() } }
            label { class: "wide", "Notes"
                textarea { rows: 3, value: "{d.notes}", oninput: move |e| draft.write().notes = e.value() } }
        }
    }
}

/// Instruments as picker choices: name, family as detail.
pub fn instrument_choices(instruments: &[Instrument]) -> Vec<Choice> {
    instruments.iter().map(|i| Choice::new(i.id, i.name.clone(), i.family.clone().unwrap_or_default(), "")).collect()
}

/// The instruments of a part: assigned list plus searchable picker. In the
/// creation form the selection is just form state; the part editor passes
/// `onsave` to store every change at once (a-assign-picker).
#[component]
pub fn InstrumentPicker(
    instruments: Vec<Instrument>,
    selected: Signal<Vec<i64>>,
    /// Called with the new list instead of changing `selected` directly.
    #[props(default)]
    onsave: Option<EventHandler<Vec<i64>>>,
    #[props(default)] busy: bool,
    #[props(default)] status: Option<Result<String, String>>,
) -> Element {
    let mut selected = selected;
    if instruments.is_empty() {
        return rsx! {
            p { class: "muted small",
                "No instruments yet. "
                Link { to: Route::Instruments {}, "Add them on the Instruments page." }
            }
        };
    }
    let mut change = move |next: Vec<i64>| match onsave {
        Some(h) => h.call(next),
        None => selected.set(next),
    };
    rsx! {
        AssignList {
            options: instrument_choices(&instruments),
            assigned: selected(),
            add_label: "+ Add instruments",
            empty: "No instruments yet: nobody gets this part through an instrument grant.",
            placeholder: "Search instrument or family…",
            busy,
            status,
            onadd: move |added: Vec<i64>| {
                let mut next = selected();
                next.extend(added.into_iter().filter(|a| !next.contains(a)).collect::<Vec<_>>());
                change(next);
            },
            onremove: move |gone: i64| change(selected().into_iter().filter(|x| *x != gone).collect()),
        }
    }
}

/// Uploads a PDF as the new version of a part.
pub async fn upload_pdf(part_id: i64, file: dioxus::html::FileData) -> Result<Part, String> {
    match file.read_bytes().await {
        Ok(bytes) => api::upload::<Part>(&format!("/api/parts/{part_id}/file"), &bytes, "application/pdf")
            .await
            .map_err(|e| e.message),
        Err(e) => Err(format!("Could not read the file: {e}")),
    }
}

/// Short description of a file version: "Version 2 · 1.2 MB · 4 pages".
pub fn describe_file(f: &FileVersion) -> String {
    let mut s = format!("Version {} · {}", f.version, format_size(f.size_bytes));
    if let Some(n) = f.page_count {
        s.push_str(&format!(" · {}", pages(n)));
    }
    s
}

pub fn pages(n: i64) -> String {
    if n == 1 { "1 page".into() } else { format!("{n} pages") }
}

/// Editor of an existing part: label, voice and key (saved with "Save
/// part"), instruments (saved as soon as they change), the PDF (upload /
/// replace, version history) and deletion.
#[component]
pub fn PartEditor(
    part: Part,
    instruments: Vec<Instrument>,
    onchange: EventHandler<()>,
    ondelete: EventHandler<()>,
    #[props(default)] versions_open: bool,
) -> Element {
    let id = part.id;
    let mut label = use_signal(|| part.label.clone());
    let mut voice = use_signal(|| part.voice.map(|v| v.to_string()).unwrap_or_default());
    let mut transposition = use_signal(|| part.transposition.clone().unwrap_or_default());
    let mut selected = use_signal(|| part.instrument_ids.clone());
    // What the server has: an immediate instrument save sends these values
    // for the other fields, so unsaved typing in them is not stored by it.
    let mut saved = use_signal(|| PartInput {
        label: part.label.clone(),
        instrument_ids: part.instrument_ids.clone(),
        voice: part.voice,
        transposition: part.transposition.clone(),
        notes: part.notes.clone(),
    });
    let mut status = use_signal(|| None::<Result<String, String>>);
    let mut instrument_status = use_signal(|| None::<Result<String, String>>);
    let mut instrument_busy = use_signal(|| false);
    let mut generation = use_signal(|| 0u32);
    let mut uploading = use_signal(|| false);
    let mut versions =
        use_resource(move || async move { api::get::<Vec<FileVersion>>(&format!("/api/parts/{id}/versions")).await });

    let save = move |e: FormEvent| async move {
        e.prevent_default();
        let input = PartInput {
            label: label().trim().to_owned(),
            instrument_ids: selected(),
            voice: voice().trim().parse().ok(),
            transposition: none_if_empty(transposition()),
            notes: saved().notes,
        };
        match api::put::<_, Part>(&format!("/api/parts/{id}"), &input).await {
            Ok(_) => {
                saved.set(input);
                status.set(Some(Ok("Saved".into())));
                onchange.call(());
            }
            Err(e) => status.set(Some(Err(e.message))),
        }
    };
    let save_instruments = move |next: Vec<i64>| {
        let before = selected();
        selected.set(next.clone());
        instrument_busy.set(true);
        generation.set(generation() + 1);
        instrument_status.set(Some(Ok("Saving…".into())));
        spawn(async move {
            let input = PartInput { instrument_ids: next, ..saved() };
            let result = api::put::<_, Part>(&format!("/api/parts/{id}"), &input).await;
            instrument_busy.set(false);
            match result {
                Ok(_) => {
                    saved.set(input);
                    instrument_status.set(Some(Ok("Saved".into())));
                    onchange.call(());
                    let mine = generation() + 1;
                    generation.set(mine);
                    gloo_timers::future::TimeoutFuture::new(2500).await;
                    if generation() == mine {
                        instrument_status.set(None);
                    }
                }
                Err(e) => {
                    selected.set(before);
                    generation.set(generation() + 1);
                    instrument_status.set(Some(Err(e.message)));
                }
            }
        });
    };
    let delete = move |_| async move {
        if !confirm("Delete this part? Everyone's annotations on it are deleted too.") {
            return;
        }
        match api::call(Method::DELETE, &format!("/api/parts/{id}")).await {
            Ok(()) => ondelete.call(()),
            Err(e) => status.set(Some(Err(e.message))),
        }
    };
    let upload = move |e: FormEvent| async move {
        let Some(file) = e.files().into_iter().next() else { return };
        uploading.set(true);
        status.set(Some(Ok(format!("Uploading {}…", file.name()))));
        let result = upload_pdf(id, file).await;
        uploading.set(false);
        match result {
            Ok(p) => {
                let v = p.file.map(|f| f.version).unwrap_or_default();
                status.set(Some(Ok(format!("Uploaded as version {v}"))));
                versions.restart();
                onchange.call(());
            }
            Err(e) => status.set(Some(Err(e))),
        }
    };
    let history = match &*versions.read() {
        Some(Ok(list)) => list.clone(),
        _ => Vec::new(),
    };
    rsx! {
        form { class: "part-editor", onsubmit: save, oninput: move |_| status.set(None),
            div { class: "part-fields",
                label { class: "grow", "Label"
                    input { required: true, value: "{label}", oninput: move |e| label.set(e.value()) } }
                label { class: "short", "Voice"
                    input { inputmode: "numeric", placeholder: "1", value: "{voice}", oninput: move |e| voice.set(e.value()) } }
                label { class: "short", "Key"
                    input { placeholder: "Bb", value: "{transposition}", oninput: move |e| transposition.set(e.value()) } }
            }
            div { class: "field-label", "Instruments (saved right away)" }
            InstrumentPicker { instruments: instruments.clone(), selected, onsave: save_instruments,
                busy: instrument_busy(), status: instrument_status() }
            div { class: "part-file",
                match &part.file {
                    Some(f) => rsx! {
                        span { class: "file-badge", "PDF" }
                        span { class: "muted small", "{describe_file(f)} · uploaded {format_time(f.uploaded_at)}" }
                        Link { class: "button", to: Route::Viewer { id }, "Open" }
                    },
                    None => rsx! { span { class: "warn-text small", "No PDF yet" } },
                }
                label { class: "button", class: if uploading() { "disabled" },
                    if uploading() { "Uploading…" } else if part.file.is_some() { "Replace PDF…" } else { "Upload PDF…" }
                    input { r#type: "file", accept: "application/pdf,.pdf", hidden: true, disabled: uploading(), onchange: upload }
                }
            }
            if history.len() > 1 || (versions_open && !history.is_empty()) {
                details { class: "versions", open: versions_open,
                    summary { "All versions ({history.len()})" }
                    ul { class: "plain",
                        for v in history.iter() {
                            li { key: "{v.version}", class: "row",
                                span {
                                    strong { "Version {v.version}" }
                                    if part.file.as_ref().is_some_and(|f| f.version == v.version) {
                                        span { class: "tag genre", "current" }
                                    }
                                }
                                span { class: "muted small", "{format_size(v.size_bytes)}"
                                    if let Some(n) = v.page_count { " · {pages(n)}" }
                                    " · {format_time(v.uploaded_at)}"
                                }
                            }
                        }
                    }
                    p { class: "muted small",
                        "Old versions stay stored, so markings made on them are kept and can be moved to the current version."
                    }
                }
            }
            div { class: "actions",
                button { r#type: "submit", "Save part" }
                Status { status: status() }
                span { class: "grow" }
                button { class: "danger", r#type: "button", onclick: delete, "Delete part" }
            }
        }
    }
}

/// Upload state of a part added in this session.
#[derive(Clone, PartialEq)]
pub enum Upload {
    /// No file chosen.
    NoFile,
    Running(String),
    Done(FileVersion),
    Failed(String),
}

/// Form that adds a part to a score, optionally with its PDF. The upload
/// runs in the background so the next part can be entered meanwhile;
/// `onstate` reports every change (created, uploading, done, failed).
#[component]
pub fn NewPartForm(score_id: i64, instruments: Vec<Instrument>, onstate: EventHandler<(Part, Upload)>) -> Element {
    let mut label = use_signal(String::new);
    let mut voice = use_signal(String::new);
    let mut transposition = use_signal(String::new);
    let mut selected = use_signal(Vec::<i64>::new);
    let mut file = use_signal(|| None::<dioxus::html::FileData>);
    let mut error = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);

    let add = move |e: FormEvent| async move {
        e.prevent_default();
        if busy() {
            return;
        }
        busy.set(true);
        let input = PartInput {
            label: label().trim().to_owned(),
            instrument_ids: selected(),
            voice: voice().trim().parse().ok(),
            transposition: none_if_empty(transposition()),
            notes: None,
        };
        let created = api::post::<_, Part>(&format!("/api/scores/{score_id}/parts"), &input).await;
        busy.set(false);
        let part = match created {
            Ok(p) => p,
            Err(e) => {
                error.set(Some(e.message));
                return;
            }
        };
        error.set(None);
        let chosen = file.take();
        label.set(String::new());
        voice.set(String::new());
        transposition.set(String::new());
        // Start the next part from scratch: a leftover tick would grant it to the wrong group.
        selected.set(Vec::new());
        // Lets the same file be chosen again for the next part.
        document::eval("document.querySelectorAll('form.new-part input[type=file]').forEach(i => i.value = '')");
        match chosen {
            None => onstate.call((part, Upload::NoFile)),
            Some(f) => {
                onstate.call((part.clone(), Upload::Running(f.name())));
                spawn(async move {
                    let state = match upload_pdf(part.id, f).await {
                        Ok(p) => p.file.map(Upload::Done).unwrap_or(Upload::NoFile),
                        Err(e) => Upload::Failed(e),
                    };
                    onstate.call((part, state));
                });
            }
        }
    };
    let file_name = file.read().as_ref().map(|f| f.name());
    rsx! {
        form { class: "new-part", onsubmit: add,
            div { class: "part-fields",
                label { class: "grow", "Label"
                    input { required: true, placeholder: "e.g. 2nd Trumpet in Bb", value: "{label}",
                        oninput: move |e| label.set(e.value()) } }
                label { class: "short", "Voice"
                    input { inputmode: "numeric", placeholder: "2", value: "{voice}", oninput: move |e| voice.set(e.value()) } }
                label { class: "short", "Key"
                    input { placeholder: "Bb", value: "{transposition}", oninput: move |e| transposition.set(e.value()) } }
            }
            div { class: "field-label", "Instruments" }
            InstrumentPicker { instruments: instruments.clone(), selected }
            div { class: "part-file",
                label { class: "button",
                    if file_name.is_some() { "Choose another PDF…" } else { "Choose PDF…" }
                    input { r#type: "file", accept: "application/pdf,.pdf", hidden: true,
                        onchange: move |e: FormEvent| file.set(e.files().into_iter().next()) }
                }
                match file_name {
                    Some(n) => rsx! { span { class: "file-badge", "PDF" } span { class: "small", "{n}" } },
                    None => rsx! { span { class: "muted small", "No file chosen. You can also upload it later." } },
                }
            }
            if let Some(e) = error() { div { class: "error", role: "alert", "{e}" } }
            div { class: "actions",
                button { class: "primary", r#type: "submit", disabled: busy(), "+ Add part" }
            }
        }
    }
}

/// One line per part added in this session, with its upload state.
#[component]
pub fn AddedParts(rows: Vec<(Part, Upload)>, names: std::collections::HashMap<i64, String>) -> Element {
    rsx! {
        ul { class: "added-parts",
            for (part, state) in rows.iter() {
                li { key: "{part.id}",
                    div { class: "grow",
                        strong { "{part.label}" }
                        span { class: "muted small",
                            " "
                            {part.instrument_ids.iter().filter_map(|i| names.get(i).cloned()).collect::<Vec<_>>().join(", ")}
                        }
                    }
                    match state {
                        Upload::NoFile => rsx! {
                            span { class: "warn-text small", "No PDF yet" }
                            Link { class: "button", to: Route::EditPart { id: part.id }, "Edit" }
                        },
                        Upload::Running(name) => rsx! {
                            progress { class: "uploading" }
                            span { class: "muted small", "Uploading {name}…" }
                        },
                        Upload::Done(f) => rsx! { span { class: "ok small", "✓ {describe_file(f)}" } },
                        Upload::Failed(e) => rsx! {
                            span { class: "error-text small", "Upload failed: {e}" }
                            Link { class: "button", to: Route::EditPart { id: part.id }, "Edit" }
                        },
                    }
                }
            }
        }
    }
}
