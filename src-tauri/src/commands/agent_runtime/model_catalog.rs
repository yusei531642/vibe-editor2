use super::*;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelOption {
    pub id: String,
    pub label: String,
    pub description: String,
    pub is_default: bool,
    pub default_effort: String,
    pub supported_efforts: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeModelCatalog {
    pub engine: String,
    pub models: Vec<RuntimeModelOption>,
}

#[tauri::command]
pub async fn agent_runtime_model_catalog(engine: String) -> CommandResult<RuntimeModelCatalog> {
    match engine.as_str() {
        "claude" => Ok(RuntimeModelCatalog {
            engine,
            models: claude_model_catalog(),
        }),
        "codex" => codex_catalog(engine).await,
        _ => Err(CommandError::validation("engine must be claude or codex")),
    }
}

#[cfg(not(unix))]
async fn codex_catalog(_engine: String) -> CommandResult<RuntimeModelCatalog> {
    Err(CommandError::coded(
        "runtime_native_unsupported",
        "Codex model catalog is only available on Unix",
    ))
}

#[cfg(unix)]
async fn codex_catalog(engine: String) -> CommandResult<RuntimeModelCatalog> {
    let codex_command = crate::commands::settings::settings_load()
        .await
        .map(|settings| settings.codex_command)
        .unwrap_or_else(|_| "codex".to_string());
    let value = crate::agent_runtime::codex::model_catalog(codex_command)
        .await
        .map_err(|error| CommandError::coded(error.code, error.message))?;
    Ok(RuntimeModelCatalog {
        engine,
        models: parse_codex_model_catalog(&value),
    })
}

fn claude_model_catalog() -> Vec<RuntimeModelOption> {
    [
        (
            "fable",
            "Fable",
            "Claude Fable — coding and agentic work",
            true,
            true,
        ),
        ("opus", "Opus", "Claude Opus — deep reasoning", false, true),
        (
            "sonnet",
            "Sonnet",
            "Claude Sonnet — balanced speed and quality",
            false,
            true,
        ),
        (
            "haiku",
            "Haiku",
            "Claude Haiku — fastest responses",
            false,
            false,
        ),
    ]
    .into_iter()
    .map(
        |(id, label, description, is_default, extended)| RuntimeModelOption {
            id: id.to_string(),
            label: label.to_string(),
            description: description.to_string(),
            is_default,
            default_effort: "high".to_string(),
            supported_efforts: effort_levels(extended),
        },
    )
    .collect()
}

fn effort_levels(extended: bool) -> Vec<String> {
    if extended {
        ["low", "medium", "high", "xhigh", "max"]
            .into_iter()
            .map(str::to_string)
            .collect()
    } else {
        ["low", "medium", "high"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }
}

#[cfg(unix)]
pub(super) fn parse_codex_model_catalog(value: &serde_json::Value) -> Vec<RuntimeModelOption> {
    value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| {
            !entry
                .get("hidden")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(parse_codex_model)
        .collect()
}

#[cfg(unix)]
fn parse_codex_model(entry: &serde_json::Value) -> Option<RuntimeModelOption> {
    let id = entry.get("model")?.as_str()?.to_string();
    let supported_efforts = entry
        .get("supportedReasoningEfforts")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|option| option.get("reasoningEffort")?.as_str().map(str::to_string))
        .collect();
    Some(RuntimeModelOption {
        label: entry
            .get("displayName")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&id)
            .to_string(),
        id,
        description: entry
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        is_default: entry
            .get("isDefault")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        default_effort: entry
            .get("defaultReasoningEffort")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("high")
            .to_string(),
        supported_efforts,
    })
}
