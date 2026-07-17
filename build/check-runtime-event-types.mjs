#!/usr/bin/env node
// Issue #22: ts-rs で生成した runtime event 型が最新かを CI で確認する。

import { execFileSync } from 'node:child_process';

const env = { ...process.env, UPDATE_RUNTIME_EVENT_TYPES: '1' };
const writeOnly = process.argv.includes('--write');

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

try {
  execFileSync('git', ['diff', '--exit-code', '--', 'src/types/generated/runtime-events.ts'], {
    stdio: 'pipe'
  });
} catch (error) {
  process.stderr.write(
    '[check-runtime-event-types] src/types/generated/runtime-events.ts is stale. ' +
      'Run `npm run generate:runtime-event-types` and commit the generated diff.\n'
  );
  if (error.stdout) process.stderr.write(error.stdout);
  if (error.stderr) process.stderr.write(error.stderr);
  process.exit(1);
}

console.log('[check-runtime-event-types] OK (generated runtime event types are current)');
