import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
        "v2.composer.add": "追加",
        "v2.composer.addMenu": "追加するものを選択",
        "v2.composer.attach": "ファイルを添付",
        "v2.composer.createGoal": "ゴールを作成",
        "v2.composer.createTeam": "チームを作成",
        "v2.composer.cancelMode": "作成モードを解除",
        "v2.composer.clearGoal": "現在のゴールを解除",
      })[key] ?? key,
}));

function renderComposer(
  prompt = "実装してください",
  intent: "message" | "goal" | "team" = "message",
  attachments: Array<{ path: string; name: string }> = [],
) {
  const onSubmit = vi.fn();
  const onPromptChange = vi.fn();
  const onAttachFile = vi.fn();
  const onIntentChange = vi.fn();
  const onRemoveAttachment = vi.fn();
  const result = render(
    <UnifiedComposer
      branch="main"
      engine="claude"
      model="fable"
      models={[{
        id: "fable",
        label: "Fable",
        description: "Claude Fable",
        isDefault: true,
        defaultEffort: "high",
        supportedEfforts: ["low", "high"]
      }]}
      effort="high"
      efforts={["low", "high"]}
      permission="workspace"
      projectName="vibe-editor2"
      prompt={prompt}
      running={false}
      activeGoal={null}
      attachments={attachments}
      intent={intent}
      onEngineChange={vi.fn()}
      onModelChange={vi.fn()}
      onEffortChange={vi.fn()}
      onPermissionChange={vi.fn()}
      onProjectClick={vi.fn()}
      onPromptChange={onPromptChange}
      onAttachFile={onAttachFile}
      onClearGoal={vi.fn()}
      onIntentChange={onIntentChange}
      onRemoveAttachment={onRemoveAttachment}
      onSubmit={onSubmit}
      onStop={vi.fn()}
    />,
  );
  return { ...result, onSubmit, onPromptChange, onAttachFile, onIntentChange, onRemoveAttachment };
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

  it("通常会話は添付だけで送信でき、GoalとTeamは説明入力を必須にする", () => {
    const attachment = [{ path: "/tmp/spec.md", name: "spec.md" }];
    const message = renderComposer("", "message", attachment);
    expect(screen.getByRole("button", { name: "送信" })).toBeEnabled();
    message.unmount();
    const goal = renderComposer("", "goal", attachment);
    expect(screen.getByRole("button", { name: "送信" })).toBeDisabled();
    goal.unmount();
    renderComposer("", "team", attachment);
    expect(screen.getByRole("button", { name: "送信" })).toBeDisabled();
  });

  it("＋メニューから添付・ゴール・チームの各経路を選べる", async () => {
    const { onAttachFile, onIntentChange } = renderComposer();
    fireEvent.click(screen.getByRole("button", { name: "追加" }));
    const menu = screen.getByRole("menu", { name: "追加するものを選択" });
    expect(menu).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("menuitem", { name: "ファイルを添付" })).toHaveFocus());
    fireEvent.click(screen.getByRole("menuitem", { name: "ファイルを添付" }));
    expect(onAttachFile).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "追加" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "ゴールを作成" }));
    expect(onIntentChange).toHaveBeenCalledWith("goal");

    fireEvent.click(screen.getByRole("button", { name: "追加" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "チームを作成" }));
    expect(onIntentChange).toHaveBeenCalledWith("team");
  });

  it("メニューを矢印キーで移動しEscapeで閉じる", async () => {
    renderComposer();
    const add = screen.getByRole("button", { name: "追加" });
    fireEvent.click(add);
    const first = screen.getByRole("menuitem", { name: "ファイルを添付" });
    await waitFor(() => expect(first).toHaveFocus());
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(screen.getByRole("menuitem", { name: "ゴールを作成" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "Escape" });
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });
});
