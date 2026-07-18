use super::*;
use serde_json::json;

#[tokio::test]
async fn diagnostics_uses_camelcase_and_safe_auto_fallback() {
    let result = agent_runtime_diagnostics("auto".to_string()).await.unwrap();
    let value = serde_json::to_value(result).unwrap();

    assert_eq!(value["requestedBackend"], json!("auto"));
    #[cfg(unix)]
    if which::which("codex").is_ok() {
        assert_eq!(value["selectedBackend"], json!("native"));
        assert_eq!(value["reason"], json!("autoNativeCapabilitiesAvailable"));
        assert!(value["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("approvalResponses")));
    } else {
        assert_eq!(value["selectedBackend"], json!("pty"));
        assert_eq!(value["reason"], json!("autoPtyFallback"));
        assert_eq!(value["capabilities"], json!(["ptyExecution"]));
    }
    #[cfg(not(unix))]
    {
        assert_eq!(value["selectedBackend"], json!("pty"));
        assert_eq!(value["reason"], json!("autoPtyFallback"));
        assert_eq!(value["capabilities"], json!(["ptyExecution"]));
    }
}

#[tokio::test]
async fn diagnostics_rejects_unknown_backend() {
    let error = agent_runtime_diagnostics("unknown".to_string())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "validation");
    let serialized = serde_json::to_value(&error).unwrap();
    assert!(serialized["message"]
        .as_str()
        .unwrap()
        .contains("expected auto, native, or pty"));
}

#[test]
#[cfg(unix)]
fn codex_registration_paths_reject_nul_and_oversize_values() {
    assert!(validate_bounded_no_nul("codexCommand", "codex\0evil", 4_096).is_err());
    assert!(validate_bounded_no_nul("socketPath", &"s".repeat(4_097), 4_096).is_err());
    assert!(validate_bounded_no_nul("socketPath", "/tmp/codex.sock", 4_096).is_ok());
}

#[test]
fn approval_request_ids_are_opaque_but_bounded_and_control_free() {
    assert!(validate_approval_request_id("request:900/alpha").is_ok());
    assert!(validate_approval_request_id("").is_err());
    assert!(validate_approval_request_id("request\n900").is_err());
    assert!(validate_approval_request_id(&"r".repeat(257)).is_err());
}

#[test]
fn resume_requires_previously_observed_thread_id() {
    let known = std::sync::Mutex::new(std::collections::HashSet::new());
    assert!(super::authorize_known_thread(&known, "thread-1").is_err());

    super::record_known_thread(&known, Some("thread-1".to_string()));
    assert!(super::authorize_known_thread(&known, "thread-1").is_ok());
    assert!(super::authorize_known_thread(&known, "thread-2").is_err());
}

#[test]
fn runtime_options_are_bounded_and_permissions_are_closed() {
    assert!(validate_runtime_option("model", Some("gpt-5")).is_ok());
    assert!(validate_runtime_option("model", Some("bad\nmodel")).is_err());
    assert!(validate_runtime_option("effort", Some(&"x".repeat(257))).is_err());
    assert!(validate_runtime_permission(Some("workspace")).is_ok());
    assert!(validate_runtime_permission(Some("full")).is_ok());
    assert!(validate_runtime_permission(Some("bypass")).is_err());
}

#[test]
fn team_runtime_permission_is_capped_by_backend_policy() {
    assert_eq!(
        super::registration::effective_runtime_permission(true, Some("full".to_string())),
        Some("workspace".to_string())
    );
    assert_eq!(
        super::registration::effective_runtime_permission(true, None),
        Some("workspace".to_string())
    );
    assert_eq!(
        super::registration::effective_runtime_permission(false, Some("full".to_string())),
        Some("full".to_string())
    );
}

#[test]
#[cfg(unix)]
fn codex_catalog_keeps_advertised_model_and_efforts() {
    let models = parse_codex_model_catalog(&json!({
        "data": [
            {
                "model": "gpt-fixture",
                "displayName": "GPT Fixture",
                "description": "fixture model",
                "isDefault": true,
                "hidden": false,
                "defaultReasoningEffort": "medium",
                "supportedReasoningEfforts": [
                    { "reasoningEffort": "low" },
                    { "reasoningEffort": "high" }
                ]
            },
            { "model": "hidden", "hidden": true }
        ]
    }));
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "gpt-fixture");
    assert_eq!(models[0].default_effort, "medium");
    assert_eq!(models[0].supported_efforts, ["low", "high"]);
}
