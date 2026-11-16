//! Types shared by the Musicus server and its clients.
//!
//! - [`dto`]: the JSON bodies of the REST API under `/api`.
//! - [`ink`]: the annotation model and its compact binary encoding, which
//!   clients send and receive as `application/octet-stream`.
//! - [`layout`]: music systems of sheet files and the system view of the reader.
//! - [`naming`]: part names derived from instruments, key and voice; the key list.

pub mod dto;
pub mod ink;
pub mod layout;
pub mod naming;

pub use dto::*;
