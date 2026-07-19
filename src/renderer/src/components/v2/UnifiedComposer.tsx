import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import {
  ArrowUp,
  Folder,
  GitBranch,
  Laptop,
  Mic,
  Paperclip,
  Plus,
  ShieldCheck,
  Square,
  Target,
  Users,
  X,
} from "lucide-react";
import { useT } from "../../lib/i18n";
import type {
  V2ComposerAttachment,
  V2ComposerIntent,
} from "../../lib/v2-composer-actions";
import type { RuntimeModelOption } from "../../../../types/agent-runtime";
import type { V2PermissionMode } from "../../../../types/shared";

export type V2Engine = "claude" | "codex";
export type V2Permission = V2PermissionMode;

interface UnifiedComposerProps {
  branch: string;
  engine: V2Engine;
  model: string;
  models: RuntimeModelOption[];
  effort: string;
  efforts: string[];
  permission: V2Permission;
  projectName: string;
  prompt: string;
  running: boolean;
  activeGoal: string | null;
  attachments: V2ComposerAttachment[];
  intent: V2ComposerIntent;
  onEngineChange: (engine: V2Engine) => void;
  onModelChange: (model: string) => void;
  onEffortChange: (effort: string) => void;
  onPermissionChange: (permission: V2Permission) => void;
  onProjectClick: () => void;
  onPromptChange: (prompt: string) => void;
  onAttachFile: () => void | Promise<void>;
  onClearGoal: () => void;
  onIntentChange: (intent: V2ComposerIntent) => void;
  onRemoveAttachment: (path: string) => void;
  onSubmit: () => void;
  onStop: () => void;
}

export function UnifiedComposer({
  branch,
  engine,
  model,
  models,
  effort,
  efforts,
  permission,
  projectName,
  prompt,
  running,
  activeGoal,
  attachments,
  intent,
  onEngineChange,
  onModelChange,
  onEffortChange,
  onPermissionChange,
  onProjectClick,
  onPromptChange,
  onAttachFile,
  onClearGoal,
  onIntentChange,
  onRemoveAttachment,
  onSubmit,
  onStop,
}: UnifiedComposerProps): JSX.Element {
  const t = useT();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const actionButtonRef = useRef<HTMLButtonElement>(null);
  const actionMenuRef = useRef<HTMLDivElement>(null);
  const [actionsOpen, setActionsOpen] = useState(false);
  const submitReady = prompt.trim().length > 0
    || (intent === "message" && attachments.length > 0);

  const closeActions = (restoreFocus = false): void => {
    setActionsOpen(false);
    if (restoreFocus) window.requestAnimationFrame(() => actionButtonRef.current?.focus());
  };

  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  useEffect(() => {
    if (!actionsOpen) return;
    actionMenuRef.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
    const handlePointerDown = (event: PointerEvent): void => {
      const target = event.target as Node;
      if (actionMenuRef.current?.contains(target) || actionButtonRef.current?.contains(target)) return;
      closeActions();
    };
    document.addEventListener("pointerdown", handlePointerDown, true);
    return () => document.removeEventListener("pointerdown", handlePointerDown, true);
  }, [actionsOpen]);

  const handleMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const items = Array.from(
      actionMenuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [],
    );
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    let next: number;
    if (event.key === "ArrowDown") next = (current + 1) % items.length;
    else if (event.key === "ArrowUp") next = (current - 1 + items.length) % items.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = items.length - 1;
    else if (event.key === "Escape") {
      event.preventDefault();
      closeActions(true);
      return;
    } else return;
    event.preventDefault();
    items[next]?.focus();
  };

  const selectIntent = (nextIntent: V2ComposerIntent): void => {
    onIntentChange(nextIntent);
    closeActions();
    window.requestAnimationFrame(() => textareaRef.current?.focus());
  };

  const selectAttachment = (): void => {
    closeActions();
    void onAttachFile();
  };

  useEffect(() => {
    const focusComposer = (): void => textareaRef.current?.focus();
    window.addEventListener("vibe-editor2:focus-composer", focusComposer);
    return () =>
      window.removeEventListener("vibe-editor2:focus-composer", focusComposer);
  }, []);

  const handleKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>): void => {
    if (event.nativeEvent.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      onSubmit();
    }
  };

  return (
    <section className="v2-composer" aria-label={t("v2.composer.aria")}>
      <div
        className="v2-composer__context"
        aria-label={t("v2.composer.context")}
      >
        <button
          type="button"
          className="v2-context-item"
          onClick={onProjectClick}
        >
          <Folder size={18} strokeWidth={1.75} />
          <span>{projectName}</span>
        </button>
        <span
          className="v2-context-item"
          aria-label={t("v2.composer.locationAria")}
        >
          <Laptop size={18} strokeWidth={1.75} />
          <span>{t("v2.composer.local")}</span>
        </span>
        <span
          className="v2-context-item"
          aria-label={t("v2.composer.branchAria", { branch })}
        >
          <GitBranch size={18} strokeWidth={1.75} />
          <span>{branch}</span>
        </span>
      </div>

      <textarea
        ref={textareaRef}
        value={prompt}
        onChange={(event) => onPromptChange(event.target.value)}
        onKeyDown={handleKeyDown}
        placeholder={intent === "goal"
          ? t("v2.composer.goalPlaceholder")
          : intent === "team"
            ? t("v2.composer.teamPlaceholder")
            : t("v2.composer.placeholder")}
        rows={2}
        aria-label={t("v2.composer.inputAria")}
      />

      {(intent !== "message" || activeGoal || attachments.length > 0) && (
        <div className="v2-composer__chips" aria-label={t("v2.composer.selectedContext")}>
          {intent !== "message" && (
            <span className={`v2-composer-chip v2-composer-chip--${intent}`}>
              {intent === "goal" ? <Target size={15} /> : <Users size={15} />}
              {intent === "goal" ? t("v2.composer.goalMode") : t("v2.composer.teamMode")}
              <button
                type="button"
                aria-label={t("v2.composer.cancelMode")}
                onClick={() => onIntentChange("message")}
              >
                <X size={14} />
              </button>
            </span>
          )}
          {intent === "message" && activeGoal && (
            <span className="v2-composer-chip v2-composer-chip--active-goal" title={activeGoal}>
              <Target size={15} />
              <span>{activeGoal}</span>
              <button type="button" aria-label={t("v2.composer.clearGoal")} onClick={onClearGoal}>
                <X size={14} />
              </button>
            </span>
          )}
          {attachments.map((attachment) => (
            <span className="v2-composer-chip" key={attachment.path} title={attachment.path}>
              <Paperclip size={15} />
              <span>{attachment.name}</span>
              <button
                type="button"
                aria-label={t("v2.composer.removeAttachment", { name: attachment.name })}
                onClick={() => onRemoveAttachment(attachment.path)}
              >
                <X size={14} />
              </button>
            </span>
          ))}
        </div>
      )}

      <div className="v2-composer__toolbar">
        <div className="v2-composer__group">
          <div className="v2-composer__action-wrap">
            <button
              ref={actionButtonRef}
              type="button"
              className={`v2-icon-button v2-composer__plus${actionsOpen ? " is-open" : ""}`}
              aria-label={t("v2.composer.add")}
              aria-haspopup="menu"
              aria-expanded={actionsOpen}
              aria-controls="v2-composer-actions"
              onClick={() => setActionsOpen((open) => !open)}
            >
              <Plus size={20} strokeWidth={1.75} />
            </button>
            {actionsOpen && (
              <div
                ref={actionMenuRef}
                id="v2-composer-actions"
                className="v2-composer-actions"
                role="menu"
                aria-label={t("v2.composer.addMenu")}
                onKeyDown={handleMenuKeyDown}
              >
                <button type="button" role="menuitem" onClick={selectAttachment}>
                  <Paperclip size={18} strokeWidth={1.75} />
                  <span>{t("v2.composer.attach")}</span>
                </button>
                <button type="button" role="menuitem" onClick={() => selectIntent("goal")}>
                  <Target size={18} strokeWidth={1.75} />
                  <span>{t("v2.composer.createGoal")}</span>
                </button>
                <button type="button" role="menuitem" onClick={() => selectIntent("team")}>
                  <Users size={18} strokeWidth={1.75} />
                  <span>{t("v2.composer.createTeam")}</span>
                </button>
              </div>
            )}
          </div>
          <label className="v2-select-control">
            <ShieldCheck size={18} strokeWidth={1.75} />
            <select
              value={permission}
              disabled={running}
              onChange={(event) =>
                onPermissionChange(event.target.value as V2Permission)
              }
              aria-label={t("v2.composer.permission")}
            >
              <option value="full">{t("v2.permission.full")}</option>
              <option value="agent">{t("v2.permission.agent")}</option>
              <option value="ask">{t("v2.permission.ask")}</option>
            </select>
          </label>
        </div>

        <div className="v2-composer__group v2-composer__group--end">
          <label className="v2-engine-control">
            <select
              value={engine}
              disabled={running}
              onChange={(event) =>
                onEngineChange(event.target.value as V2Engine)
              }
              aria-label={t("v2.composer.engine")}
            >
              <option value="claude">Claude</option>
              <option value="codex">Codex</option>
            </select>
          </label>
          <label className="v2-model-control">
            <span className="sr-only">{t("v2.composer.model")}</span>
            <select
              value={model}
              disabled={running || models.length === 0}
              onChange={(event) => onModelChange(event.target.value)}
              aria-label={t("v2.composer.model")}
            >
              {models.length === 0 ? (
                <option value="">{t("v2.composer.modelLoading")}</option>
              ) : models.map((option) => (
                <option key={option.id} value={option.id} title={option.description}>
                  {option.label}
                </option>
              ))}
            </select>
          </label>
          <label className="v2-effort-control">
            <span className="sr-only">{t("v2.composer.effort")}</span>
            <select
              value={effort}
              disabled={running || efforts.length === 0}
              onChange={(event) => onEffortChange(event.target.value)}
              aria-label={t("v2.composer.effort")}
            >
              {efforts.map((value) => (
                <option key={value} value={value}>{value}</option>
              ))}
            </select>
          </label>
          <button
            type="button"
            className="v2-icon-button"
            aria-label={t("v2.composer.voice")}
          >
            <Mic size={20} strokeWidth={1.75} />
          </button>
          <button
            type="button"
            className="v2-send-button"
            aria-label={running ? t("v2.composer.stop") : t("v2.composer.send")}
            disabled={!running && !submitReady}
            onClick={running ? onStop : onSubmit}
          >
            {running ? (
              <Square size={16} fill="currentColor" />
            ) : (
              <ArrowUp size={22} />
            )}
          </button>
        </div>
      </div>
    </section>
  );
}
