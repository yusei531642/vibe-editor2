use super::*;
use crate::pty::session::test_support::test_handle;
use crate::pty::SessionRegistry;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct FakeAdapter {
    spawn_error: Option<RuntimeAdapterError>,
    write_error: Arc<Mutex<Option<RuntimeAdapterError>>>,
    capabilities: Vec<RuntimeCapability>,
}

impl FakeAdapter {
    fn healthy() -> Self {
        Self {
            spawn_error: None,
            write_error: Arc::new(Mutex::new(None)),
            capabilities: vec![RuntimeCapability::StructuredEventStream],
        }
    }

    fn cooperative() -> Self {
        Self {
            capabilities: vec![
                RuntimeCapability::StructuredEventStream,
                RuntimeCapability::CooperativeCancellation,
            ],
            ..Self::healthy()
        }
    }

    fn failing_spawn() -> Self {
        Self {
            spawn_error: Some(RuntimeAdapterError::new(
                "fake_spawn_failed",
                "fake spawn failed",
                false,
            )),
            write_error: Arc::new(Mutex::new(None)),
            capabilities: vec![RuntimeCapability::StructuredEventStream],
        }
    }

    fn crash_on_write(&self) {
        *self.write_error.lock().unwrap() = Some(RuntimeAdapterError::new(
            "fake_mid_session_crash",
            "fake runtime crashed",
            false,
        ));
    }
}

impl AgentRuntimeAdapter for FakeAdapter {
    fn backend_kind(&self) -> BackendKind {
        BackendKind::Native
    }

    fn capabilities(&self) -> Vec<RuntimeCapability> {
        self.capabilities.clone()
    }

    fn spawn_session(
        &self,
        _request: &RuntimeSessionSpawnRequest,
    ) -> Result<(), RuntimeAdapterError> {
        self.spawn_error.clone().map_or(Ok(()), Err)
    }

    fn spawn_turn(&self, _request: &RuntimeTurnSpawnRequest) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }

    fn write(&self, _data: &str) -> Result<(), RuntimeAdapterError> {
        self.write_error.lock().unwrap().clone().map_or(Ok(()), Err)
    }

    fn stop(&self) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }

    fn dispose(&self) -> Result<(), RuntimeAdapterError> {
        Ok(())
    }
}

fn lifecycle_states(events: &[RuntimeEventEnvelope]) -> Vec<RuntimeLifecycleState> {
    events
        .iter()
        .filter_map(|event| match event.payload {
            RuntimeEventPayload::Lifecycle { state, .. } => Some(state),
            _ => None,
        })
        .collect()
}

#[test]
fn fake_adapter_covers_ready_and_stop_lifecycle() {
    let manager = RuntimeManager::new();
    let registered =
        manager.register_endpoint("endpoint-1".to_string(), Arc::new(FakeAdapter::healthy()));
    assert!(registered.result.is_ok());
    assert_eq!(
        lifecycle_states(&registered.events),
        vec![
            RuntimeLifecycleState::Spawning,
            RuntimeLifecycleState::Ready
        ]
    );
    assert_eq!(registered.events[0].sequence, 1);
    assert_eq!(registered.events[1].sequence, 2);

    let stopped = manager.stop("endpoint-1");
    assert!(stopped.result.is_ok());
    assert_eq!(
        lifecycle_states(&stopped.events),
        vec![RuntimeLifecycleState::Exited]
    );
    assert_eq!(stopped.events[0].sequence, 3);
    assert!(manager.registry().is_empty());

    let missing = manager.write("endpoint-1", "after stop");
    assert_eq!(
        missing.result.unwrap_err().code,
        "runtime_endpoint_not_found"
    );
}

#[test]
fn cooperative_stop_keeps_endpoint_ready_and_attached() {
    let manager = RuntimeManager::new();
    assert!(manager
        .register_endpoint(
            "endpoint-cooperative".to_string(),
            Arc::new(FakeAdapter::cooperative())
        )
        .result
        .is_ok());

    let stopped = manager.stop("endpoint-cooperative");
    assert!(stopped.result.is_ok());
    assert!(lifecycle_states(&stopped.events).is_empty());
    assert!(stopped
        .events
        .iter()
        .any(|event| matches!(event.payload, RuntimeEventPayload::Diagnostic { .. })));
    assert!(manager.registry().resolve("endpoint-cooperative").is_some());
    assert!(manager
        .write("endpoint-cooperative", "next turn")
        .result
        .is_ok());
    assert!(manager.dispose("endpoint-cooperative").result.is_ok());
}

#[test]
fn bounded_buffer_coalesces_deltas_and_evicts_old_delta_first() {
    let mut buffer = RuntimeEventBuffer::new(2);
    buffer.push(RuntimeEventEnvelope::new(
        "endpoint-1".to_string(),
        1,
        1,
        RuntimeEventPayload::MessageDelta {
            delta: "hel".to_string(),
        },
    ));
    buffer.push(RuntimeEventEnvelope::new(
        "endpoint-1".to_string(),
        1,
        2,
        RuntimeEventPayload::MessageDelta {
            delta: "lo".to_string(),
        },
    ));
    let coalesced = buffer.snapshot();
    assert_eq!(coalesced.len(), 1);
    assert!(matches!(
        &coalesced[0].payload,
        RuntimeEventPayload::MessageDelta { delta } if delta == "hello"
    ));
    assert_eq!(coalesced[0].sequence, 2);
    buffer.push(RuntimeEventEnvelope::new(
        "endpoint-1".to_string(),
        1,
        3,
        RuntimeEventPayload::Lifecycle {
            state: RuntimeLifecycleState::Ready,
            detail: None,
        },
    ));
    buffer.push(RuntimeEventEnvelope::new(
        "endpoint-1".to_string(),
        1,
        4,
        RuntimeEventPayload::Diagnostic {
            message: "kept".to_string(),
        },
    ));

    assert_eq!(buffer.len(), 2);
    assert_eq!(buffer.dropped_count(), 1);
    assert!(buffer
        .snapshot()
        .iter()
        .all(|event| !matches!(event.payload, RuntimeEventPayload::MessageDelta { .. })));
}

#[test]
fn spawn_failure_emits_error_and_failed_lifecycle_and_unregisters() {
    let manager = RuntimeManager::new();
    let operation = manager.register_endpoint(
        "endpoint-fail".to_string(),
        Arc::new(FakeAdapter::failing_spawn()),
    );

    assert_eq!(operation.result.unwrap_err().code, "fake_spawn_failed");
    assert_eq!(
        lifecycle_states(&operation.events),
        vec![
            RuntimeLifecycleState::Spawning,
            RuntimeLifecycleState::Failed
        ]
    );
    assert!(operation.events.iter().any(|event| matches!(
        event.payload,
        RuntimeEventPayload::Error { ref code, .. } if code == "fake_spawn_failed"
    )));
    assert!(manager.registry().is_empty());
}

#[test]
fn mid_session_crash_emits_error_then_failed_lifecycle() {
    let manager = RuntimeManager::new();
    let adapter = FakeAdapter::healthy();
    let handle = adapter.clone();
    assert!(manager
        .register_endpoint("endpoint-crash".to_string(), Arc::new(adapter))
        .result
        .is_ok());
    handle.crash_on_write();

    let operation = manager.deliver_to_endpoint("endpoint-crash", "message");
    assert_eq!(operation.result.unwrap_err().code, "fake_mid_session_crash");
    assert!(matches!(
        operation.events[0].payload,
        RuntimeEventPayload::Error { .. }
    ));
    assert_eq!(
        lifecycle_states(&operation.events),
        vec![RuntimeLifecycleState::Failed]
    );
    assert!(operation.events[0].sequence < operation.events[1].sequence);
    assert!(manager.registry().is_empty());

    let after_crash = manager.deliver_to_endpoint("endpoint-crash", "retry");
    assert_eq!(
        after_crash.result.unwrap_err().code,
        "runtime_endpoint_not_found"
    );
}

#[test]
fn dispose_clears_sequence_before_same_endpoint_is_registered_again() {
    let manager = RuntimeManager::new();
    assert!(manager
        .register_endpoint(
            "endpoint-reuse".to_string(),
            Arc::new(FakeAdapter::healthy())
        )
        .result
        .is_ok());
    let disposed = manager.dispose("endpoint-reuse");
    assert!(disposed.result.is_ok());
    assert_eq!(disposed.events[0].sequence, 3);

    let registered = manager.register_endpoint(
        "endpoint-reuse".to_string(),
        Arc::new(FakeAdapter::healthy()),
    );
    assert!(registered.result.is_ok());
    assert_eq!(registered.events[0].sequence, 1);
    assert_eq!(registered.events[1].sequence, 2);
}

#[test]
fn manager_drop_detaches_pty_endpoint_without_killing_terminal_owned_session() {
    let registry = Arc::new(SessionRegistry::new());
    let kills = Arc::new(AtomicUsize::new(0));
    assert!(registry
        .insert_if_absent("terminal-owned".to_string(), test_handle(kills.clone()))
        .is_ok());

    {
        let manager = RuntimeManager::new();
        let adapter = Arc::new(PtyCompatAdapter::new(registry.clone(), "terminal-owned"));
        assert!(manager
            .register_endpoint("endpoint-pty".to_string(), adapter)
            .result
            .is_ok());
    }

    assert!(registry.get("terminal-owned").is_some());
    assert_eq!(kills.load(Ordering::SeqCst), 0);
    registry.remove("terminal-owned");
}

#[test]
fn unknown_endpoint_operations_do_not_accumulate_sequence_counters() {
    let manager = RuntimeManager::new();

    // renderer が任意の endpointId を連打しても sequence counter は永続化されない。
    for attempt in 0..3 {
        let operation = manager.write(&format!("ghost-{attempt}"), "data");
        assert!(operation.result.is_err());
        assert_eq!(operation.events.len(), 1);
        // transient 経路は常に sequence=1 (counter 非採番) で emit する。
        assert_eq!(operation.events[0].sequence, 1);
    }
    let repeated = manager.write("ghost-0", "data");
    assert_eq!(repeated.events[0].sequence, 1);
}
