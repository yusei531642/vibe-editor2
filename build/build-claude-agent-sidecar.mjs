import { build } from 'esbuild';
import { mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outfile = resolve(root, 'src-sidecars/claude-agent/dist/index.mjs');

await mkdir(dirname(outfile), { recursive: true });
await build({
  entryPoints: [resolve(root, 'src-sidecars/claude-agent/index.mjs')],
  outfile,
  bundle: true,
  format: 'esm',
  platform: 'node',
  target: 'node20',
  sourcemap: false,
  legalComments: 'eof'
});
