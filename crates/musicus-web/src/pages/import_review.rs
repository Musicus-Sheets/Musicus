//! Review of an import batch (`/import/:id`, librarians): one row per
//! proposed piece with its parts, every value with its source and colour
//! (green sure, yellow check, red blocks), unknown instruments mapped once
//! (stored alias), existing pieces offered for confirmation, bulk edit,
//! import per piece or "all green", undo of the batch, and continuing an
//! interrupted upload by selecting the folder again.

use crate::{
    Route, api,
    bridge::{self, FolderFile},
    errors::{code_text, note_text},
    pages::{
        forms::instrument_choices,
        import::{FolderPicker, Progress, ProgressView, Selection, level_class, level_counts, part_label, send_folder},
    },
    picker::{Choice, Picker},
    tag_input::TagInput,
    ui::{ErrorBox, Loading, NoAccess, can_manage, confirm, format_size},
};
use dioxus::prelude::*;
use gloo_net::http::Method;
use musicus_api::{
    CommitImport, CommittedImport, Id, ImportReview as Review, Instrument, InstrumentAlias, InstrumentAliasInput,
    InstrumentInput, UndoneImport, UpdateImportBatch, UpdateImportItems,
    import::{
        Language, Level, Note, Source,
        pattern::PRESETS,
        review::{Field, InstrumentPick, ItemState, PartOverrides, PieceOverrides, ReviewPart, ReviewPiece, Target},
    },
    naming,
};
use musicus_i18n::t;
use std::collections::{BTreeMap, HashSet};

/// Pieces per page of the review.
const PAGE: usize = 20;

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Attention,
    Ready,
    Imported,
}

impl Filter {
    fn keeps(self, p: &ReviewPiece) -> bool {
        match self {
            Filter::All => true,
            Filter::Attention => p.state == ItemState::Pending && p.level != Level::Green,
            Filter::Ready => p.is_green(),
            Filter::Imported => p.state == ItemState::Committed,
        }
    }
}

fn source_text(source: Option<Source>) -> String {
    match source {
        Some(Source::Pattern) => t::import_source_pattern(),
        Some(Source::Dictionary) => t::import_source_dictionary(),
        Some(Source::Alias) => t::import_source_alias(),
        Some(Source::Library) => t::import_source_library(),
        Some(Source::Librarian) => t::import_source_librarian(),
        Some(Source::Inventory) => t::import_source_inventory(),
        Some(Source::Ocr) => t::import_source_ocr(),
        None => String::new(),
    }
}

/// Tooltip of a value: source and note.
fn hint(field: &Field) -> String {
    let mut t = source_hint(field.source, field.note.as_ref());
    if !field.others.is_empty() {
        t.push_str(&t::import_also(field.others.join(", ")));
    }
    t
}

fn source_hint(source: Option<Source>, note: Option<&Note>) -> String {
    let mut t = source_text(source);
    if let Some(n) = note {
        if !t.is_empty() {
            t.push_str(" — ");
        }
        t.push_str(&note_text(n));
    }
    t
}

/// Replaces changed pieces in the loaded review.
fn merge_pieces(mut review: Signal<Option<Review>>, changed: Vec<ReviewPiece>) {
    if let Some(r) = review.write().as_mut() {
        for c in changed {
            if let Some(p) = r.pieces.iter_mut().find(|p| p.id == c.id) {
                *p = c;
            }
        }
    }
}

/// Stores changes; `false` if the server refused them (the message is shown).
async fn patch(
    review: Signal<Option<Review>>,
    mut error: Signal<Option<String>>,
    ids: Vec<Id>,
    piece: Option<PieceOverrides>,
    part: Option<PartOverrides>,
) -> bool {
    let body = UpdateImportItems { ids, piece, part, reset: false };
    match api::send::<_, Vec<ReviewPiece>>(Method::PATCH, "/api/import/items", &body).await {
        Ok(changed) => {
            error.set(None);
            merge_pieces(review, changed);
            true
        }
        Err(e) => {
            error.set(Some(e.message));
            false
        }
    }
}

/// `/import/:id`.
#[component]
pub fn ImportReview(id: Id) -> Element {
    if !can_manage() {
        return rsx! { NoAccess {} };
    }
    let mut review = use_signal(|| None::<Review>);
    let mut load_error = use_signal(|| None::<String>);
    let mut reload = use_signal(|| 0u32);
    let error = use_signal(|| None::<String>);
    let mut message = use_signal(|| None::<String>);
    let mut filter = use_signal(|| Filter::All);
    let mut page = use_signal(|| 0usize);
    let selected = use_signal(HashSet::<Id>::new);
    let progress = use_signal(|| Progress::Idle);
    let mut busy = use_signal(|| false);
    let instruments = use_resource(move || {
        reload();
        api::get::<Vec<Instrument>>("/api/instruments")
    });
    use_effect(move || {
        reload();
        spawn(async move {
            match api::get::<Review>(&format!("/api/import/batches/{id}")).await {
                Ok(r) => {
                    load_error.set(None);
                    review.set(Some(r));
                }
                Err(e) => load_error.set(Some(e.message)),
            }
        });
    });
    let instruments: Vec<Instrument> = match &*instruments.read() {
        Some(Ok(l)) => {
            let mut l = l.clone();
            crate::locale::sort_by_text(&mut l, |i| &i.name);
            l
        }
        _ => vec![],
    };

    let Some(r) = review() else {
        return rsx! {
            main {
                match load_error() {
                    Some(e) => rsx! { ErrorBox { message: e } },
                    None => rsx! { Loading {} },
                }
            }
        };
    };

    let commit = move |ids: Option<Vec<Id>>| async move {
        busy.set(true);
        let result: Result<CommittedImport, _> =
            api::post(&format!("/api/import/batches/{id}/commit"), &CommitImport { piece_ids: ids }).await;
        busy.set(false);
        match result {
            Ok(done) => {
                let mut text = t::import_committed(done.committed.len());
                for e in &done.errors {
                    text.push_str(&t::import_not_committed(code_text(&e.code)));
                }
                message.set(Some(text));
                reload += 1;
            }
            Err(e) => message.set(Some(e.message)),
        }
    };
    let undo = move |_| async move {
        if !confirm(&t::import_undo_confirm()) {
            return;
        }
        match api::post::<_, UndoneImport>(&format!("/api/import/batches/{id}/undo"), &()).await {
            Ok(u) => message.set(Some(t::import_undone(u.pieces, u.parts))),
            Err(e) => message.set(Some(e.message)),
        }
        reload += 1;
    };
    let resume = move |all: Vec<FolderFile>| {
        spawn(async move {
            send_folder(id, Selection::new(all), progress).await;
            bridge::clear_import_files();
            reload += 1;
        });
    };

    let (green, yellow, red) = level_counts(&r.pieces);
    let committed = r.pieces.iter().filter(|p| p.state == ItemState::Committed).count();
    // Sorted in the browser before paging (a-browser-sort).
    let mut shown: Vec<&ReviewPiece> = r.pieces.iter().filter(|p| filter().keeps(p)).collect();
    shown.sort_by(|a, b| crate::pages::import::cmp_pieces(a, b));
    let pages = shown.len().div_ceil(PAGE).max(1);
    let current = page().min(pages - 1);
    let visible: Vec<ReviewPiece> = shown.iter().skip(current * PAGE).take(PAGE).map(|p| (*p).clone()).collect();
    let f = &r.batch.files;
    let missing = f.pending + f.failed;
    let uploading = !matches!(progress(), Progress::Idle | Progress::Finished { .. } | Progress::Failed(_));

    rsx! {
        main { class: "import-review",
            nav { class: "crumbs", Link { to: Route::Imports {}, {t::import_crumb()} } }
            div { class: "page-head",
                h1 { "{r.batch.name}" }
            }
            BatchSettings { batch: r.batch.clone(), onchange: move |_| reload += 1 }
            p { class: "import-summary",
                {t::import_summary_pieces(r.pieces.len())}
                span { class: "count lvl-green", {t::import_count_ready(green)} } " · "
                span { class: "count lvl-yellow", {t::import_count_check(yellow)} } " · "
                span { class: "count lvl-red", {t::import_count_complete(red)} } " · "
                span { class: "count", {t::import_count_imported(committed)} }
                br {}
                span { class: "muted small",
                    {t::import_files_summary(f.total, f.uploaded, f.duplicate, f.unmatched)}
                }
            }
            if missing > 0 || uploading {
                section { class: "card banner-card",
                    if missing > 0 {
                        p { class: "warn-text", {t::import_resume(missing)} }
                    }
                    FolderPicker { onfiles: resume, disabled: uploading }
                    ProgressView { progress: progress() }
                }
            }
            UnknownInstruments { pieces: r.pieces.clone(), instruments: instruments.clone(), onchange: move |_| reload += 1 }
            div { class: "review-tools",
                select { "aria-label": t::import_filter(),
                    onchange: move |e: FormEvent| {
                        filter.set(match e.value().as_str() {
                            "attention" => Filter::Attention, // i18n-ok: option value
                            "ready" => Filter::Ready, // i18n-ok: option value
                            "imported" => Filter::Imported, // i18n-ok: option value
                            _ => Filter::All,
                        });
                        page.set(0);
                    },
                    option { value: "all", {t::import_filter_all()} }
                    option { value: "attention", {t::import_filter_attention()} }
                    option { value: "ready", {t::import_filter_ready()} }
                    option { value: "imported", {t::import_filter_imported()} }
                }
                span { class: "grow" }
                button { class: "primary", disabled: busy() || green == 0,
                    onclick: move |_| commit(None),
                    {t::import_commit_green(green)}
                }
                if committed > 0 {
                    button { class: "danger", disabled: busy(), onclick: undo, {t::import_undo()} }
                }
            }
            if let Some(m) = message() { div { class: "banner info", role: "status", "{m}" } }
            if let Some(e) = error() { ErrorBox { message: e } }
            BulkEdit { review, error, selected }
            div { class: "review-list",
                for p in visible.iter() {
                    PieceRow { key: "{p.id}", piece: p.clone(), instruments: instruments.clone(), review, error, selected,
                        oncommit: move |pid: Id| commit(Some(vec![pid])) }
                }
            }
            if pages > 1 {
                div { class: "pager",
                    button { disabled: current == 0, onclick: move |_| page.set(current.saturating_sub(1)), {t::import_previous()} }
                    span { {t::import_page(current + 1, pages)} }
                    button { disabled: current + 1 >= pages, onclick: move |_| page.set(current + 1), {t::import_next()} }
                }
            }
            if !r.unmatched.is_empty() {
                details { class: "card",
                    summary { {t::import_count_unmatched(r.unmatched.len())} }
                    ul { class: "small", for f in r.unmatched.iter() { li { class: "mono", key: "{f.id}", "{f.path}" } } }
                }
            }
            if !r.duplicates.is_empty() {
                details { class: "card",
                    summary { {t::import_duplicates(r.duplicates.len())} }
                    ul { class: "small",
                        for f in r.duplicates.iter() {
                            li { key: "{f.id}", span { class: "mono", "{f.path}" } " — " span { class: "muted", {f.problem.as_ref().map(code_text).unwrap_or_default()} } }
                        }
                    }
                }
            }
        }
    }
}

/// Pattern and language of the batch; changing them reads the names again.
#[component]
fn BatchSettings(batch: musicus_api::ImportBatch, onchange: EventHandler<()>) -> Element {
    let id = batch.id;
    let mut editing = use_signal(|| false);
    let mut pattern = use_signal(|| batch.pattern.clone());
    let mut language = use_signal(|| batch.language);
    let mut error = use_signal(|| None::<String>);
    let save = move |_| async move {
        if !confirm(&t::import_read_again_confirm()) {
            return;
        }
        let body = UpdateImportBatch { name: None, pattern: Some(pattern()), language: Some(language()) };
        match api::put::<_, musicus_api::ImportBatch>(&format!("/api/import/batches/{id}"), &body).await {
            Ok(_) => {
                editing.set(false);
                error.set(None);
                onchange.call(());
            }
            Err(e) => error.set(Some(e.message)),
        }
    };
    let lang = if batch.language == Language::De { t::import_language_name_de() } else { t::import_language_name_en() };
    if !editing() {
        return rsx! {
            p { class: "muted small batch-settings",
                {t::import_settings_pattern()} code { "{batch.pattern}" } {t::import_settings_language(lang)}
                button { class: "link", onclick: move |_| editing.set(true), {t::import_settings_change()} }
            }
        };
    }
    rsx! {
        section { class: "card",
            div { class: "grid",
                label { class: "wide", {t::import_pattern()}
                    input { class: "mono", value: "{pattern}", oninput: move |e| pattern.set(e.value()) }
                    span { class: "muted small", {t::import_presets()}
                        for p in PRESETS.iter() {
                            button { class: "link", key: "{p.id}", onclick: move |_| pattern.set(p.pattern.to_owned()), {crate::pages::import::preset_name(p)} }
                            " "
                        }
                    }
                }
                label { {t::import_naming_language()}
                    select { onchange: move |e: FormEvent| language.set(Language::parse(&e.value()).unwrap_or_default()),
                        option { value: "de", selected: language() == Language::De, {t::import_language_de()} }
                        option { value: "en", selected: language() == Language::En, {t::import_language_en()} }
                    }
                }
            }
            if let Some(e) = error() { ErrorBox { message: e } }
            div { class: "actions",
                button { onclick: move |_| editing.set(false), {t::common_cancel()} }
                button { class: "primary", onclick: save, {t::import_read_again()} }
            }
        }
    }
}

/// Instrument words the library does not know (red) or that are only
/// abbreviations (yellow): map each once; the mapping is stored as alias
/// and used for every row and every later import (R-LIB-020).
#[component]
fn UnknownInstruments(pieces: Vec<ReviewPiece>, instruments: Vec<Instrument>, onchange: EventHandler<()>) -> Element {
    let mut words: BTreeMap<String, Word> = BTreeMap::new();
    for p in pieces.iter().filter(|p| p.state == ItemState::Pending) {
        for part in p.parts.iter().filter(|x| !x.skip && x.state == ItemState::Pending) {
            for i in part.instruments.iter().filter(|i| i.level != Level::Green && !i.alias.is_empty()) {
                let w = words.entry(i.alias.clone()).or_insert(Word {
                    text: i.text.clone(),
                    entry: i.entry.clone(),
                    count: 0,
                    level: i.level,
                    current: i.instrument_id,
                });
                w.count += 1;
                w.level = w.level.max(i.level);
            }
        }
    }
    let mut error = use_signal(|| None::<String>);
    if words.is_empty() {
        return rsx! {};
    }
    rsx! {
        section { class: "card unknown-instruments",
            h2 { {t::import_confirm_title()} }
            p { class: "muted small",
                {t::import_confirm_intro()}
            }
            if let Some(e) = error() { ErrorBox { message: e } }
            ul { class: "rows",
                for (alias, w) in words.into_iter() {
                    AliasRow { key: "{alias}", alias, text: w.text, entry: w.entry, count: w.count, level: w.level, current: w.current, instruments: instruments.clone(),
                        onerror: move |e| error.set(Some(e)), onchange }
                }
            }
        }
    }
}

/// An instrument word to confirm: as written, dictionary name, files, worst level, instrument found.
struct Word {
    text: String,
    entry: Option<String>,
    count: usize,
    level: Level,
    current: Option<Id>,
}

#[component]
fn AliasRow(
    alias: String,
    text: String,
    entry: Option<String>,
    count: usize,
    level: Level,
    current: Option<Id>,
    instruments: Vec<Instrument>,
    onerror: EventHandler<String>,
    onchange: EventHandler<()>,
) -> Element {
    let mut choice = use_signal(move || current);
    let alias_for_use = alias.clone();
    let use_it = move |_| {
        let alias = alias_for_use.clone();
        async move {
            let Some(instrument_id) = choice() else { return };
            match api::put::<_, InstrumentAlias>("/api/import/aliases", &InstrumentAliasInput { alias, instrument_id })
                .await
            {
                Ok(_) => onchange.call(()),
                Err(e) => onerror.call(e.message),
            }
        }
    };
    let new_name = entry.clone().unwrap_or_else(|| text.clone());
    let alias_for_create = alias.clone();
    let name_for_create = new_name.clone();
    let create = move |_| {
        let (alias, name) = (alias_for_create.clone(), name_for_create.clone());
        async move {
            let made = api::post::<_, Instrument>("/api/instruments", &InstrumentInput { name, family: None }).await;
            let instrument = match made {
                Ok(i) => i,
                Err(e) => return onerror.call(e.message),
            };
            let input = InstrumentAliasInput { alias, instrument_id: instrument.id };
            match api::put::<_, InstrumentAlias>("/api/import/aliases", &input).await {
                Ok(_) => onchange.call(()),
                Err(e) => onerror.call(e.message),
            }
        }
    };
    let exists = instruments.iter().any(|i| i.name.eq_ignore_ascii_case(&new_name));
    let unknown = if level == Level::Red { "unknown" } else { "abbreviation" }; // i18n-ok: selector
    rsx! {
        li { class: "row {level_class(level)}",
            div { class: "grow",
                strong { "“{text}”" }
                span { class: "muted small", {t::import_word_count(unknown, count)} }
                if let Some(e) = &entry { span { class: "muted small", {t::import_word_probably(e)} } }
            }
            select { "aria-label": t::import_word_instrument(&text),
                onchange: move |e: FormEvent| choice.set(e.value().parse().ok()),
                option { value: "", selected: choice().is_none(), {t::import_word_choose()} }
                for i in instruments.iter() {
                    option { key: "{i.id}", value: "{i.id}", selected: choice() == Some(i.id), "{i.name}" }
                }
            }
            button { disabled: choice().is_none(), onclick: use_it, {t::import_word_use(&text)} }
            if !exists {
                button { onclick: create, {t::import_word_create(&new_name)} }
            }
        }
    }
}

/// Genre, composer or arranger for all ticked pieces at once.
#[component]
fn BulkEdit(review: Signal<Option<Review>>, error: Signal<Option<String>>, selected: Signal<HashSet<Id>>) -> Element {
    let mut genre = use_signal(String::new);
    let mut composer = use_signal(String::new);
    let mut arranger = use_signal(String::new);
    let count = selected.read().len();
    if count == 0 {
        return rsx! {
            p { class: "muted small", {t::import_bulk_hint()} }
        };
    }
    let apply = move |_| async move {
        let some = |s: String| (!s.trim().is_empty()).then_some(s);
        let changes = PieceOverrides {
            genre: some(genre()),
            composer: some(composer()),
            arranger: some(arranger()),
            ..Default::default()
        };
        let ids: Vec<Id> = selected.read().iter().copied().collect();
        patch(review, error, ids, Some(changes), None).await;
        genre.set(String::new());
        composer.set(String::new());
        arranger.set(String::new());
    };
    rsx! {
        section { class: "card bulk-edit",
            strong { {t::common_picker_selected(count)} }
            input { placeholder: t::library_field_genre(), value: "{genre}", oninput: move |e| genre.set(e.value()) }
            input { placeholder: t::library_field_composer(), value: "{composer}", oninput: move |e| composer.set(e.value()) }
            input { placeholder: t::library_field_arranger(), value: "{arranger}", oninput: move |e| arranger.set(e.value()) }
            button { class: "primary", onclick: apply, {t::import_bulk_apply(count)} }
            button { class: "link", onclick: move |_| selected.write().clear(), {t::import_bulk_clear()} }
        }
    }
}

/// One editable value of a piece.
#[component]
fn PieceField(
    label: String,
    field: Field,
    disabled: bool,
    #[props(default)] wide: bool,
    onsave: EventHandler<String>,
) -> Element {
    let value = field.value.clone().unwrap_or_default();
    let tip = hint(&field);
    let class = format!("{} {}", level_class(field.level), if wide { "wide" } else { "" });
    rsx! {
        label { class: "{class}", title: "{tip}",
            span { class: "field-label", "{label}" }
            input { value: "{value}", disabled,
                onchange: move |e: FormEvent| onsave.call(e.value()) }
            if let Some(n) = &field.note { span { class: "note small", {note_text(n)} } }
        }
    }
}

#[component]
fn PieceRow(
    piece: ReviewPiece,
    instruments: Vec<Instrument>,
    review: Signal<Option<Review>>,
    error: Signal<Option<String>>,
    selected: Signal<HashSet<Id>>,
    oncommit: EventHandler<Id>,
) -> Element {
    let id = piece.id;
    let done = piece.state == ItemState::Committed;
    let set = move |f: fn(&mut PieceOverrides, String)| {
        move |v: String| {
            let mut o = PieceOverrides::default();
            f(&mut o, v);
            spawn(async move {
                patch(review, error, vec![id], Some(o), None).await;
            });
        }
    };
    let target = move |t: Target| {
        spawn(async move {
            patch(review, error, vec![id], Some(PieceOverrides { target: Some(t), ..Default::default() }), None).await;
        });
    };
    let ticked = selected.read().contains(&id);
    let status = if done { "lvl-done" } else { level_class(piece.level) };
    rsx! {
        article { class: "import-piece {status}", id: "import-piece-{id}",
            div { class: "piece-head",
                if !done {
                    input { r#type: "checkbox", checked: ticked, "aria-label": t::import_select_piece(),
                        onchange: move |e: FormEvent| {
                            if e.checked() { selected.write().insert(id); } else { selected.write().remove(&id); }
                        } }
                }
                div { class: "grid piece-fields",
                    PieceField { label: t::import_field_number(), field: piece.number.clone(), disabled: done, onsave: set(|o, v| o.number = Some(v)) }
                    PieceField { label: t::library_field_title(), field: piece.title.clone(), disabled: done, wide: true, onsave: set(|o, v| o.title = Some(v)) }
                    PieceField { label: t::library_field_composer(), field: piece.composer.clone(), disabled: done, onsave: set(|o, v| o.composer = Some(v)) }
                    PieceField { label: t::library_field_arranger(), field: piece.arranger.clone(), disabled: done, onsave: set(|o, v| o.arranger = Some(v)) }
                    PieceField { label: t::library_field_genre(), field: piece.genre.clone(), disabled: done, onsave: set(|o, v| o.genre = Some(v)) }
                }
                div { class: "piece-actions",
                    if done {
                        span { class: "ok small", {t::import_piece_done()} }
                        if let Some(s) = &piece.score {
                            Link { class: "button", to: Route::EditScore { id: s.id }, {t::import_open_piece()} }
                        }
                    } else {
                        button { class: "primary", disabled: !piece.can_commit(), title: piece.blocking_note().map(|n| note_text(&n)).unwrap_or_default(),
                            onclick: move |_| oncommit.call(id), {t::import_commit_piece()} }
                    }
                }
            }
            if let (Some(s), false) = (&piece.suggestion, done) {
                {
                    let sid = s.id;
                    let label = s.label();
                    let chosen_existing = piece.target == Some(Target::Existing(sid));
                    let chosen_new = piece.target == Some(Target::New);
                    rsx! {
                        div { class: if piece.target.is_none() { "match lvl-yellow" } else { "match" },
                            span { {t::import_match(&label)} }
                            label { input { r#type: "radio", name: "target-{id}", checked: chosen_existing,
                                onchange: move |_| target(Target::Existing(sid)) } {t::import_match_add()} }
                            label { input { r#type: "radio", name: "target-{id}", checked: chosen_new,
                                onchange: move |_| target(Target::New) } {t::import_match_new()} }
                        }
                    }
                }
            }
            for n in piece.notes.iter().filter(|n| !matches!(n, Note::ConfirmTarget(_))) {
                p { class: "note small", {note_text(n)} }
            }
            table { class: "import-parts",
                thead { tr { th { {t::import_col_page()} } th { {t::import_col_file()} } th { {t::admin_instruments_title()} } th { {t::library_field_key()} } th { {t::library_field_voices()} } th { {t::import_col_name()} } th { {t::import_col_readers()} } th { {t::import_col_import()} } } }
                tbody {
                    for part in piece.parts.iter() {
                        PartRow { key: "{part.id}", part: part.clone(), instruments: instruments.clone(), review, error }
                    }
                }
            }
        }
    }
}

#[component]
fn PartRow(
    part: ReviewPart,
    instruments: Vec<Instrument>,
    review: Signal<Option<Review>>,
    error: Signal<Option<String>>,
) -> Element {
    let id = part.id;
    let done = part.state == ItemState::Committed;
    let change = move |o: PartOverrides| {
        spawn(async move {
            patch(review, error, vec![id], None, Some(o)).await;
        });
    };
    let level = if part.skip {
        "lvl-skip"
    } else if done {
        "lvl-done"
    } else {
        level_class(part.level)
    };
    let thumb = format!("/api/import/files/{}/thumbnail", part.file.id);
    let key = part.key.value.clone().unwrap_or_default();
    let legacy_key = !key.is_empty() && naming::key(&key).is_none();
    // The instruments as the override they become when one is added or
    // removed: detected ones keep their word (a-import-instruments).
    let picks = part.instrument_picks();
    let chosen: Vec<Id> = part.instruments.iter().filter_map(|i| i.instrument_id).collect();
    let addable: Vec<Choice> =
        instrument_choices(&instruments).into_iter().filter(|c| !chosen.contains(&c.id)).collect();
    let path = part.file.path.clone();
    let staves = part.file.staves.map(t::import_staves).unwrap_or_default();
    let pages = part.file.page_count.map(t::import_pages_short).unwrap_or_default();
    rsx! {
        tr { class: "{level}",
            td { class: "thumb",
                if part.file.has_thumbnail {
                    a { href: "/api/import/files/{part.file.id}", target: "_blank", title: t::import_open_pdf(),
                        img { class: "thumb-small", src: "{thumb}", alt: "", loading: "lazy" } }
                } else {
                    div { class: "thumb-small empty", "PDF" } // i18n-ok: file format name
                }
            }
            td { class: "file", "data-label": t::import_col_file(),
                span { class: "mono small", "{part.file.path}" }
                br {}
                span { class: "muted small", "{pages} {format_size(part.file.size_bytes)}{staves}" }
            }
            // Any number of instruments: chips with ×, plus the picker of the
            // part editor (a-assign-picker); an unknown word stays red until
            // it is mapped under "Instruments to confirm".
            td { class: "instruments", "data-label": t::admin_instruments_title(),
                div { class: "ichips", role: "group", "aria-label": t::import_instruments_of(&path),
                    for (i, ins) in part.instruments.iter().enumerate() {
                        {
                            let name = ins.name.clone().unwrap_or_else(|| format!("? {}", ins.text));
                            let gone = ins.pick();
                            let rest: Vec<InstrumentPick> = picks.iter().filter(|p| Some(*p) != gone.as_ref()).cloned().collect();
                            rsx! {
                                span { key: "{i}", class: "ichip {level_class(ins.level)}",
                                    title: "{source_text(ins.source)} {ins.note.as_ref().map(|n| note_text(n)).unwrap_or_default()}",
                                    span { "{name}" }
                                    if !done {
                                        button { r#type: "button", class: "ichip-remove",
                                            title: t::common_remove_entry(&name), "aria-label": t::import_remove_instrument(&name),
                                            onclick: move |_| change(PartOverrides { instruments: Some(rest.clone()), ..Default::default() }),
                                            "×"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if !done {
                    Picker { options: addable, label: t::import_add_instrument(), placeholder: t::common_search_instruments(),
                        none_left: t::import_all_instruments_chosen(),
                        onapply: {
                            let picks = picks.clone();
                            move |ids: Vec<i64>| {
                                let mut next = picks.clone();
                                next.extend(ids.into_iter().map(InstrumentPick::Id));
                                change(PartOverrides { instruments: Some(next), ..Default::default() });
                            }
                        } }
                }
            }
            td { class: "key {level_class(part.key.level)}", "data-label": t::library_field_key(), title: "{hint(&part.key)}",
                select { disabled: done, "aria-label": t::import_key_of(&part.file.path),
                    onchange: move |e: FormEvent| change(PartOverrides { key: Some(e.value()), ..Default::default() }),
                    option { value: "", selected: key.is_empty(), "—" }
                    for k in naming::KEYS.iter() {
                        option { key: "{k.code}", value: "{k.code}", selected: key == k.code, {crate::names::key_option(k.code)} }
                    }
                    if legacy_key { option { value: "{key}", selected: true, "{key}" } }
                }
            }
            // The voices as chips, the input of the part editor (a-import-voices).
            td { class: "voices {level_class(part.voices.level)}", "data-label": t::library_field_voices(),
                title: "{source_hint(part.voices.source, part.voices.note.as_ref())}",
                if done {
                    {part.voices.values.join(&musicus_i18n::t::part_name_voice_sep())}
                } else {
                    VoicesCell { id, path: path.clone(), values: part.voices.values.clone(), review, error }
                }
            }
            td { class: "name", "data-label": t::import_col_name(),
                strong { {part_label(&part)} }
                for n in part.notes.iter().filter(|n| !matches!(n, Note::NoReaders)) {
                    div { class: "note small", {note_text(n)} }
                }
            }
            td { class: "small readers", "data-label": t::import_col_readers(),
                if part.readers.is_empty() {
                    span { class: "warn-text", {t::import_readers_none()} }
                } else {
                    {crate::pages::import::sorted_join(&part.readers)}
                }
            }
            td { class: "take", "data-label": t::import_col_import(),
                if done {
                    span { class: "ok", "✓" }
                } else {
                    input { r#type: "checkbox", checked: !part.skip, "aria-label": t::import_take(&part.file.path),
                        onchange: move |e: FormEvent| change(PartOverrides { skip: Some(!e.checked()), ..Default::default() }) }
                }
            }
        }
    }
}

/// The voices of a part as chips (`TagInput` with "&" as separator, like
/// the part editor, a-import-voices). Every change is stored at once; the
/// server's cleaned list replaces the chips when it differs, and a refused
/// change (more than `naming::MAX_VOICES`) puts the stored list back.
#[component]
fn VoicesCell(
    id: Id,
    path: String,
    values: Vec<String>,
    review: Signal<Option<Review>>,
    error: Signal<Option<String>>,
) -> Element {
    let mut voices = use_signal(|| values.clone());
    let mut stored = use_signal(|| values.clone());
    // New values from the server (own change, other tab, reload): take them.
    use_effect(use_reactive!(|values| {
        if *stored.peek() != values {
            stored.set(values.clone());
            voices.set(values);
        }
    }));
    use_effect(move || {
        let now = voices();
        if now == *stored.peek() {
            return;
        }
        spawn(async move {
            let o = PartOverrides { voices: Some(now), ..Default::default() };
            if !patch(review, error, vec![id], None, Some(o)).await {
                voices.set(stored());
            }
        });
    });
    let label_id = use_hook(|| format!("voices-of-{}", crate::picker::unique_id()));
    rsx! {
        span { id: "{label_id}", class: "visually-hidden", {t::import_voices_of(&path)} }
        TagInput { tags: voices, labelledby: label_id.clone(), placeholder: "—", entries: crate::tag_input::Entries::Voices,
            separators: ",&", maxlength: naming::MAX_VOICE_CHARS as u32 }
    }
}
