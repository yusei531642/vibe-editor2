import { useEffect, useRef, type KeyboardEvent } from "react";
import {
  ArrowUp,
  Folder,
  GitBranch,
  Laptop,
  Mic,
  Paperclip,
  ShieldCheck,
  Square,
} from "lucide-react";
import { useT } from "../../lib/i18n";

export type V2Engine = "claude" | "codex";
export type V2Permission = "workspace" | "full";

interface UnifiedComposerProps {
  branch: string;
  engine: V2Engine;
  model: string;
  permission: V2Permission;
  projectName: string;
  prompt: string;
  running: boolean;
  onEngineChange: (engine: V2Engine) => void;
  onPermissionChange: (permission: V2Permission) => void;
  onProjectClick: () => void;
  onPromptChange: (prompt: string) => void;
  onSubmit: () => void;
  onStop: () => void;
}

export function UnifiedComposer({
  branch,
  engine,
  model,
  permission,
  projectName,
  prompt,
  running,
  onEngineChange,
  onPermissionChange,
  onProjectClick,
  onPromptChange,
  onSubmit,
  onStop,
}: UnifiedComposerProps): JSX.Element {
  const t = useT();
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

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
        placeholder={t("v2.composer.placeholder")}
        rows={4}
        aria-label={t("v2.composer.inputAria")}
      />

      <div className="v2-composer__toolbar">
        <div className="v2-composer__group">
          <button
            type="button"
            className="v2-icon-button"
            aria-label={t("v2.composer.attach")}
          >
            <Paperclip size={20} strokeWidth={1.75} />
          </button>
          <label className="v2-select-control">
            <ShieldCheck size={18} strokeWidth={1.75} />
            <select
              value={permission}
              onChange={(event) =>
                onPermissionChange(event.target.value as V2Permission)
              }
              aria-label={t("v2.composer.permission")}
            >
              <option value="workspace">{t("v2.permission.workspace")}</option>
              <option value="full">{t("v2.permission.full")}</option>
            </select>
          </label>
        </div>

        <div className="v2-composer__group v2-composer__group--end">
          <label className="v2-engine-control">
            <select
              value={engine}
              onChange={(event) =>
                onEngineChange(event.target.value as V2Engine)
              }
              aria-label={t("v2.composer.engine")}
            >
              <option value="claude">Claude</option>
              <option value="codex">Codex</option>
            </select>
            <span>{model}</span>
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
            disabled={!running && prompt.trim().length === 0}
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
