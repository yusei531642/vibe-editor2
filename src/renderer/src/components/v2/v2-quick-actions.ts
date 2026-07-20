import { Bug, CodeXml, Hammer, SearchCode } from "lucide-react";

/**
 * 会話ビュー空状態のクイックアクション定義。
 * V2Shell.tsx の file-size ratchet 超過 (Issue #81) に伴い定数を分離した。
 */
export const QUICK_ACTIONS = [
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
