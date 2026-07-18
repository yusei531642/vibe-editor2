import { FileCode2, Files, X } from 'lucide-react';
import { useT } from '../../lib/i18n';

export function V2WorkspaceDrawer({
  projectName,
  changedFiles,
  hasEntries,
  onClose,
  onOpenProject,
}: {
  projectName: string;
  changedFiles: Array<{ path: string }>;
  hasEntries: boolean;
  onClose: () => void;
  onOpenProject: () => void;
}): JSX.Element {
  const t = useT();
  return (
    <aside className="v2-drawer v2-drawer--left" aria-label={t('v2.drawer.left')}>
      <header>
        <strong>{t('v2.drawer.workspace')}</strong>
        <button type="button" aria-label={t('common.close')} onClick={onClose}>
          <X size={20} />
        </button>
      </header>
      <section>
        <h2>{t('v2.drawer.projects')}</h2>
        <button type="button" className="v2-drawer-row" onClick={onOpenProject}>
          <Files size={18} />
          {projectName}
        </button>
      </section>
      <section>
        <h2>{t('v2.drawer.sessions')}</h2>
        <p>{hasEntries ? t('v2.drawer.currentSession') : t('v2.drawer.noSessions')}</p>
      </section>
      <section>
        <h2>{t('v2.drawer.changedFiles')}</h2>
        {changedFiles.slice(0, 8).map((file) => (
          <div className="v2-drawer-row" key={file.path}>
            <FileCode2 size={17} />
            {file.path}
          </div>
        ))}
      </section>
    </aside>
  );
}
