//! Endpoint registry and runtime operation coordinator.

use super::{
    AgentRuntimeAdapter, RuntimeAdapterError, RuntimeEventBuffer, RuntimeEventEnvelope,
    RuntimeEventPayload, RuntimeLifecycleState, RuntimeSessionSpawnRequest,
    RuntimeTurnSpawnRequest, DEFAULT_RUNTIME_EVENT_BUFFER_CAPACITY,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

fn recover_mutex<'a, T>(
    result: Result<MutexGuard<'a, T>, PoisonError<MutexGuard<'a, T>>>,
) -> MutexGuard<'a, T> {
    result.unwrap_or_else(|poisoned| {
        tracing::warn!("[runtime] mutex poisoned — recovering inner data");
        poisoned.into_inner()
    })
}

pub struct RuntimeEndpointRegistry {
    endpoints: RwLock<HashMap<String, Arc<dyn AgentRuntimeAdapter>>>,
}

impl RuntimeEndpointRegistry {
    pub fn new() -> Self {
        Self {
            endpoints: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(
        &self,
        endpoint_id: String,
        adapter: Arc<dyn AgentRuntimeAdapter>,
    ) -> Result<(), RuntimeAdapterError> {
        let mut endpoints = self
            .endpoints
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if endpoints.contains_key(&endpoint_id) {
            return Err(RuntimeAdapterError::new(
                "runtime_endpoint_exists",
                format!("runtime endpoint '{endpoint_id}' is already registered"),
                true,
            ));
        }
        endpoints.insert(endpoint_id, adapter);
        Ok(())
    }

    pub fn resolve(&self, endpoint_id: &str) -> Option<Arc<dyn AgentRuntimeAdapter>> {
        self.endpoints
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(endpoint_id)
            .cloned()
    }

    pub fn remove(&self, endpoint_id: &str) -> Option<Arc<dyn AgentRuntimeAdapter>> {
        self.endpoints
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(endpoint_id)
    }

    pub fn dispose(&self, endpoint_id: &str) -> Result<(), RuntimeAdapterError> {
        let adapter = self.remove(endpoint_id).ok_or_else(|| {
            RuntimeAdapterError::new(
                "runtime_endpoint_not_found",
                format!("runtime endpoint '{endpoint_id}' was not found"),
                true,
            )
        })?;
        adapter.dispose()
    }

    pub fn dispose_all(&self) {
        let adapters: Vec<_> = self
            .endpoints
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .drain()
            .map(|(_, adapter)| adapter)
            .collect();
        for adapter in adapters {
            if let Err(error) = adapter.dispose() {
                tracing::warn!(code = %error.code, "[runtime] endpoint dispose failed: {error}");
            }
        }
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.endpoints
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for RuntimeEndpointRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RuntimeOperation {
    pub events: Vec<RuntimeEventEnvelope>,
    pub result: Result<(), RuntimeAdapterError>,
}

pub struct RuntimeManager {
    registry: RuntimeEndpointRegistry,
    sequences: Mutex<HashMap<String, u64>>,
    event_buffer: Mutex<RuntimeEventBuffer>,
}

impl RuntimeManager {
    pub fn new() -> Self {
        Self::with_event_buffer_capacity(DEFAULT_RUNTIME_EVENT_BUFFER_CAPACITY)
    }

    pub fn with_event_buffer_capacity(capacity: usize) -> Self {
        Self {
            registry: RuntimeEndpointRegistry::new(),
            sequences: Mutex::new(HashMap::new()),
            event_buffer: Mutex::new(RuntimeEventBuffer::new(capacity)),
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
        if let Err(error) = self.registry.register(endpoint_id.clone(), adapter.clone()) {
            return RuntimeOperation {
                events: Vec::new(),
                result: Err(error),
            };
        }

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
        let request = RuntimeSessionSpawnRequest {
            endpoint_id: endpoint_id.clone(),
        };
        match adapter.spawn_session(&request) {
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

    /// TeamHub 等の上位層が PTY registry を知らずに endpoint へ配送する API。
    #[allow(dead_code)]
    pub fn deliver_to_endpoint(&self, endpoint_id: &str, data: &str) -> RuntimeOperation {
        self.write(endpoint_id, data)
    }

    /// Runtime の実行を停止する。cooperative cancellation capability を持たない PTY adapter
    /// では `stop` は session process の kill を意味する。成功後は endpoint を自動 detach し、
    /// terminal layer が所有する session lifetime には dispose 経由で干渉しない。
    pub fn stop(&self, endpoint_id: &str) -> RuntimeOperation {
        let mut operation = self.run_adapter_operation(endpoint_id, |adapter| adapter.stop());
        if operation.result.is_ok() {
            operation.events.push(self.record_event(
                endpoint_id,
                RuntimeEventPayload::Lifecycle {
                    state: RuntimeLifecycleState::Exited,
                    detail: Some("interrupted".to_string()),
                },
            ));
            self.detach_endpoint(endpoint_id);
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

    fn detach_endpoint(&self, endpoint_id: &str) {
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

    fn remove_sequence(&self, endpoint_id: &str) {
        recover_mutex(self.sequences.lock()).remove(endpoint_id);
    }

    fn failure_events(
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
    fn transient_error_event(
        &self,
        endpoint_id: &str,
        error: &RuntimeAdapterError,
    ) -> RuntimeEventEnvelope {
        let event = RuntimeEventEnvelope::new(
            endpoint_id.to_string(),
            1,
            RuntimeEventPayload::Error {
                code: error.code.clone(),
                message: error.message.clone(),
                recoverable: error.recoverable,
            },
        );
        recover_mutex(self.event_buffer.lock()).push(event.clone());
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
        let sequence = {
            let mut sequences = recover_mutex(self.sequences.lock());
            let current = sequences.entry(endpoint_id.to_string()).or_insert(0);
            *current = current.saturating_add(1);
            *current
        };
        let event = RuntimeEventEnvelope::new(endpoint_id.to_string(), sequence, payload);
        recover_mutex(self.event_buffer.lock()).push(event.clone());
        event
    }

    #[allow(dead_code)]
    pub fn event_snapshot(&self) -> Vec<RuntimeEventEnvelope> {
        recover_mutex(self.event_buffer.lock()).snapshot()
    }

    #[allow(dead_code)]
    pub fn dropped_event_count(&self) -> u64 {
        recover_mutex(self.event_buffer.lock()).dropped_count()
    }
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
