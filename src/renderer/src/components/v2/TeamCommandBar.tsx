import { useState } from 'react';
import { CheckCheck, Radio, Send } from 'lucide-react';
import { useT } from '../../lib/i18n';
import { KEYS, useKeybinding } from '../../lib/keybindings';
import { useTeamProjection } from './TeamProjectionProvider';

export function TeamCommandBar(): JSX.Element {
  const t = useT();
  const { projection, broadcast, setApprovalsOpen } = useTeamProjection();
  const [message, setMessage] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);

  useKeybinding(KEYS.teamApprovals, () => setApprovalsOpen(true));
  useKeybinding(KEYS.teamApprovalsMac, () => setApprovalsOpen(true));

  const submit = async (): Promise<void> => {
    setSending(true);
    setError(null);
    try {
      await broadcast(message);
      setMessage('');
    } catch (submitError) {
      setError(submitError instanceof Error ? submitError.message : String(submitError));
    } finally {
      setSending(false);
    }
  };

  return (
    <section className="team-command-bar glass-surface" aria-label={t('v2.team.commandBar')}>
      <Radio size={18} strokeWidth={1.75} aria-hidden="true" />
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <label>
          <span className="sr-only">{t('v2.team.broadcastInput')}</span>
          <input
            value={message}
            onChange={(event) => setMessage(event.target.value)}
            placeholder={t('v2.team.broadcastPlaceholder')}
            aria-label={t('v2.team.broadcastInput')}
          />
        </label>
        <button
          type="submit"
          disabled={!message.trim() || sending}
          aria-label={t('v2.team.broadcast')}
        >
          <Send size={16} strokeWidth={1.75} aria-hidden="true" />
          {t('v2.team.broadcast')}
        </button>
      </form>
      <button
        type="button"
        className="team-command-bar__approvals"
        onClick={() => setApprovalsOpen(true)}
        aria-label={t('v2.team.openApprovals', { count: projection.approvals.length })}
      >
        <CheckCheck size={18} strokeWidth={1.75} aria-hidden="true" />
        {t('v2.team.approvals')}
        <span>{projection.approvals.length}</span>
        <kbd>Ctrl Shift A</kbd>
      </button>
      {error ? <p role="status">{error}</p> : null}
    </section>
  );
}
