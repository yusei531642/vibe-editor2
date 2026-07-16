// tauri-api/agent-runtime.ts — Issue #21 runtime capability diagnostics.

import { invokeCommand } from './command-error';
import type {
  AgentRuntimeBackend,
  AgentRuntimeDiagnostics
} from '../../../../types/shared';

export const agentRuntime = {
  diagnostics: (backend: AgentRuntimeBackend): Promise<AgentRuntimeDiagnostics> =>
    invokeCommand('agent_runtime_diagnostics', { backend })
};
