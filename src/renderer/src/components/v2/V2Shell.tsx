import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  ArrowRight,
  Bug,
  CodeXml,
  FileCode2,
  Files,
  Hammer,
  PanelLeft,
  PanelRight,
  SearchCode,
  SquarePen,
  TestTube2,
  X,
} from "lucide-react";
import { useProject, useTeam } from "../../lib/app-state-context";
import { useT } from "../../lib/i18n";
import {
  UnifiedComposer,
  type V2Engine,
  type V2Permission,
} from "./UnifiedComposer";

interface TimelineEntry {
  id: string;
  role: "user" | "agent";
  text: string;
  engine: V2Engine;
}

const QUICK_ACTIONS = [
  {
    key: "explore",
    labelKey: "v2.quick.explore.label",
    promptKey: "v2.quick.explore.prompt",
    icon: SearchCode,
    tone: "blue",
  },
  {
    key: "build",
    labelKey: "v2.quick.build.label",
    promptKey: "v2.quick.build.prompt",
    icon: Hammer,
    tone: "violet",
  },
  {
    key: "review",
    labelKey: "v2.quick.review.label",
    promptKey: "v2.quick.review.prompt",
    icon: CodeXml,
    tone: "green",
  },
  {
    key: "fix",
    labelKey: "v2.quick.fix.label",
    promptKey: "v2.quick.fix.prompt",
    icon: Bug,
    tone: "orange",
  },
] as const;

export function V2Shell(): JSX.Element {
  const t = useT();
  const { projectRoot, handleOpenFolder, gitStatus } = useProject();
  const { claudeCheck, runClaudeCheck } = useTeam();
  const [engine, setEngine] = useState<V2Engine>("claude");
  const [permission, setPermission] = useState<V2Permission>("workspace");
  const [prompt, setPrompt] = useState("");
  const [running, setRunning] = useState(false);
  const [hasStarted, setHasStarted] = useState(false);
  const [leftOpen, setLeftOpen] = useState(false);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [entries, setEntries] = useState<TimelineEntry[]>([]);

  const projectName = useMemo(() => {
    if (!projectRoot) return t("v2.project.select");
    return projectRoot.split(/[\\/]/).filter(Boolean).at(-1) ?? projectRoot;
  }, [projectRoot, t]);
  const branch = gitStatus?.branch || "main";
  const model = engine === "claude" ? "Fable 5" : "5.6 Sol";

  const selectQuickAction = (template: string): void => {
    setPrompt((current) =>
      current.trim() ? `${current}\n\n${template}` : template,
    );
    window.dispatchEvent(new Event("vibe-editor2:focus-composer"));
  };

  const submit = useCallback(() => {
    const text = prompt.trim();
    if (!text || running) return;
    const entry: TimelineEntry = {
      id: crypto.randomUUID(),
      role: "user",
      text,
      engine,
    };
    setEntries((current) => [...current, entry]);
    setPrompt("");
    setHasStarted(true);
    setRunning(true);
    window.setTimeout(() => {
      setEntries((current) => [
        ...current,
        {
          id: crypto.randomUUID(),
          role: "agent",
          engine,
          text: t("v2.runtime.fakeReady", {
            engine: engine === "claude" ? "Claude" : "Codex",
          }),
        },
      ]);
      setRunning(false);
    }, 650);
  }, [engine, prompt, running, t]);

  const startNewTask = useCallback(() => {
    setEntries([]);
    setPrompt("");
    setRunning(false);
    setHasStarted(false);
    window.dispatchEvent(new Event("vibe-editor2:focus-composer"));
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.isComposing) return;
      const mod = event.metaKey || event.ctrlKey;
      if (mod && event.key.toLowerCase() === "l") {
        event.preventDefault();
        window.dispatchEvent(new Event("vibe-editor2:focus-composer"));
      } else if (mod && event.key.toLowerCase() === "b") {
        event.preventDefault();
        setLeftOpen((current) => !current);
      } else if (mod && event.key === "\\") {
        event.preventDefault();
        setInspectorOpen((current) => !current);
      } else if (mod && event.key === ".") {
        event.preventDefault();
        setRunning(false);
      } else if (event.key === "Escape") {
        setLeftOpen(false);
        setInspectorOpen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);

  return (
    <main className={`v2-shell${hasStarted ? " v2-shell--session" : ""}`}>
      <div className="v2-drag-region" data-tauri-drag-region />
      <nav className="v2-history-actions" aria-label={t("v2.shell.navigation")}>
        <button
          type="button"
          className="v2-history-actions__workspace"
          aria-label={t("v2.drawer.left")}
          onClick={() => setLeftOpen(true)}
        >
          <PanelLeft size={20} strokeWidth={1.65} />
          <i aria-hidden="true" />
        </button>
        <button
          type="button"
          aria-label={t("v2.shell.back")}
          disabled={!hasStarted}
          onClick={() => setHasStarted(false)}
        >
          <ArrowLeft size={20} strokeWidth={1.65} />
        </button>
        <button type="button" aria-label={t("v2.shell.forward")} disabled>
          <ArrowRight size={20} strokeWidth={1.65} />
        </button>
        <button
          type="button"
          aria-label={t("v2.shell.newTask")}
          onClick={startNewTask}
        >
          <SquarePen size={20} strokeWidth={1.65} />
        </button>
      </nav>
      <nav className="v2-window-actions" aria-label={t("v2.shell.views")}>
        <button
          type="button"
          aria-label={t("v2.drawer.left")}
          onClick={() => setLeftOpen(true)}
        >
          <PanelLeft size={21} strokeWidth={1.65} />
        </button>
        <button
          type="button"
          aria-label={t("v2.drawer.inspector")}
          onClick={() => setInspectorOpen(true)}
        >
          <PanelRight size={21} strokeWidth={1.65} />
        </button>
      </nav>

      {!hasStarted ? (
        <section className="v2-home" aria-labelledby="v2-home-title">
          {claudeCheck.state === "missing" && engine === "claude" ? (
            <div className="v2-runtime-setup" role="status">
              <AlertTriangle size={24} />
              <h1>{t("v2.runtime.setupTitle")}</h1>
              <p>{claudeCheck.error || t("v2.runtime.missing")}</p>
              <button type="button" onClick={() => void runClaudeCheck()}>
                {t("v2.runtime.redetect")}
              </button>
            </div>
          ) : (
            <>
              <div className="v2-mark" aria-hidden="true">
                <svg viewBox="140 140 760 720" role="presentation">
                  <path d="M350 721c-94 0-170-76-170-170 0-77 51-142 121-163 6-115 101-206 217-206 91 0 172 56 204 139 79 14 139 83 139 166 0 44-17 84-45 114 2 12 3 24 3 36 0 94-76 170-170 170-51 0-97-22-128-58-31 36-77 58-128 58-70 0-130-42-156-102-25 10-52 16-80 16Z" />
                  <path d="M400 429l74 86-74 86" />
                  <path d="M536 601h126" />
                </svg>
              </div>
              <h1 id="v2-home-title">
                <button type="button" onClick={() => void handleOpenFolder()}>
                  {projectName}
                </button>
                <span>{t("v2.home.question")}</span>
              </h1>
              <div className="v2-quick-actions">
                {QUICK_ACTIONS.map((action) => {
                  const Icon = action.icon;
                  return (
                    <button
                      key={action.key}
                      type="button"
                      className={`v2-quick-action v2-quick-action--${action.tone}`}
                      onClick={() => selectQuickAction(t(action.promptKey))}
                    >
                      <Icon size={22} strokeWidth={1.75} />
                      <span>{t(action.labelKey)}</span>
                    </button>
                  );
                })}
              </div>
            </>
          )}
        </section>
      ) : (
        <section className="v2-timeline" aria-live="polite">
          <header>
            <div>
              <span className={`v2-engine-dot v2-engine-dot--${engine}`} />
              <strong>{projectName}</strong>
            </div>
            <span>
              {engine === "claude" ? "Claude" : "Codex"} · {model}
            </span>
          </header>
          <div className="v2-timeline__body">
            {entries.map((entry) => (
              <article
                key={entry.id}
                className={`v2-message v2-message--${entry.role}`}
              >
                <span>
                  {entry.role === "user"
                    ? t("v2.timeline.you")
                    : entry.engine === "claude"
                      ? "Claude"
                      : "Codex"}
                </span>
                <p>{entry.text}</p>
              </article>
            ))}
            {running && (
              <div
                className="v2-thinking"
                aria-label={t("v2.timeline.running")}
              >
                <i />
                {t("v2.timeline.exploring")}
              </div>
            )}
          </div>
        </section>
      )}

      <div className="v2-composer-wrap">
        <UnifiedComposer
          branch={branch}
          engine={engine}
          model={model}
          permission={permission}
          projectName={projectName}
          prompt={prompt}
          running={running}
          onEngineChange={setEngine}
          onPermissionChange={setPermission}
          onProjectClick={() => void handleOpenFolder()}
          onPromptChange={setPrompt}
          onSubmit={submit}
          onStop={() => setRunning(false)}
        />
      </div>

      {leftOpen && (
        <aside
          className="v2-drawer v2-drawer--left"
          aria-label={t("v2.drawer.left")}
        >
          <header>
            <strong>{t("v2.drawer.workspace")}</strong>
            <button
              type="button"
              aria-label={t("common.close")}
              onClick={() => setLeftOpen(false)}
            >
              <X size={20} />
            </button>
          </header>
          <section>
            <h2>{t("v2.drawer.projects")}</h2>
            <button
              type="button"
              className="v2-drawer-row"
              onClick={() => void handleOpenFolder()}
            >
              <Files size={18} />
              {projectName}
            </button>
          </section>
          <section>
            <h2>{t("v2.drawer.sessions")}</h2>
            <p>
              {entries.length > 0
                ? t("v2.drawer.currentSession")
                : t("v2.drawer.noSessions")}
            </p>
          </section>
          <section>
            <h2>{t("v2.drawer.changedFiles")}</h2>
            {gitStatus?.files.slice(0, 8).map((file) => (
              <div className="v2-drawer-row" key={file.path}>
                <FileCode2 size={17} />
                {file.path}
              </div>
            ))}
          </section>
        </aside>
      )}

      {inspectorOpen && (
        <aside
          className="v2-drawer v2-drawer--right"
          aria-label={t("v2.drawer.inspector")}
        >
          <header>
            <strong>Inspector</strong>
            <button
              type="button"
              aria-label={t("common.close")}
              onClick={() => setInspectorOpen(false)}
            >
              <X size={20} />
            </button>
          </header>
          <div className="v2-inspector-tabs" role="tablist">
            <button role="tab" aria-selected="true">
              Diff
            </button>
            <button role="tab">Test</button>
            <button role="tab">Artifact</button>
            <button role="tab">Raw</button>
          </div>
          <section>
            <TestTube2 size={22} />
            <h2>{t("v2.inspector.results")}</h2>
            <p>{t("v2.inspector.resultsEmpty")}</p>
          </section>
          <button type="button" className="v2-compat-terminal">
            {t("v2.inspector.openTerminal")}
          </button>
        </aside>
      )}
    </main>
  );
}
