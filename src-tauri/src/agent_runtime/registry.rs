//! Runtime endpoint registry.

use super::{AgentRuntimeAdapter, RuntimeAdapterError};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
