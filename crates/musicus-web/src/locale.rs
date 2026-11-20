//! The UI language and everything that depends on it besides the messages
//! (a-intl-js): chosen once at start from `navigator.languages` (first shipped
//! language, else English; no setting, nothing stored: R-I18N-001,
//! R-I18N-002, R-SEC-001), `<html lang>`, and dates, times and numbers via the
//! browser's `Intl` in that language (R-I18N-005). The only module that knows
//! where the language comes from (a desktop app would replace it).

use std::cell::RefCell;
use wasm_bindgen::{JsCast, JsValue};

thread_local! {
    /// BCP 47 tag for `Intl`: the browser preference that matched ("en-GB").
    static INTL: RefCell<String> = RefCell::new("en".to_owned());
}

/// Chooses the language; call once before the first render.
pub fn init() {
    let window = web_sys::window();
    let prefs: Vec<String> = window
        .as_ref()
        .map(|w| w.navigator().languages().iter().filter_map(|v| v.as_string()).collect())
        .unwrap_or_default();
    let (language, tag) = musicus_i18n::negotiate(&prefs);
    musicus_i18n::set_language(language);
    let intl = if language == musicus_i18n::PSEUDO { "en".to_owned() } else { tag.clone() };
    INTL.with(|i| *i.borrow_mut() = intl);
    if let Some(html) = window.and_then(|w| w.document()).and_then(|d| d.document_element()) {
        let _ = html.set_attribute("lang", &tag);
    }
}

/// The tag `Intl` formats with.
pub fn intl_locale() -> String {
    INTL.with(|i| i.borrow().clone())
}

/// Unix seconds as a local date and time.
pub fn format_time(secs: i64) -> String {
    let date = js_sys::Date::new(&JsValue::from_f64(secs as f64 * 1000.0));
    String::from(date.to_locale_string(&intl_locale(), &JsValue::UNDEFINED))
}

/// `2026-12-24` as a localised date.
pub fn format_date(iso: &str) -> String {
    let date = js_sys::Date::new(&JsValue::from_str(&format!("{iso}T12:00:00")));
    if date.get_time().is_nan() {
        return iso.to_owned();
    }
    String::from(date.to_locale_date_string(&intl_locale(), &JsValue::UNDEFINED))
}

/// A number with exactly `min`..=`max` fraction digits in the UI language.
pub fn format_number(n: f64, min: u32, max: u32) -> String {
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &"minimumFractionDigits".into(), &min.into());
    let _ = js_sys::Reflect::set(&opts, &"maximumFractionDigits".into(), &max.into());
    let locales = js_sys::Array::of1(&intl_locale().into());
    let f = js_sys::Intl::NumberFormat::new(&locales, &opts).format();
    f.unchecked_ref::<js_sys::Function>()
        .call1(&JsValue::NULL, &JsValue::from_f64(n))
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_else(|| n.to_string())
}

/// File size: "1.5 MB" from 1 MB on, else whole kilobytes rounded up ("12 KB").
pub fn format_size(bytes: i64) -> String {
    use musicus_i18n::t;
    if bytes >= 1_000_000 {
        t::common_size_mb(format_number(bytes as f64 / 1e6, 1, 1))
    } else {
        t::common_size_kb(format_number(((bytes + 999) / 1000) as f64, 0, 0))
    }
}
