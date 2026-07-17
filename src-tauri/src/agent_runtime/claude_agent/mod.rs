mod adapter;
mod client;
mod sidecar_protocol;

pub use adapter::{ClaudeAdapterEvent, ClaudeAdapterEventSink, ClaudeAgentRuntimeAdapter};
pub use client::SidecarLaunchConfig;

#[cfg(test)]
mod tests;
