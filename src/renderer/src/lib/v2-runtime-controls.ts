import type {
  RuntimeEngine,
  RuntimeModelCatalog,
  RuntimeModelOption
} from '../../../types/agent-runtime';

export const V2_REQUEST_TEAM_SCENE_EVENT = 'vibe-editor2:request-team-scene';

const TEAM_NOUN = /(?:チーム|team)/iu;
const TEAM_ACTION =
  /(?:で(?:や|進|作|実装|調査|レビュー)|を(?:作|組|編成|立ち上げ)|として|体制|協力|分担|並列|parallel|collaborat|work\s+together|worker|ワーカー|agents?)/iu;

/** 明示的な Team 要求だけを起動扱いにする。「team の説明」等は通常会話のまま。 */
export function requestsVisibleTeam(input: string): boolean {
  const normalized = input.normalize('NFKC').trim();
  return TEAM_NOUN.test(normalized) && TEAM_ACTION.test(normalized);
}

export function defaultRuntimeModel(catalog: RuntimeModelCatalog): RuntimeModelOption | null {
  return catalog.models.find((model) => model.isDefault) ?? catalog.models[0] ?? null;
}

export function engineLabel(engine: RuntimeEngine): string {
  return engine === 'claude' ? 'Claude' : 'Codex';
}
