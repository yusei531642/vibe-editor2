mod adapter;
mod client;
mod sidecar_protocol;

pub use adapter::{
    ClaudeAdapterEvent, ClaudeAdapterEventSink, ClaudeAgentRuntimeAdapter, ClaudeAgentRuntimeConfig,
};
pub use client::SidecarLaunchConfig;

#[cfg(test)]
mod tests;
