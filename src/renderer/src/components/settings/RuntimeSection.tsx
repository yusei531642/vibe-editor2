import { useCallback, useEffect, useState } from 'react';
import { RefreshCw } from 'lucide-react';
import type {
  AgentRuntimeBackend,
  AgentRuntimeDiagnostics,
  AppSettings
} from '../../../../types/shared';
import { useT } from '../../lib/i18n';
import type { UpdateSetting } from './types';
import '../../styles/components/runtime-settings.css';

interface Props {
  draft: AppSettings;
  update: UpdateSetting;
}

const BACKENDS: AgentRuntimeBackend[] = ['pty', 'auto', 'native'];

export function RuntimeSection({ draft, update }: Props): JSX.Element {
  const t = useT();
  const [diagnostics, setDiagnostics] = useState<AgentRuntimeDiagnostics | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // backend 変更と手動 refresh の両方で同じ effect を再実行させるためのカウンタ
  const [reloadKey, setReloadKey] = useState(0);

  const refreshDiagnostics = useCallback((): void => {
    setReloadKey((key) => key + 1);
  }, []);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);
    window.api.agentRuntime
      .diagnostics(draft.agentRuntimeBackend)
      .then((result) => {
        if (active) setDiagnostics(result);
      })
      .catch((err: unknown) => {
        if (!active) return;
        setDiagnostics(null);
        setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [draft.agentRuntimeBackend, reloadKey]);

  return (
    <section className="modal__section runtime-section glass-surface">
      <h3>{t('settings.runtime.title')}</h3>
      <label className="runtime-section__field">
        <span>{t('settings.runtime.backend.label')}</span>
        <select
          value={draft.agentRuntimeBackend}
          onChange={(event) =>
            update('agentRuntimeBackend', event.target.value as AgentRuntimeBackend)
          }
        >
          {BACKENDS.map((backend) => (
            <option key={backend} value={backend}>
              {t(`settings.runtime.backend.${backend}`)}
            </option>
          ))}
        </select>
      </label>
      <p className="modal__note">{t('settings.runtime.backend.hint')}</p>

      <label className="mcp-toggle runtime-section__toggle">
        <input
          type="checkbox"
          checked={draft.teamSceneV2}
          onChange={(event) => update('teamSceneV2', event.target.checked)}
        />
        <span className="mcp-toggle__track" aria-hidden="true">
          <span className="mcp-toggle__thumb" />
        </span>
        <span className="mcp-toggle__label">{t('settings.runtime.teamSceneV2.label')}</span>
      </label>
      <p className="modal__note">{t('settings.runtime.teamSceneV2.hint')}</p>

      <div className="runtime-diagnostics" aria-live="polite">
        <div className="runtime-diagnostics__header">
          <strong>{t('settings.runtime.diagnostics.title')}</strong>
          <button
            type="button"
            className="runtime-diagnostics__refresh"
            onClick={() => void refreshDiagnostics()}
            disabled={loading}
          >
            <RefreshCw size={13} className={loading ? 'is-spinning' : undefined} />
            {t('settings.runtime.diagnostics.refresh')}
          </button>
        </div>
        {loading && !diagnostics && (
          <p className="runtime-diagnostics__status">
            {t('settings.runtime.diagnostics.loading')}
          </p>
        )}
        {error && (
          <p className="runtime-diagnostics__status runtime-diagnostics__status--error">
            {t('settings.runtime.diagnostics.error', { error })}
          </p>
        )}
        {diagnostics && (
          <div className="runtime-diagnostics__body">
            <p>
              <span>{t('settings.runtime.diagnostics.selected')}</span>
              <strong>{t(`settings.runtime.backend.${diagnostics.selectedBackend}`)}</strong>
            </p>
            <p>{t(`settings.runtime.reason.${diagnostics.reason}`)}</p>
            <div>
              <span>{t('settings.runtime.diagnostics.capabilities')}</span>
              <ul>
                {diagnostics.capabilities.map((capability) => (
                  <li key={capability}>{t(`settings.runtime.capability.${capability}`)}</li>
                ))}
              </ul>
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
