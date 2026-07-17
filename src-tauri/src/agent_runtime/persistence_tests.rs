use super::*;
use crate::agent_runtime::{RuntimeEventPayload, RuntimeLifecycleState};

fn event(epoch: u64, sequence: u64, state: RuntimeLifecycleState) -> RuntimeEventEnvelope {
    let mut event = RuntimeEventEnvelope::new(
        "native-agent-1".to_string(),
        epoch,
        sequence,
        RuntimeEventPayload::Lifecycle {
            state,
            detail: None,
        },
    );
    event.timestamp = format!("2026-07-17T00:00:0{sequence}Z");
    event
}

#[test]
fn persists_epoch_sequence_order_and_latest_binding() {
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeEventPersistence::start(temp.path().join("events.db")).unwrap();
    store.append(event(41, 1, RuntimeLifecycleState::Spawning));
    store.append(event(41, 2, RuntimeLifecycleState::Ready));
    store.bind(PersistedRuntimeBinding {
        project_root: "/workspace/one".to_string(),
        team_id: "team-1".to_string(),
        agent_id: "agent-1".to_string(),
        endpoint_id: "native-agent-1".to_string(),
        epoch: 41,
        provider: "codex-native".to_string(),
        resume_id: Some("thread-1".to_string()),
        resumable: true,
    });
    store.append(event(42, 1, RuntimeLifecycleState::Spawning));
    store.append(event(42, 3, RuntimeLifecycleState::Ready));
    store.bind(PersistedRuntimeBinding {
        project_root: "/workspace/one".to_string(),
        team_id: "team-1".to_string(),
        agent_id: "agent-1".to_string(),
        endpoint_id: "native-agent-1".to_string(),
        epoch: 42,
        provider: "codex-native".to_string(),
        resume_id: Some("thread-2".to_string()),
        resumable: true,
    });

    let restored = store.restore_latest("/workspace/one").unwrap();
    assert_eq!(restored.team_id.as_deref(), Some("team-1"));
    assert_eq!(
        restored.events,
        vec![
            event(41, 1, RuntimeLifecycleState::Spawning),
            event(41, 2, RuntimeLifecycleState::Ready),
            event(42, 1, RuntimeLifecycleState::Spawning),
            // Snapshot coalescing can leave visible sequence gaps; replay preserves them.
            event(42, 3, RuntimeLifecycleState::Ready),
        ]
    );
    assert_eq!(restored.bindings[0].epoch, 42);
    assert_eq!(restored.bindings[0].resume_id.as_deref(), Some("thread-2"));
}

#[test]
fn corrupt_database_is_quarantined_and_recreated() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.db");
    std::fs::write(&path, b"not sqlite").unwrap();
    let store = RuntimeEventPersistence::start(path.clone()).unwrap();
    assert!(store.restore_latest("/workspace/one").unwrap().events.is_empty());
    assert!(temp.path().read_dir().unwrap().flatten().any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with("events.db.corrupt-")
    }));
}

#[test]
fn malformed_envelope_row_is_quarantined_and_recreated() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("events.db");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE schema_version(version INTEGER NOT NULL);
             INSERT INTO schema_version VALUES (2);
             CREATE TABLE runtime_events(
               id INTEGER PRIMARY KEY AUTOINCREMENT,
               endpoint_id TEXT NOT NULL, epoch INTEGER NOT NULL, sequence INTEGER NOT NULL,
               kind TEXT NOT NULL, envelope_json TEXT NOT NULL, timestamp TEXT NOT NULL,
               team_id TEXT, UNIQUE(endpoint_id, epoch, sequence)
             );
             INSERT INTO runtime_events
               (endpoint_id,epoch,sequence,kind,envelope_json,timestamp)
               VALUES ('native-agent-1',1,1,'lifecycle','{invalid','2026-07-17T00:00:00Z');",
        )
        .unwrap();
    drop(connection);

    let store = RuntimeEventPersistence::start(path).unwrap();
    assert!(store.restore_latest("/workspace/one").unwrap().events.is_empty());
    assert!(temp.path().read_dir().unwrap().flatten().any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with("events.db.corrupt-")
    }));
}

#[test]
fn restore_is_scoped_to_authorized_project_root() {
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeEventPersistence::start(temp.path().join("events.db")).unwrap();

    store.append(event(51, 1, RuntimeLifecycleState::Ready));
    store.bind(PersistedRuntimeBinding {
        project_root: "/workspace/one".to_string(),
        team_id: "team-one".to_string(),
        agent_id: "agent-1".to_string(),
        endpoint_id: "native-agent-1".to_string(),
        epoch: 51,
        provider: "codex-native".to_string(),
        resume_id: Some("thread-one".to_string()),
        resumable: true,
    });

    assert_eq!(
        store
            .restore_latest("/workspace/one")
            .unwrap()
            .team_id
            .as_deref(),
        Some("team-one")
    );
    let foreign = store.restore_latest("/workspace/two").unwrap();
    assert!(foreign.team_id.is_none());
    assert!(foreign.bindings.is_empty());
    assert!(foreign.events.is_empty());
}
