//! Endpoint registry and runtime operation coordinator.

use super::{
    AgentRuntimeAdapter, PersistedRuntimeBinding, RuntimeAdapterError, RuntimeEndpointRegistry,
    RuntimeApprovalResponseRequest, RuntimeEventBuffer, RuntimeEventEnvelope, RuntimeEventPayload,
    RuntimeEventPersistence, RuntimeLifecycleState, RuntimeRestoreSnapshot,
    RuntimeSessionForkRequest, RuntimeSessionResumeRequest, RuntimeSessionSpawnRequest,
    RuntimeSteerRequest, RuntimeTeamBinding, RuntimeTurnSpawnRequest,
    DEFAULT_RUNTIME_EVENT_BUFFER_CAPACITY,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

fn recover_mutex<'a, T>(
    result: Result<MutexGuard<'a, T>, PoisonError<MutexGuard<'a, T>>>,
) -> MutexGuard<'a, T> {
    result.unwrap_or_else(|poisoned| {
        tracing::warn!("[runtime] mutex poisoned — recovering inner data");
        poisoned.into_inner()
    })
}

pub struct RuntimeOperation {
    pub events: Vec<RuntimeEventEnvelope>,
    pub result: Result<(), RuntimeAdapterError>,
}

pub struct RuntimeManager {
    registry: RuntimeEndpointRegistry,
    sequences: Mutex<HashMap<String, (u64, u64)>>,
    next_epoch: AtomicU64,
    event_buffer: Mutex<RuntimeEventBuffer>,
    persistence: Option<RuntimeEventPersistence>,
}

impl RuntimeManager {
    pub fn new() -> Self {
        Self::with_event_buffer_capacity(DEFAULT_RUNTIME_EVENT_BUFFER_CAPACITY)
    }

    pub fn with_event_buffer_capacity(capacity: usize) -> Self {
        Self {
            registry: RuntimeEndpointRegistry::new(),
            sequences: Mutex::new(HashMap::new()),
            next_epoch: AtomicU64::new(epoch_seed()),
            event_buffer: Mutex::new(RuntimeEventBuffer::new(capacity)),
            persistence: None,
        }
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn persistent_default() -> Self {
        let persistence = match RuntimeEventPersistence::start_default() {
            Ok(persistence) => Some(persistence),
            Err(error) => {
                tracing::warn!("[runtime-persistence] disabled after startup failure: {error:#}");
                None
            }
        };
        Self {
            registry: RuntimeEndpointRegistry::new(),
            sequences: Mutex::new(HashMap::new()),
            next_epoch: AtomicU64::new(epoch_seed()),
            event_buffer: Mutex::new(RuntimeEventBuffer::new(
                DEFAULT_RUNTIME_EVENT_BUFFER_CAPACITY,
            )),
            persistence,
        }
    }

    #[allow(dead_code)]
    pub fn registry(&self) -> &RuntimeEndpointRegistry {
        &self.registry
    }

    pub fn register_endpoint(
        &self,
        endpoint_id: String,
        adapter: Arc<dyn AgentRuntimeAdapter>,
    ) -> RuntimeOperation {
        self.register_with(endpoint_id, adapter, |adapter, endpoint_id| {
            adapter.spawn_session(&RuntimeSessionSpawnRequest {
                endpoint_id: endpoint_id.to_string(),
            })
        })
    }

    pub fn register_resumed_endpoint(
        &self,
        endpoint_id: String,
        adapter: Arc<dyn AgentRuntimeAdapter>,
        thread_id: String,
    ) -> RuntimeOperation {
        self.register_with(endpoint_id, adapter, move |adapter, _| {
            adapter.resume_session(&RuntimeSessionResumeRequest { thread_id })
        })
    }

    pub fn register_forked_endpoint(
        &self,
        endpoint_id: String,
        adapter: Arc<dyn AgentRuntimeAdapter>,
        thread_id: String,
    ) -> RuntimeOperation {
        self.register_with(endpoint_id, adapter, move |adapter, _| {
            adapter.fork_session(&RuntimeSessionForkRequest { thread_id })
        })
    }

    fn register_with<F>(
        &self,
        endpoint_id: String,
        adapter: Arc<dyn AgentRuntimeAdapter>,
        start: F,
    ) -> RuntimeOperation
    where
        F: FnOnce(&Arc<dyn AgentRuntimeAdapter>, &str) -> Result<(), RuntimeAdapterError>,
    {
        if let Err(error) = self.registry.register(endpoint_id.clone(), adapter.clone()) {
            return RuntimeOperation {
                events: Vec::new(),
                result: Err(error),
            };
        }
        let epoch = self.next_epoch.fetch_add(1, Ordering::Relaxed);
        recover_mutex(self.sequences.lock()).insert(endpoint_id.clone(), (epoch, 0));

        // registry insert と spawn_session 完了の間には、別 thread が endpoint を resolve して
        // write を開始できる race window がある。現行 PTY adapter は既存 session を wrap する
        // ため安全だが、将来の native adapter は内部で spawning 状態を拒否/queue すること。

        let mut events = vec![self.record_event(
            &endpoint_id,
            RuntimeEventPayload::Lifecycle {
                state: RuntimeLifecycleState::Spawning,
                detail: Some(format!("{:?}", adapter.backend_kind()).to_lowercase()),
            },
        )];
        match start(&adapter, &endpoint_id) {
            Ok(()) => {
                events.push(self.record_event(
                    &endpoint_id,
                    RuntimeEventPayload::Lifecycle {
                        state: RuntimeLifecycleState::Ready,
                        detail: None,
                    },
                ));
                RuntimeOperation {
                    events,
                    result: Ok(()),
                }
            }
            Err(error) => {
                events.extend(self.failure_events(&endpoint_id, &error));
                self.detach_endpoint(&endpoint_id);
                RuntimeOperation {
                    events,
                    result: Err(error),
                }
            }
        }
    }

    pub fn spawn_turn(
        &self,
        endpoint_id: &str,
        request: RuntimeTurnSpawnRequest,
    ) -> RuntimeOperation {
        self.run_adapter_operation(endpoint_id, |adapter| adapter.spawn_turn(&request))
    }

    pub fn write(&self, endpoint_id: &str, data: &str) -> RuntimeOperation {
        self.run_adapter_operation(endpoint_id, |adapter| adapter.write(data))
    }

    pub fn inject(&self, endpoint_id: &str, data: &str) -> RuntimeOperation {
        self.run_adapter_operation(endpoint_id, |adapter| adapter.inject(data))
    }

    pub fn steer(&self, endpoint_id: &str, input: String) -> RuntimeOperation {
        self.run_adapter_operation(endpoint_id, |adapter| {
            adapter.steer(&RuntimeSteerRequest { input })
        })
    }

    pub fn interrupt(&self, endpoint_id: &str) -> RuntimeOperation {
        self.run_adapter_operation(endpoint_id, |adapter| adapter.interrupt())
    }

    pub fn respond_approval(
        &self,
        endpoint_id: &str,
        request_id: String,
        decision: String,
    ) -> RuntimeOperation {
        self.run_adapter_operation(endpoint_id, |adapter| {
            adapter.respond_approval(&RuntimeApprovalResponseRequest {
                request_id,
                decision,
            })
        })
    }

    /// TeamHub 等の上位層が PTY registry を知らずに endpoint へ配送する API。
    #[allow(dead_code)]
    pub fn deliver_to_endpoint(&self, endpoint_id: &str, data: &str) -> RuntimeOperation {
        self.write(endpoint_id, data)
    }

    /// Runtime の実行を停止する。cooperative cancellation capability を持たない PTY adapter
    /// では `stop` は session process の kill を意味する。成功後は endpoint を自動 detach し、
    /// terminal layer が所有する session lifetime には dispose 経由で干渉しない。
    /// cooperative cancellation capability を持つ native adapter は turn だけを停止し、
    /// session を再利用できるよう endpoint を維持する。
    pub fn stop(&self, endpoint_id: &str) -> RuntimeOperation {
        let cooperative = self.registry.resolve(endpoint_id).is_some_and(|adapter| {
            adapter
                .capabilities()
                .contains(&super::RuntimeCapability::CooperativeCancellation)
        });
        let mut operation = self.run_adapter_operation(endpoint_id, |adapter| adapter.stop());
        if operation.result.is_ok() {
            if cooperative {
                operation.events.push(self.record_event(
                    endpoint_id,
                    RuntimeEventPayload::Diagnostic {
                        message: "turn interrupted; runtime session remains ready".to_string(),
                    },
                ));
            } else {
                operation.events.push(self.record_event(
                    endpoint_id,
                    RuntimeEventPayload::Lifecycle {
                        state: RuntimeLifecycleState::Exited,
                        detail: Some("interrupted".to_string()),
                    },
                ));
                self.detach_endpoint(endpoint_id);
            }
        }
        operation
    }

    pub fn dispose(&self, endpoint_id: &str) -> RuntimeOperation {
        let operation = match self.registry.dispose(endpoint_id) {
            Ok(()) => RuntimeOperation {
                events: vec![self.record_event(
                    endpoint_id,
                    RuntimeEventPayload::Lifecycle {
                        state: RuntimeLifecycleState::Exited,
                        detail: Some("disposed".to_string()),
                    },
                )],
                result: Ok(()),
            },
            Err(error) => RuntimeOperation {
                events: self.failure_events(endpoint_id, &error),
                result: Err(error),
            },
        };
        self.remove_sequence(endpoint_id);
        operation
    }

    fn run_adapter_operation<F>(&self, endpoint_id: &str, operation: F) -> RuntimeOperation
    where
        F: FnOnce(&Arc<dyn AgentRuntimeAdapter>) -> Result<(), RuntimeAdapterError>,
    {
        let Some(adapter) = self.registry.resolve(endpoint_id) else {
            let error = RuntimeAdapterError::new(
                "runtime_endpoint_not_found",
                format!("runtime endpoint '{endpoint_id}' was not found"),
                true,
            );
            // 未登録 endpoint への操作は sequence counter を永続化しない (transient 経路)。
            // renderer 起点で任意の endpointId を連打されても sequences map が成長しないため、
            // resource exhaustion にならない。同一 endpointId への繰り返し失敗は renderer 側で
            // sequence=1 の重複として out-of-order 破棄される (初回のみ表示される契約)。
            return RuntimeOperation {
                events: vec![self.transient_error_event(endpoint_id, &error)],
                result: Err(error),
            };
        };
        match operation(&adapter) {
            Ok(()) => RuntimeOperation {
                events: Vec::new(),
                result: Ok(()),
            },
            Err(error) => {
                let events = self.failure_events(endpoint_id, &error);
                if !error.recoverable {
                    self.detach_endpoint(endpoint_id);
                }
                RuntimeOperation {
                    events,
                    result: Err(error),
                }
            }
        }
    }

    pub(crate) fn detach_endpoint(&self, endpoint_id: &str) {
        if let Some(adapter) = self.registry.remove(endpoint_id) {
            if let Err(error) = adapter.dispose() {
                tracing::warn!(
                    endpoint_id,
                    code = %error.code,
                    "[runtime] detached endpoint adapter dispose failed: {error}"
                );
            }
        }
        self.remove_sequence(endpoint_id);
    }

    /// Native adapter の監視 task から届く非同期 failure を Phase 1 と同じ順序で記録し、
    /// non-recoverable failure なら endpoint を自動 detach する。
    pub fn fail_endpoint(&self, endpoint_id: &str, error: RuntimeAdapterError) -> RuntimeOperation {
        let events = self.failure_events(endpoint_id, &error);
        if !error.recoverable {
            self.detach_endpoint(endpoint_id);
        }
        RuntimeOperation {
            events,
            result: Err(error),
        }
    }

    fn remove_sequence(&self, endpoint_id: &str) {
        recover_mutex(self.sequences.lock()).remove(endpoint_id);
    }

    pub(crate) fn failure_events(
        &self,
        endpoint_id: &str,
        error: &RuntimeAdapterError,
    ) -> Vec<RuntimeEventEnvelope> {
        let mut events = vec![self.record_event(
            endpoint_id,
            RuntimeEventPayload::Error {
                code: error.code.clone(),
                message: error.message.clone(),
                recoverable: error.recoverable,
            },
        )];
        if !error.recoverable {
            events.push(self.record_event(
                endpoint_id,
                RuntimeEventPayload::Lifecycle {
                    state: RuntimeLifecycleState::Failed,
                    detail: Some(error.message.clone()),
                },
            ));
        }
        events
    }

    /// 未登録 endpoint 向けの失敗 event。sequence counter を作らず固定 sequence=1 で構築する。
    pub(crate) fn transient_error_event(
        &self,
        endpoint_id: &str,
        error: &RuntimeAdapterError,
    ) -> RuntimeEventEnvelope {
        let event = RuntimeEventEnvelope::new(
            endpoint_id.to_string(),
            0,
            1,
            RuntimeEventPayload::Error {
                code: error.code.clone(),
                message: error.message.clone(),
                recoverable: error.recoverable,
            },
        );
        recover_mutex(self.event_buffer.lock()).push(event.clone());
        if let Some(persistence) = &self.persistence {
            persistence.append(event.clone());
        }
        event
    }

    /// endpoint 内単調増加の sequence を採番して event を記録する。
    ///
    /// counter は「登録 epoch」単位: detach / dispose / stop 成功で削除され、同一 endpointId の
    /// 再登録では 1 から振り直される。renderer projection は lifecycle `spawning` を新 epoch の
    /// 開始として扱い projection を reset するため、巻き戻った sequence が黙って捨てられることはない。
    pub fn record_event(
        &self,
        endpoint_id: &str,
        payload: RuntimeEventPayload,
    ) -> RuntimeEventEnvelope {
        let event = {
            let mut sequences = recover_mutex(self.sequences.lock());
            let current = sequences
                .entry(endpoint_id.to_string())
                .or_insert_with(|| (self.next_epoch.fetch_add(1, Ordering::Relaxed), 0));
            current.1 = current.1.saturating_add(1);
            let event = RuntimeEventEnvelope::new(
                endpoint_id.to_string(),
                current.0,
                current.1,
                payload,
            );
            // sequence lock 内で enqueue して、同一 endpoint の永続順も sequence 順に固定する。
            // enqueue は unbounded channel send のみで、SQLite I/O は writer thread が担う。
            if let Some(persistence) = &self.persistence {
                persistence.append(event.clone());
            }
            recover_mutex(self.event_buffer.lock()).push(event.clone());
            event
        };
        event
    }

    pub fn persist_team_binding(&self, binding: RuntimeTeamBinding<'_>) {
        let Some(persistence) = &self.persistence else {
            return;
        };
        let Some(project_root) = binding
            .project_root
            .map(str::trim)
            .filter(|root| !root.is_empty())
        else {
            tracing::warn!(
                team_id = binding.team_id,
                endpoint_id = binding.endpoint_id,
                "[runtime-persistence] binding has no authorized project root"
            );
            return;
        };
        let Some((epoch, _)) = recover_mutex(self.sequences.lock())
            .get(binding.endpoint_id)
            .copied()
        else {
            tracing::warn!(
                endpoint_id = binding.endpoint_id,
                "[runtime-persistence] binding has no active epoch"
            );
            return;
        };
        persistence.bind(PersistedRuntimeBinding {
            project_root: project_root.to_string(),
            team_id: binding.team_id.to_string(),
            agent_id: binding.agent_id.to_string(),
            endpoint_id: binding.endpoint_id.to_string(),
            epoch,
            provider: binding.provider.to_string(),
            resume_id: binding.resume_id,
            resumable: binding.resumable,
        });
    }

    pub fn restore_latest(&self, project_root: &str) -> Result<RuntimeRestoreSnapshot, String> {
        match &self.persistence {
            Some(persistence) => persistence.restore_latest(project_root),
            None => Ok(RuntimeRestoreSnapshot::default()),
        }
    }

    #[allow(dead_code)]
    pub fn event_snapshot(&self) -> Vec<RuntimeEventEnvelope> {
        recover_mutex(self.event_buffer.lock()).snapshot()
    }

    #[allow(dead_code)]
    pub fn dropped_event_count(&self) -> u64 {
        recover_mutex(self.event_buffer.lock()).dropped_count()
    }

    #[cfg(test)]
    pub fn tracked_sequence_count(&self) -> usize {
        recover_mutex(self.sequences.lock()).len()
    }
}

fn epoch_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
        .min(u64::MAX as u128) as u64
}

impl Default for RuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RuntimeManager {
    fn drop(&mut self) {
        self.registry.dispose_all();
    }
}
