//! (teamId, agentId) ごとの runtime endpoint binding の型定義。
//! 実装 (bind / deliver / cleanup) は runtime_endpoint.rs / runtime_cleanup.rs 側にある。
//! runtime_endpoint.rs の 500 行 ratchet を守るため型だけを分離した。

use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeEndpointBackend {
    Native,
    Pty,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeEndpoint {
    pub endpoint_id: String,
    pub backend: RuntimeEndpointBackend,
    pub session_id: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AgentRuntimeBinding {
    pub native: Option<RuntimeEndpoint>,
    pub pty: Option<RuntimeEndpoint>,
    pub task_ids: Vec<u32>,
}

pub(crate) type RuntimeEndpointMap = HashMap<(String, String), AgentRuntimeBinding>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TeamRuntimeEndpointSnapshot {
    pub team_id: String,
    pub agent_id: String,
    pub endpoint_id: String,
    pub backend: String,
    pub session_id: Option<String>,
    pub task_ids: Vec<u32>,
    pub live: bool,
}
#[cfg(test)]
pub(crate) type LegacyAppServerDelivery = (String, String, String, String);
