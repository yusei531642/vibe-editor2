//! runtime endpoint routing のテスト専用フック。
//! production コードから分離して runtime_endpoint/mod.rs の 500 行 ratchet を守る。

use super::types::*;
use crate::agent_runtime::BackendKind;
use crate::team_hub::TeamHub;

impl TeamHub {
    #[cfg(test)]
    pub(crate) fn set_runtime_backend_for_test(&self, backend: BackendKind) {
        *self
            .runtime
            .backend_override
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(backend);
    }

    #[cfg(test)]
    pub(crate) fn set_codex_delivery_for_test(
        &self,
        delivery: crate::team_hub::codex_delivery::CodexDelivery,
    ) {
        *self
            .runtime
            .codex_delivery_override
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(delivery);
    }

    #[cfg(test)]
    pub(crate) fn set_legacy_app_server_result_for_test(&self, result: bool) {
        *self
            .runtime
            .legacy_app_server_override
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result);
    }

    #[cfg(test)]
    pub(crate) fn take_legacy_app_server_deliveries_for_test(
        &self,
    ) -> Vec<LegacyAppServerDelivery> {
        std::mem::take(
            &mut *self
                .runtime
                .legacy_app_server_deliveries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }
}
