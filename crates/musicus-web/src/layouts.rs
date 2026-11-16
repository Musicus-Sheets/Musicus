//! System layouts of sheet files (`GET /api/files/{sha256}/layout`), kept in
//! memory for the tab's lifetime like the sheets themselves (nothing is
//! stored on the device, R-SEC-001). A programme fetches the layouts of all
//! its sheets ahead, so the system view works without network on stage.

use crate::api;
use musicus_api::layout::{FileLayout, LayoutState};
use std::{cell::RefCell, collections::HashMap};

thread_local! {
    static CACHE: RefCell<HashMap<String, FileLayout>> = RefCell::new(HashMap::new());
}

/// The layout of a file; finished layouts come from memory after the first call.
pub async fn load(sha256: &str) -> api::ApiResult<FileLayout> {
    if let Some(l) = CACHE.with(|c| c.borrow().get(sha256).cloned()) {
        return Ok(l);
    }
    let layout: FileLayout = api::get(&format!("/api/files/{sha256}/layout")).await?;
    if layout.state != LayoutState::Pending {
        CACHE.with(|c| c.borrow_mut().insert(sha256.to_string(), layout.clone()));
    }
    Ok(layout)
}

/// Keeps only the layouts of these files.
pub fn retain_only(shas: &[String]) {
    CACHE.with(|c| c.borrow_mut().retain(|k, _| shas.contains(k)));
}
