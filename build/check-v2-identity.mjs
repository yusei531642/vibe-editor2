import { readFileSync } from 'node:fs';

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
const failures = [];

function requireText(path, expected) {
  if (!read(path).includes(expected)) failures.push(`${path}: missing ${expected}`);
}

function forbidText(path, forbidden) {
  if (read(path).includes(forbidden)) failures.push(`${path}: contains v1 identity ${forbidden}`);
}

requireText('package.json', '"name": "vibe-editor2"');
requireText('package.json', '"version": "2.0.0-alpha.0"');
requireText('src-tauri/Cargo.toml', 'name = "vibe-editor2"');
requireText('src-tauri/Cargo.toml', 'name = "vibe_editor2_lib"');
requireText('src-tauri/tauri.conf.json', '"identifier": "com.vibe-editor2.app"');
requireText('src-tauri/tauri.conf.json', 'yusei531642/vibe-editor2/releases');
forbidText('src-tauri/tauri.conf.json', 'yusei531642/vibe-editor/releases');
requireText('src-tauri/src/util/config_paths.rs', 'h.join(".vibe-editor2")');
forbidText('src-tauri/src/util/config_paths.rs', 'h.join(".vibe-editor")');
requireText('src-tauri/src/commands/api_agents.rs', 'KEYRING_SERVICE: &str = "vibe-editor2"');
requireText('src-tauri/src/commands/voice.rs', 'KEYRING_SERVICE: &str = "vibe-editor2"');
requireText('src-tauri/src/mcp_config/claude.rs', 'ENTRY: &str = "vibe-team2"');
requireText('src-tauri/src/mcp_config/codex.rs', 'SECTION: &str = "mcp_servers.vibe-team2"');
requireText('src-tauri/src/team_hub/protocol/consts.rs', 'SPOOL_DIR: &str = ".vibe-team2/tmp"');
requireText('src-tauri/src/commands/api_agents/skills.rs', 'VIBE_TEAM_SKILL_ID: &str = "vibe-team2"');
requireText('src-tauri/src/team_hub/mod.rs', 'vibe-editor2-team-hub-');
requireText('src/renderer/src/stores/ui.ts', "name: 'vibe-editor2:ui'");
requireText('src/renderer/src/stores/canvas-persistence.ts', "'vibe-editor2:canvas'");
requireText('src/renderer/src/lib/i18n/index.ts', "'vibe-editor2:language'");

if (failures.length > 0) {
  console.error(failures.join('\n'));
  process.exit(1);
}

console.log('vibe-editor 2 identity is isolated from v1');
