//! Typed HTTP client for the `GitConflux` API.
//!
//! Security properties: HTTPS enforced by [`gitconflux_core::ApiUrl`], redirects are never
//! followed (so credentials cannot leak cross-origin), responses are size-bounded, server
//! messages are sanitized, and only idempotent GETs are retried.

mod client;
mod error;
pub mod models;

pub use client::{Client, ClientOptions};
pub use error::{ApiError, sanitize};
