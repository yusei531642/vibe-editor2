import { useMemo, useState } from 'react';
import { useT } from '../../../../lib/i18n';
import type { AgentPayload } from './types';
import { useTeamProjection } from '../../../v2/TeamProjectionProvider';
import { AgentChatSurface } from './AgentChatSurface';

export function AgentCardRuntime({
  agentId,
  payload,
  setCardPayload
}: {
  agentId?: string;
  payload?: AgentPayload;
  setCardPayload?: (patch: Partial<AgentPayload>) => void;
}): JSX.Element | null {
  const t = useT();
  const { projection, dispatchAgentAction, openInspector, reconnect } = useTeamProjection();
  const agent = useMemo(
    () => projection.agents.find((candidate) => candidate.agentId === agentId) ?? null,
    [agentId, projection.agents]
  );
  const [instruction, setInstruction] = useState('');
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmingDismiss, setConfirmingDismiss] = useState(false);

  if (!agentId || !agent) return null;

  const run = async (action: 'steer' | 'interrupt' | 'stop' | 'dismiss' | 'reconnect'): Promise<void> => {
    const message = instruction.trim();
    setBusyAction(action);
    setError(null);
    try {
      if (action === 'reconnect') await reconnect(agentId);
      else if (action === 'steer' && agent.endpoint?.backend === 'native' && agent.status === 'ready') {
        await window.api.agentRuntime.spawnTurn({
          endpointId: agent.endpoint.endpointId,
          input: message,
          submit: true,
          model: payload?.runtimeModel ?? null,
          effort: payload?.runtimeEffort ?? null,
          permission: payload?.runtimePermission ?? 'workspace'
        });
      } else {
        await dispatchAgentAction(agentId, action, instruction);
      }
      if (action === 'steer') {
        setCardPayload?.({ chatUserMessages: [...(payload?.chatUserMessages ?? []), message] });
        setInstruction('');
      }
      if (action === 'dismiss') setConfirmingDismiss(false);
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError));
    } finally {
      setBusyAction(null);
    }
  };

  return (
    <>
      <AgentChatSurface
        agent={agent}
        payload={payload}
        instruction={instruction}
        busyAction={busyAction}
        confirmingDismiss={confirmingDismiss}
        onInstructionChange={setInstruction}
        onRuntimePatch={(patch) => setCardPayload?.(patch)}
        onSubmit={() => void run('steer')}
        onAction={(action) => void run(action)}
        onInspect={() => openInspector(agentId)}
        onConfirmingDismissChange={setConfirmingDismiss}
        t={t}
      />
      {error ? <p className="team-chat-error" role="status">{error}</p> : null}
    </>
  );
}
