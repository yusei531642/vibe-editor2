//! Recruit role + Rust settings を concrete runtime provider policy へ投影する。

use crate::agent_runtime::{
    select_provider, BackendKind, ProviderAvailability, ProviderSelection,
    SystemProviderAvailability,
};
use crate::commands::settings::AgentConfig;

fn select_recruit_provider_with<A: ProviderAvailability>(
    custom: Option<&AgentConfig>,
    resolved_engine: &str,
    backend: BackendKind,
    availability: &A,
) -> ProviderSelection {
    if custom.is_some_and(|agent| !agent.command.trim().is_empty()) {
        return select_provider(resolved_engine, false, BackendKind::Pty, availability);
    }
    let is_api_runtime = custom.is_some_and(|agent| agent.runtime == "api");
    // Issue #518 validates resolved_engine before this policy runs. Do not replace it with
    // a settings-side engine value after that authorization boundary.
    select_provider(resolved_engine, is_api_runtime, backend, availability)
}

pub(crate) async fn select_recruit_provider(
    role_profile_id: &str,
    resolved_engine: &str,
) -> ProviderSelection {
    let settings = crate::commands::settings::settings_load()
        .await
        .unwrap_or_default();
    let custom_id = role_profile_id.strip_prefix("custom:");
    let custom = custom_id.and_then(|id| {
        settings
            .custom_agents
            .as_ref()
            .and_then(|agents| agents.iter().find(|agent| agent.id == id))
    });
    let backend =
        BackendKind::try_from(settings.agent_runtime_backend.as_str()).unwrap_or(BackendKind::Pty);
    let availability = SystemProviderAvailability::new(settings.claude_command);
    select_recruit_provider_with(custom, resolved_engine, backend, &availability)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::RuntimeProvider;
    use std::collections::HashMap;

    struct Available;

    impl ProviderAvailability for Available {
        fn is_available(&self, _provider: RuntimeProvider) -> bool {
            true
        }
    }

    fn custom_agent(runtime: &str, command: &str, engine: Option<&str>) -> AgentConfig {
        AgentConfig {
            id: "custom-agent".to_string(),
            name: "Custom agent".to_string(),
            runtime: runtime.to_string(),
            command: command.to_string(),
            args: String::new(),
            cwd: None,
            color: None,
            provider_id: None,
            custom_base_url: None,
            model: None,
            temperature: None,
            max_output_tokens: None,
            system_prompt: None,
            skill_ids: None,
            tool_mode: None,
            engine: engine.map(str::to_string),
            env: None::<HashMap<String, String>>,
            icon: None,
            tags: None,
            default_skill_ids: None,
            skill_injection: None,
        }
    }

    #[test]
    fn custom_command_forces_pty_even_when_native_is_available() {
        let custom = custom_agent("cli", "my-claude-wrapper", Some("claude"));

        let selected = select_recruit_provider_with(
            Some(&custom),
            "claude",
            BackendKind::Native,
            &Available,
        );

        assert_eq!(selected.provider, RuntimeProvider::Pty);
        assert_eq!(selected.fallback_from, None);
    }

    #[test]
    fn custom_command_takes_precedence_over_api_runtime_marker() {
        let custom = custom_agent("api", "custom-cli", Some("codex"));

        let selected = select_recruit_provider_with(
            Some(&custom),
            "codex",
            BackendKind::Native,
            &Available,
        );

        assert_eq!(selected.provider, RuntimeProvider::Pty);
    }

    #[test]
    fn policy_validated_resolved_engine_is_not_replaced_by_custom_engine() {
        let custom = custom_agent("cli", "", Some("codex"));

        let selected = select_recruit_provider_with(
            Some(&custom),
            "claude",
            BackendKind::Native,
            &Available,
        );

        assert_eq!(selected.provider, RuntimeProvider::ClaudeNative);
    }

    #[test]
    fn api_runtime_without_command_still_selects_api_provider() {
        let custom = custom_agent("api", "", Some("claude"));

        let selected = select_recruit_provider_with(
            Some(&custom),
            "claude",
            BackendKind::Native,
            &Available,
        );

        assert_eq!(selected.provider, RuntimeProvider::Api);
    }
}
