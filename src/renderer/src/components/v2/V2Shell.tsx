import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
import { useV2RuntimeCatalog } from "../../lib/hooks/use-v2-runtime-catalog";
import { useV2RuntimeSession } from "../../lib/hooks/use-v2-runtime-session";
import { requestsVisibleTeam, V2_REQUEST_TEAM_SCENE_EVENT } from "../../lib/v2-runtime-controls";
import { reportV2RuntimeActionError } from "../../lib/v2-runtime-action";
import { useCanvasStore } from "../../stores/canvas";
import { useUiStore } from "../../stores/ui";
import { launchV2Team } from "../../lib/v2-team-launch";
import { V2Timeline, type V2TimelineEntry } from "./V2Timeline";
import {
  UnifiedComposer,
  type V2Engine,
  type V2Permission,
} from "./UnifiedComposer";
import { TeamInspector } from "./TeamInspector";
import { useTeamProjection } from "./TeamProjectionProvider";

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

export interface V2ShellProps {
  shortcutsEnabled?: boolean;
}

export function V2Shell({ shortcutsEnabled = true }: V2ShellProps = {}): JSX.Element {
  const t = useT();
  const { projectRoot, handleOpenFolder, gitStatus } = useProject();
  const { claudeCheck, runClaudeCheck } = useTeam();
  const [engine, setEngine] = useState<V2Engine>("claude");
  const [model, setModel] = useState("");
  const [effort, setEffort] = useState("");
  const [permission, setPermission] = useState<V2Permission>("workspace");
  const [prompt, setPrompt] = useState("");
  const [teamStarting, setTeamStarting] = useState(false);
  const [hasStarted, setHasStarted] = useState(false);
  const [leftOpen, setLeftOpen] = useState(false);
  const [standaloneInspectorOpen, setStandaloneInspectorOpen] = useState(false);
  const teamProjection = useTeamProjection();
  const hasTeamProjection = teamProjection.sessionActive;
  const inspectorOpen = hasTeamProjection
    ? teamProjection.inspectorOpen
    : standaloneInspectorOpen;
  const setInspectorOpen = useCallback(
    (open: boolean | ((current: boolean) => boolean)) => {
      const next = typeof open === "function" ? open(inspectorOpen) : open;
      if (hasTeamProjection) teamProjection.setInspectorOpen(next);
      else setStandaloneInspectorOpen(next);
    },
    [hasTeamProjection, inspectorOpen, teamProjection],
  );
  const [entries, setEntries] = useState<V2TimelineEntry[]>([]);
  const restoredTimelineHydratedRef = useRef(false);
  const activeAgentEntryIdRef = useRef<string | null>(null);
  const addCard = useCanvasStore((state) => state.addCard);
  const setWorkspaceTeamId = useUiStore((state) => state.setWorkspaceTeamId);
  const catalog = useV2RuntimeCatalog(engine);

  const onRuntimeDelta = useCallback((delta: string, runtimeEngine: V2Engine) => {
    let entryId = activeAgentEntryIdRef.current;
    if (!entryId) {
      entryId = crypto.randomUUID();
      activeAgentEntryIdRef.current = entryId;
      const entry: V2TimelineEntry = { id: entryId, role: "agent", text: delta, engine: runtimeEngine };
      setEntries((current) => [...current, entry]);
      return;
    }
    setEntries((current) => current.map((entry) =>
      entry.id === entryId ? { ...entry, text: entry.text + delta } : entry
    ));
  }, []);

  const onRuntimeComplete = useCallback((message: string, runtimeEngine: V2Engine) => {
    const entryId = activeAgentEntryIdRef.current;
    if (entryId) {
      setEntries((current) => current.map((entry) =>
        entry.id === entryId ? { ...entry, text: message } : entry
      ));
    } else {
      setEntries((current) => [...current, {
        id: crypto.randomUUID(), role: "agent", text: message, engine: runtimeEngine
      }]);
    }
    activeAgentEntryIdRef.current = null;
  }, []);

  const onRuntimeError = useCallback((message: string, runtimeEngine: V2Engine) => {
    activeAgentEntryIdRef.current = null;
    setEntries((current) => [...current, {
      id: crypto.randomUUID(), role: "agent", text: message, engine: runtimeEngine
    }]);
  }, []);

  const runtime = useV2RuntimeSession({
    onDelta: onRuntimeDelta,
    onComplete: onRuntimeComplete,
    onError: onRuntimeError
  });
  const running = runtime.running || teamStarting;

  const projectName = useMemo(() => {
    if (!projectRoot) return t("v2.project.select");
    return projectRoot.split(/[\\/]/).filter(Boolean).at(-1) ?? projectRoot;
  }, [projectRoot, t]);
  const branch = gitStatus?.branch || "main";
  const selectedModel = catalog.models.find((candidate) => candidate.id === model)
    ?? catalog.models[0]
    ?? null;
  const efforts = selectedModel?.supportedEfforts ?? [];
  const modelLabel = (selectedModel?.label ?? model) || "—";

  useEffect(() => {
    if (catalog.models.length === 0) return;
    const nextModel = catalog.models.find((candidate) => candidate.id === model)
      ?? catalog.models[0];
    if (nextModel.id !== model) setModel(nextModel.id);
    if (!nextModel.supportedEfforts.includes(effort)) {
      setEffort(nextModel.defaultEffort || nextModel.supportedEfforts[0] || "");
    }
  }, [catalog.models, effort, model]);

  useEffect(() => {
    if (restoredTimelineHydratedRef.current || entries.length > 0) return;
    const restored = teamProjection.projection.agents
      .flatMap((agent) =>
        (agent.runtime?.eventHistory ?? []).flatMap((event) =>
          event.payload.type === "messageComplete"
            ? [{
                id: `restore:${event.endpointId}:${event.epoch}:${event.sequence}`,
                role: "agent" as const,
                text: event.payload.message,
                engine: agent.endpoint?.provider === "codex-native" ? "codex" as const : "claude" as const,
                timestamp: event.timestamp,
              }]
            : [],
        ),
      )
      .sort((left, right) => left.timestamp.localeCompare(right.timestamp));
    if (restored.length === 0) return;
    restoredTimelineHydratedRef.current = true;
    setEntries(restored.map(({ timestamp: _timestamp, ...entry }) => entry));
    setHasStarted(true);
  }, [entries.length, teamProjection.projection.agents]);

  const selectQuickAction = (template: string): void => {
    setPrompt((current) =>
      current.trim() ? `${current}\n\n${template}` : template,
    );
    window.dispatchEvent(new Event("vibe-editor2:focus-composer"));
  };

  const launchTeam = useCallback(async (text: string): Promise<void> => {
    if (!projectRoot) throw new Error(t("v2.runtime.projectRequired"));
    await runtime.reset();
    await launchV2Team({
      projectRoot,
      teamName: t("v2.team.defaultName"),
      initialMessage: text,
      engine,
      model,
      effort,
      permission,
      setupTeamMcp: window.api.app.setupTeamMcp,
      addCard,
      selectTeam: setWorkspaceTeamId,
      requestTeamScene: () => window.requestAnimationFrame(() => {
        window.dispatchEvent(new Event(V2_REQUEST_TEAM_SCENE_EVENT));
      })
    });
  }, [addCard, effort, engine, model, permission, projectRoot, runtime, setWorkspaceTeamId, t]);

  const submit = useCallback(() => {
    const text = prompt.trim();
    if (!text || running) return;
    const entry: V2TimelineEntry = {
      id: crypto.randomUUID(),
      role: "user",
      text,
      engine,
    };
    setEntries((current) => [...current, entry]);
    setPrompt("");
    setHasStarted(true);
    activeAgentEntryIdRef.current = null;
    if (requestsVisibleTeam(text)) {
      setTeamStarting(true);
      void launchTeam(text).catch((error) => {
        onRuntimeError(error instanceof Error ? error.message : String(error), engine);
      }).finally(() => setTeamStarting(false));
      return;
    }
    void runtime.send({ input: text, engine, model, effort, permission }).catch(() => undefined);
  }, [effort, engine, launchTeam, model, onRuntimeError, permission, prompt, running, runtime]);

  const stopRun = useCallback(() => {
    if (teamStarting) return;
    void reportV2RuntimeActionError(runtime.stop(), engine, onRuntimeError);
  }, [engine, onRuntimeError, runtime, teamStarting]);

  const startNewTask = useCallback(() => {
    if (teamStarting) return;
    void runtime.reset();
    setEntries([]);
    setPrompt("");
    setHasStarted(false);
    activeAgentEntryIdRef.current = null;
    window.dispatchEvent(new Event("vibe-editor2:focus-composer"));
  }, [runtime, teamStarting]);

  useEffect(() => {
    if (!shortcutsEnabled) return;
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
        stopRun();
      } else if (event.key === "Escape") {
        setLeftOpen(false);
        setInspectorOpen(false);
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [setInspectorOpen, shortcutsEnabled, stopRun]);

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
          onClick={startNewTask} disabled={teamStarting}
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
        <V2Timeline
          projectName={projectName}
          engine={engine}
          modelLabel={modelLabel}
          effort={effort}
          entries={entries}
          running={running}
          pendingApproval={runtime.pendingApproval}
          onApproval={(decision) => void reportV2RuntimeActionError(
            runtime.respondApproval(decision), engine, onRuntimeError)}
        />
      )}

      <div className="v2-composer-wrap">
        <UnifiedComposer
          branch={branch}
          engine={engine}
          model={model}
          models={catalog.models}
          effort={effort}
          efforts={efforts}
          permission={permission}
          projectName={projectName}
          prompt={prompt}
          running={running}
          onEngineChange={(nextEngine) => {
            setEngine(nextEngine);
            setModel("");
            setEffort("");
          }}
          onModelChange={setModel}
          onEffortChange={setEffort}
          onPermissionChange={setPermission}
          onProjectClick={() => void handleOpenFolder()}
          onPromptChange={setPrompt}
          onSubmit={submit}
          onStop={stopRun}
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

      {inspectorOpen && hasTeamProjection ? (
        <TeamInspector />
      ) : inspectorOpen ? (
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
      ) : null}
    </main>
  );
}
