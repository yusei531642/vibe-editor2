#!/usr/bin/env node
// Issue #22: ts-rs で生成した runtime event 型が最新かを CI で確認する。

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';

const env = { ...process.env, UPDATE_RUNTIME_EVENT_TYPES: '1' };
const writeOnly = process.argv.includes('--write');
const generatedPath = 'src/types/generated/runtime-events.ts';
const before = existsSync(generatedPath) ? readFileSync(generatedPath, 'utf8') : null;

execFileSync(
  'cargo',
  [
    'test',
    '--manifest-path',
    'src-tauri/Cargo.toml',
    'generated_runtime_event_bindings_are_current',
    '--',
    '--nocapture'
  ],
  { stdio: 'inherit', env }
);

if (writeOnly) {
  console.log('[check-runtime-event-types] generated src/types/generated/runtime-events.ts');
  process.exit(0);
}

const after = readFileSync(generatedPath, 'utf8');
if (before !== after) {
  process.stderr.write(
    '[check-runtime-event-types] src/types/generated/runtime-events.ts is stale. ' +
      'Run `npm run generate:runtime-event-types` and commit the generated diff.\n'
  );
  process.exit(1);
}

console.log('[check-runtime-event-types] OK (generated runtime event types are current)');
