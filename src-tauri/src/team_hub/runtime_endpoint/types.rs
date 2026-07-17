//! (teamId, agentId) ごとの runtime endpoint binding の型定義。
//! 実装 (bind / deliver / cleanup) は runtime_endpoint.rs / runtime_cleanup.rs 側にある。
//! runtime_endpoint.rs の 500 行 ratchet を守るため型だけを分離した。

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
    /// prune 済み (dead) native endpoint の履歴。reconnect の「過去に native だった」判定に
    /// 使い、spawn-phase gate を免除する (PR #34 レビュー: prune 後の reconnect 復帰不能防止)。
    pub prior_native_endpoint: Option<String>,
    pub pty: Option<RuntimeEndpoint>,
    pub task_ids: Vec<u32>,
}

pub(crate) type RuntimeEndpointMap = HashMap<(String, String), AgentRuntimeBinding>;
#[cfg(test)]
pub(crate) type LegacyAppServerDelivery = (String, String, String, String);
