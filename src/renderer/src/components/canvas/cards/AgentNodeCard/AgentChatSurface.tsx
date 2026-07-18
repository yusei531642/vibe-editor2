import { useEffect, useMemo, type KeyboardEvent } from 'react';
import {
  ArrowUp,
  ExternalLink,
  RotateCcw,
  Square,
  Trash2,
  X
} from 'lucide-react';
import type { RuntimePermission } from '../../../../../../types/agent-runtime';
import { useV2RuntimeCatalog } from '../../../../lib/hooks/use-v2-runtime-catalog';
import type { TeamAgentProjection } from '../../../../lib/team-projection';
import { isNativeRuntimeProvider } from './NativeRuntimeConnector';
import type { AgentPayload } from './types';

type TFn = (key: string, params?: Record<string, string | number>) => string;

interface AgentChatSurfaceProps {
  agent: TeamAgentProjection;
  payload?: AgentPayload;
  instruction: string;
  busyAction: string | null;
  confirmingDismiss: boolean;
  onInstructionChange: (value: string) => void;
  onRuntimePatch: (patch: Partial<AgentPayload>) => void;
  onSubmit: () => void;
  onAction: (action: 'interrupt' | 'stop' | 'dismiss' | 'reconnect') => void;
  onInspect: () => void;
  onConfirmingDismissChange: (value: boolean) => void;
  t: TFn;
}

function AgentMessages({ agent, payload, t }: Pick<AgentChatSurfaceProps, 'agent' | 'payload' | 't'>) {
  const userMessages = useMemo(
    () => [payload?.initialMessage ?? agent.task?.description, ...(payload?.chatUserMessages ?? [])].filter(
      (message): message is string => Boolean(message?.trim())
    ),
    [agent.task?.description, payload?.chatUserMessages, payload?.initialMessage]
  );
  const assistantMessages = agent.runtime?.completedMessages ?? [];
  const count = Math.max(userMessages.length, assistantMessages.length);
  const rows: JSX.Element[] = [];
  for (let index = 0; index < count; index += 1) {
    const user = userMessages[index];
    const assistant = assistantMessages[index];
    if (user) {
      rows.push(
        <article key={`user-${index}`} className="team-chat-message team-chat-message--user">
          <span>{t('v2.team.card.you')}</span>
          <p>{user}</p>
        </article>
      );
    }
    if (assistant) {
      rows.push(
        <article key={`agent-${index}`} className="team-chat-message team-chat-message--agent">
          <span>{agent.title}</span>
          <p>{assistant}</p>
        </article>
      );
    }
  }
  if (agent.runtime?.currentMessage) {
    rows.push(
      <article key="stream" className="team-chat-message team-chat-message--agent" aria-live="polite">
        <span>{agent.title}</span>
        <p>{agent.runtime.currentMessage}<i className="team-chat-cursor" /></p>
      </article>
    );
  }
  return <div className="team-chat-history nodrag nowheel">{rows}</div>;
}

export function AgentChatSurface(props: AgentChatSurfaceProps): JSX.Element {
  const { agent, payload, busyAction, instruction, onRuntimePatch, t } = props;
  const engine = payload?.agent ?? 'claude';
  const nativeRuntime = isNativeRuntimeProvider(payload?.runtimeProvider);
  const catalog = useV2RuntimeCatalog(engine, nativeRuntime);
  const modelValue = payload?.runtimeModel ?? catalog.models[0]?.id ?? '';
  const model = catalog.models.find((option) => option.id === modelValue) ?? catalog.models[0];
  const efforts = model?.supportedEfforts ?? (payload?.runtimeEffort ? [payload.runtimeEffort] : []);
  const effortValue = payload?.runtimeEffort ?? model?.defaultEffort ?? efforts[0] ?? '';
  const permission = nativeRuntime ? 'workspace' : (payload?.runtimePermission ?? 'workspace');
  const running = agent.status === 'running' || agent.status === 'spawning';
  const unavailable = Boolean(agent.endpoint && !agent.endpoint.live);
  const canSubmit = Boolean(instruction.trim()) && busyAction === null && !unavailable;
  const canInterrupt = running && !instruction.trim() && busyAction === null && !unavailable;
  useEffect(() => {
    if (!payload || !nativeRuntime) return;
    const patch: Partial<AgentPayload> = {};
    if (!payload.runtimeModel && modelValue) patch.runtimeModel = modelValue;
    if (!payload.runtimeEffort && modelValue && effortValue) patch.runtimeEffort = effortValue;
    if (payload.runtimePermission === 'full') patch.runtimePermission = 'workspace';
    if (Object.keys(patch).length > 0) onRuntimePatch(patch);
  }, [effortValue, modelValue, nativeRuntime, onRuntimePatch, payload]);
  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): void => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      if (canSubmit) props.onSubmit();
    }
  };

  return (
    <section className="canvas-agent-runtime team-chat-card__body nodrag" aria-label={t('v2.team.card.runtime')}>
      <div className="team-chat-meta">
        <span className={`team-agent-state team-agent-state--${agent.status}`}>
          <i aria-hidden="true" />{t(`v2.team.status.${agent.status}`)}
        </span>
        <span title={agent.task?.description}>{agent.task?.description ?? t('v2.team.card.noTask')}</span>
      </div>
      <AgentMessages agent={agent} payload={payload} t={t} />
      {agent.latestTool ? (
        <p className="team-chat-tool"><span>⎿</span>{agent.latestTool.toolName} · {agent.latestTool.status}</p>
      ) : null}
      <div className="team-chat-composer">
        <textarea
          value={instruction}
          rows={2}
          onChange={(event) => props.onInstructionChange(event.target.value)}
          onKeyDown={handleKeyDown}
          placeholder={t('v2.team.card.steerPlaceholder')}
          aria-label={t('v2.team.card.steerInput')}
        />
        <div className="team-chat-composer__toolbar">
          <div className="team-chat-composer__controls">
            <select
              value={modelValue}
              disabled={running || catalog.models.length === 0}
              onChange={(event) => {
                const next = catalog.models.find((option) => option.id === event.target.value);
                props.onRuntimePatch({
                  runtimeModel: event.target.value,
                  runtimeEffort: next?.defaultEffort
                });
              }}
              aria-label={t('v2.composer.model')}
            >
              {catalog.models.map((option) => <option key={option.id} value={option.id}>{option.label}</option>)}
            </select>
            <select
              value={effortValue}
              disabled={running || efforts.length === 0}
              onChange={(event) => props.onRuntimePatch({ runtimeEffort: event.target.value })}
              aria-label={t('v2.composer.effort')}
            >
              {efforts.map((effort) => <option key={effort} value={effort}>{effort}</option>)}
            </select>
            <select
              value={permission}
              disabled={running || nativeRuntime}
              onChange={(event) => props.onRuntimePatch({ runtimePermission: event.target.value as RuntimePermission })}
              aria-label={t('v2.composer.permission')}
            >
              <option value="workspace">{t('v2.permission.workspace')}</option>
              <option value="full">{t('v2.permission.full')}</option>
            </select>
          </div>
          <div className="team-chat-composer__actions">
            {agent.endpoint?.restoreState === 'reconnectable' ? (
              <button type="button" onClick={() => props.onAction('reconnect')} aria-label={t('v2.team.card.reconnect')}>
                <RotateCcw size={16} strokeWidth={1.75} />
              </button>
            ) : null}
            <button type="button" onClick={props.onInspect} aria-label={t('v2.team.card.inspect')}>
              <ExternalLink size={16} strokeWidth={1.75} />
            </button>
            <button type="button" disabled={busyAction !== null} onClick={() => props.onAction('stop')} aria-label={t('v2.team.card.stop')}>
              <Square size={15} strokeWidth={1.75} />
            </button>
            {props.confirmingDismiss ? (
              <>
                <button type="button" className="team-chat-action--danger" onClick={() => props.onAction('dismiss')} aria-label={t('v2.team.card.confirmDismiss')}>
                  <Trash2 size={16} strokeWidth={1.75} />
                </button>
                <button type="button" onClick={() => props.onConfirmingDismissChange(false)} aria-label={t('v2.team.card.cancelDismiss')}>
                  <X size={16} strokeWidth={1.75} />
                </button>
              </>
            ) : (
              <button type="button" onClick={() => props.onConfirmingDismissChange(true)} aria-label={t('v2.team.card.dismiss')}>
                <Trash2 size={16} strokeWidth={1.75} />
              </button>
            )}
            <button
              type="button"
              className="team-chat-send"
              disabled={!canSubmit && !canInterrupt}
              onClick={canInterrupt ? () => props.onAction('interrupt') : props.onSubmit}
              aria-label={canInterrupt ? t('v2.team.card.pause') : t('v2.team.card.steer')}
            >
              {canInterrupt
                ? <Square size={13} fill="currentColor" />
                : <ArrowUp size={19} strokeWidth={1.75} />}
            </button>
          </div>
        </div>
      </div>
    </section>
  );
}
