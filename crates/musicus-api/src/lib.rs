//! Types shared by the Musicus server and its clients.
//!
//! - [`dto`]: the JSON bodies of the REST API under `/api`.
//! - [`ink`]: the annotation model and its compact binary encoding, which
//!   clients send and receive as `application/octet-stream`.
//! - [`layout`]: music systems of sheet files and the system view of the reader.
//! - [`import`]: mass import of a sheet archive: file name patterns, part
//!   names, grouping and review colours (pure, shared by server and client).
//! - [`naming`]: part names derived from instruments, key and voice; the key list.
//! - [`pixels`]: whole device pixels for the reader's canvases (sharp rendering).

pub mod dto;
pub mod import;
pub mod ink;
pub mod layout;
pub mod naming;
pub mod pixels;

pub use dto::*;
