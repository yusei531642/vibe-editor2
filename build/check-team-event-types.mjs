#!/usr/bin/env node
// Issue #959: ts-rs で生成した TeamHub event 型が最新かを CI で確認する。

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';

const env = { ...process.env, UPDATE_TEAM_EVENT_TYPES: '1' };
const writeOnly = process.argv.includes('--write');
const generatedPath = 'src/types/generated/team-events.ts';
const before = existsSync(generatedPath) ? readFileSync(generatedPath, 'utf8') : null;

execFileSync(
  'cargo',
  [
    'test',
    '--manifest-path',
    'src-tauri/Cargo.toml',
    'generated_team_event_bindings_are_current',
    '--',
    '--nocapture'
  ],
  { stdio: 'inherit', env }
);

if (writeOnly) {
  console.log('[check-team-event-types] generated src/types/generated/team-events.ts');
  process.exit(0);
}

const after = readFileSync(generatedPath, 'utf8');
if (before !== after) {
  process.stderr.write(
    '[check-team-event-types] src/types/generated/team-events.ts is stale. ' +
      'Run `npm run generate:team-event-types` and commit the generated diff.\n'
  );
  process.exit(1);
}

console.log('[check-team-event-types] OK (generated TeamHub event types are current)');
