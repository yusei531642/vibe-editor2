//! Engine / settings から concrete runtime provider を選ぶ Phase 7 policy。

use super::{BackendKind, RuntimeCapability};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeProvider {
    CodexNative,
    ClaudeNative,
    Api,
    Pty,
}

impl RuntimeProvider {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CodexNative => "codex-native",
            Self::ClaudeNative => "claude-native",
            Self::Api => "api",
            Self::Pty => "pty",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderSelectionReason {
    ApiRuntime,
    ExplicitPty,
    NativeAvailable,
    NativeUnavailable,
    UnsupportedEngine,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSelection {
    pub provider: RuntimeProvider,
    pub fallback_from: Option<RuntimeProvider>,
    pub reason: ProviderSelectionReason,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProviderDeclaration {
    pub provider: RuntimeProvider,
    pub available: bool,
    pub capabilities: Vec<RuntimeCapability>,
}

pub trait ProviderAvailability {
    fn is_available(&self, provider: RuntimeProvider) -> bool;
}

pub fn capabilities_for(provider: RuntimeProvider) -> Vec<RuntimeCapability> {
    match provider {
        RuntimeProvider::CodexNative | RuntimeProvider::ClaudeNative => vec![
            RuntimeCapability::NativeProcessExecution,
            RuntimeCapability::StructuredEventStream,
            RuntimeCapability::CooperativeCancellation,
            RuntimeCapability::SessionResume,
            RuntimeCapability::SessionFork,
            RuntimeCapability::TurnSteering,
            RuntimeCapability::ApprovalResponses,
        ],
        RuntimeProvider::Api => vec![
            RuntimeCapability::StructuredEventStream,
            RuntimeCapability::CooperativeCancellation,
        ],
        RuntimeProvider::Pty => vec![RuntimeCapability::PtyExecution],
    }
}

pub fn provider_declarations<A: ProviderAvailability>(
    availability: &A,
) -> Vec<RuntimeProviderDeclaration> {
    [
        RuntimeProvider::CodexNative,
        RuntimeProvider::ClaudeNative,
        RuntimeProvider::Api,
        RuntimeProvider::Pty,
    ]
    .into_iter()
    .map(|provider| RuntimeProviderDeclaration {
        provider,
        available: availability.is_available(provider),
        capabilities: capabilities_for(provider),
    })
    .collect()
}

pub fn select_provider<A: ProviderAvailability>(
    engine: &str,
    is_api_runtime: bool,
    requested_backend: BackendKind,
    availability: &A,
) -> ProviderSelection {
    if is_api_runtime {
        return ProviderSelection {
            provider: RuntimeProvider::Api,
            fallback_from: None,
            reason: ProviderSelectionReason::ApiRuntime,
        };
    }
    if requested_backend == BackendKind::Pty {
        return ProviderSelection {
            provider: RuntimeProvider::Pty,
            fallback_from: None,
            reason: ProviderSelectionReason::ExplicitPty,
        };
    }
    let desired = match engine {
        "claude" => RuntimeProvider::ClaudeNative,
        "codex" => RuntimeProvider::CodexNative,
        _ => {
            return ProviderSelection {
                provider: RuntimeProvider::Pty,
                fallback_from: None,
                reason: ProviderSelectionReason::UnsupportedEngine,
            };
        }
    };
    if availability.is_available(desired) {
        ProviderSelection {
            provider: desired,
            fallback_from: None,
            reason: ProviderSelectionReason::NativeAvailable,
        }
    } else {
        ProviderSelection {
            provider: RuntimeProvider::Pty,
            fallback_from: Some(desired),
            reason: ProviderSelectionReason::NativeUnavailable,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SystemProviderAvailability {
    claude_command: String,
}

impl SystemProviderAvailability {
    pub fn new(claude_command: impl Into<String>) -> Self {
        Self {
            claude_command: claude_command.into(),
        }
    }
}

impl ProviderAvailability for SystemProviderAvailability {
    fn is_available(&self, provider: RuntimeProvider) -> bool {
        match provider {
            RuntimeProvider::CodexNative => cfg!(unix) && which::which("codex").is_ok(),
            RuntimeProvider::ClaudeNative => {
                resolve_node_executable().is_some()
                    && resolve_sidecar_entrypoint().is_some()
                    && resolve_native_claude_command(&self.claude_command).is_some()
            }
            RuntimeProvider::Api | RuntimeProvider::Pty => true,
        }
    }
}

/// Native sidecar may forward credentials only to the default Claude CLI resolved from the
/// host PATH. A settings-supplied wrapper/path stays on the PTY provider.
pub fn resolve_native_claude_command(command: &str) -> Option<PathBuf> {
    if command.trim() != "claude" {
        return None;
    }
    which::which("claude").ok()
}

pub fn resolve_node_executable() -> Option<PathBuf> {
    which::which("node").ok()
}

pub fn resolve_sidecar_entrypoint() -> Option<PathBuf> {
    let dev =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src-sidecars/claude-agent/index.mjs");
    if dev.is_file() {
        return Some(dev);
    }
    let executable = std::env::current_exe().ok()?;
    let base = executable.parent()?;
    [
        base.join("sidecars/claude-agent/dist/index.mjs"),
        base.join("resources/sidecars/claude-agent/dist/index.mjs"),
        base.join("../Resources/sidecars/claude-agent/dist/index.mjs"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    struct Available(HashSet<RuntimeProvider>);

    impl ProviderAvailability for Available {
        fn is_available(&self, provider: RuntimeProvider) -> bool {
            self.0.contains(&provider)
        }
    }

    fn available(providers: &[RuntimeProvider]) -> Available {
        Available(providers.iter().copied().collect())
    }

    #[test]
    fn claude_engine_selects_claude_native_when_available() {
        let selected = select_provider(
            "claude",
            false,
            BackendKind::Auto,
            &available(&[RuntimeProvider::ClaudeNative]),
        );
        assert_eq!(selected.provider, RuntimeProvider::ClaudeNative);
        assert_eq!(selected.fallback_from, None);
    }

    #[test]
    fn unavailable_native_falls_back_explicitly_to_pty() {
        let selected = select_provider("claude", false, BackendKind::Native, &available(&[]));
        assert_eq!(selected.provider, RuntimeProvider::Pty);
        assert_eq!(selected.fallback_from, Some(RuntimeProvider::ClaudeNative));
        assert_eq!(selected.reason, ProviderSelectionReason::NativeUnavailable);
    }

    #[test]
    fn codex_and_claude_can_be_selected_for_the_same_team_policy() {
        let availability =
            available(&[RuntimeProvider::CodexNative, RuntimeProvider::ClaudeNative]);
        let leader = select_provider("codex", false, BackendKind::Auto, &availability);
        let reviewer = select_provider("claude", false, BackendKind::Auto, &availability);
        assert_eq!(leader.provider, RuntimeProvider::CodexNative);
        assert_eq!(reviewer.provider, RuntimeProvider::ClaudeNative);
    }

    #[test]
    fn api_runtime_bypasses_cli_engine_selection() {
        let selected = select_provider("claude", true, BackendKind::Pty, &available(&[]));
        assert_eq!(selected.provider, RuntimeProvider::Api);
        assert_eq!(selected.reason, ProviderSelectionReason::ApiRuntime);
    }

    #[test]
    fn custom_claude_commands_are_not_native_credential_targets() {
        assert!(resolve_native_claude_command("my-claude-wrapper").is_none());
        assert!(resolve_native_claude_command("/tmp/claude").is_none());
        assert!(resolve_native_claude_command("claude --dangerous-flag").is_none());
    }
}
