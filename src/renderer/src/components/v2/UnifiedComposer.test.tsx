import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UnifiedComposer } from "./UnifiedComposer";

vi.mock("../../lib/i18n", () => ({
  useT:
    () =>
    (key: string): string =>
      ({
        "v2.composer.inputAria": "指示を入力",
        "v2.composer.send": "送信",
        "v2.composer.stop": "実行を停止",
      })[key] ?? key,
}));

function renderComposer(prompt = "実装してください") {
  const onSubmit = vi.fn();
  const onPromptChange = vi.fn();
  render(
    <UnifiedComposer
      branch="main"
      engine="claude"
      model="Fable 5"
      permission="workspace"
      projectName="vibe-editor2"
      prompt={prompt}
      running={false}
      onEngineChange={vi.fn()}
      onPermissionChange={vi.fn()}
      onProjectClick={vi.fn()}
      onPromptChange={onPromptChange}
      onSubmit={onSubmit}
      onStop={vi.fn()}
    />,
  );
  return { onSubmit, onPromptChange };
}

describe("UnifiedComposer", () => {
  it("Enterで送信し、Shift+Enterでは送信しない", () => {
    const { onSubmit } = renderComposer();
    const textarea = screen.getByRole("textbox", { name: "指示を入力" });
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: true });
    expect(onSubmit).not.toHaveBeenCalled();
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("IME変換中のEnterでは送信しない", () => {
    const { onSubmit } = renderComposer();
    const textarea = screen.getByRole("textbox", { name: "指示を入力" });
    fireEvent.keyDown(textarea, {
      key: "Enter",
      isComposing: true,
      keyCode: 229,
    });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("空入力では送信ボタンを無効化する", () => {
    renderComposer("   ");
    expect(screen.getByRole("button", { name: "送信" })).toBeDisabled();
  });
});
