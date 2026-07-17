import { useMemo, useState } from 'react';
import {
  CirclePause,
  ExternalLink,
  Send,
  Square,
  Trash2,
  X
} from 'lucide-react';
import { useT } from '../../../../lib/i18n';
import { useTeamProjection } from '../../../v2/TeamProjectionProvider';

export function AgentCardRuntime({ agentId }: { agentId?: string }): JSX.Element | null {
  const t = useT();
  const {
    projection,
    dispatchAgentAction,
    openInspector
  } = useTeamProjection();
  const agent = useMemo(
    () => projection.agents.find((candidate) => candidate.agentId === agentId) ?? null,
    [agentId, projection.agents]
  );
  const [instruction, setInstruction] = useState('');
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmingDismiss, setConfirmingDismiss] = useState(false);

  if (!agentId || !agent) return null;

  const run = async (action: 'steer' | 'interrupt' | 'stop' | 'dismiss'): Promise<void> => {
    setBusyAction(action);
    setError(null);
    try {
      await dispatchAgentAction(agentId, action, instruction);
      if (action === 'steer') setInstruction('');
      if (action === 'dismiss') setConfirmingDismiss(false);
    } catch (actionError) {
      setError(actionError instanceof Error ? actionError.message : String(actionError));
    } finally {
      setBusyAction(null);
    }
  };

  const latestSummary = agent.latestTool
    ? `${agent.latestTool.toolName} · ${agent.latestTool.status}`
    : agent.latestDiff
      ? t('v2.team.card.diffReady')
      : agent.latestUsage
        ? t('v2.team.card.usage', { count: agent.latestUsage.outputTokens })
        : t('v2.team.card.noRuntime');

  return (
    <section className="canvas-agent-runtime nodrag glass-surface" aria-label={t('v2.team.card.runtime')}>
      <div className="canvas-agent-runtime__status">
        <span className={`team-agent-state team-agent-state--${agent.status}`}>
          <i aria-hidden="true" />
          {t(`v2.team.status.${agent.status}`)}
        </span>
        <span title={agent.task?.description ?? undefined}>
          {agent.task?.description ?? t('v2.team.card.noTask')}
        </span>
      </div>
      <p className="canvas-agent-runtime__latest">{latestSummary}</p>
      <form
        className="canvas-agent-runtime__steer"
        onSubmit={(event) => {
          event.preventDefault();
          void run('steer');
        }}
      >
        <label>
          <span className="sr-only">{t('v2.team.card.steerInput')}</span>
          <input
            value={instruction}
            onChange={(event) => setInstruction(event.target.value)}
            placeholder={t('v2.team.card.steerPlaceholder')}
            aria-label={t('v2.team.card.steerInput')}
          />
        </label>
        <button
          type="submit"
          disabled={!instruction.trim() || busyAction !== null}
          aria-label={t('v2.team.card.steer')}
          title={t('v2.team.card.steer')}
        >
          <Send size={16} strokeWidth={1.75} aria-hidden="true" />
        </button>
      </form>
      <div className="canvas-agent-runtime__actions">
        <button
          type="button"
          onClick={() => openInspector(agentId)}
          aria-label={t('v2.team.card.inspect')}
          title={t('v2.team.card.inspect')}
        >
          <ExternalLink size={16} strokeWidth={1.75} aria-hidden="true" />
        </button>
        <button
          type="button"
          disabled={busyAction !== null}
          onClick={() => void run('interrupt')}
          aria-label={t('v2.team.card.pause')}
          title={t('v2.team.card.pause')}
        >
          <CirclePause size={16} strokeWidth={1.75} aria-hidden="true" />
        </button>
        <button
          type="button"
          disabled={busyAction !== null}
          onClick={() => void run('stop')}
          aria-label={t('v2.team.card.stop')}
          title={t('v2.team.card.stop')}
        >
          <Square size={16} strokeWidth={1.75} aria-hidden="true" />
        </button>
        {confirmingDismiss ? (
          <>
            <button
              type="button"
              className="canvas-agent-runtime__danger"
              disabled={busyAction !== null}
              onClick={() => void run('dismiss')}
              aria-label={t('v2.team.card.confirmDismiss')}
            >
              {t('v2.team.card.confirm')}
            </button>
            <button
              type="button"
              onClick={() => setConfirmingDismiss(false)}
              aria-label={t('v2.team.card.cancelDismiss')}
            >
              <X size={16} strokeWidth={1.75} aria-hidden="true" />
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={() => setConfirmingDismiss(true)}
            aria-label={t('v2.team.card.dismiss')}
            title={t('v2.team.card.dismiss')}
          >
            <Trash2 size={16} strokeWidth={1.75} aria-hidden="true" />
          </button>
        )}
      </div>
      {error ? (
        <p className="canvas-agent-runtime__error" role="status">
          {error}
        </p>
      ) : null}
    </section>
  );
}
