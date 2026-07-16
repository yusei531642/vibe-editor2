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
