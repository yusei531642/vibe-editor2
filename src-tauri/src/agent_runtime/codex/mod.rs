//! Phase 2 Codex app-server native runtime adapter (Unix only).

mod adapter;
mod client;
mod convert;

pub use adapter::{CodexAdapterEvent, CodexAdapterEventSink, CodexRuntimeAdapter};

#[cfg(test)]
mod tests;
