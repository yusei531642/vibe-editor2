//! TeamHub delivery operation built on the runtime adapter boundary.

use super::{RuntimeAdapterError, RuntimeDeliveryRequest, RuntimeManager, RuntimeOperation};

impl RuntimeManager {
    /// TeamHub の構造化配送。adapter ごとの差異 (native turn / PTY bracketed paste) は
    /// `AgentRuntimeAdapter::deliver` の内側へ閉じ込める。
    pub async fn deliver_team_message(
        &self,
        endpoint_id: &str,
        request: RuntimeDeliveryRequest,
    ) -> RuntimeOperation {
        let Some(adapter) = self.registry().resolve(endpoint_id) else {
            let error = RuntimeAdapterError::new(
                "runtime_endpoint_not_found",
                format!("runtime endpoint '{endpoint_id}' was not found"),
                true,
            );
            return RuntimeOperation {
                events: vec![self.transient_error_event(endpoint_id, &error)],
                result: Err(error),
            };
        };
        match adapter.deliver(&request).await {
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
}
