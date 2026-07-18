import { readFileSync } from 'node:fs';

const ci = readFileSync('.github/workflows/ci.yml', 'utf8');
const release = readFileSync('.github/workflows/release.yml', 'utf8');

const failures = [];
const requireMatch = (source, pattern, message) => {
  if (!pattern.test(source)) failures.push(message);
};

requireMatch(ci, /^\s{2}workflow_call:\s*$/m, 'ci.yml must support workflow_call');
for (const command of [
  'npm run typecheck',
  'npm test',
  'cargo clippy --locked',
  'cargo test --locked',
]) {
  if (!ci.includes(command)) failures.push(`ci.yml quality gate is missing: ${command}`);
}

requireMatch(release, /^\s{2}validate-release-ref:\s*$/m, 'release.yml must validate the release ref');
requireMatch(release, /\[\[ "\$\{GITHUB_REF\}" != refs\/tags\/v\* \]\]/, 'release ref must be restricted to v* tags');
requireMatch(release, /git merge-base --is-ancestor "\$\{GITHUB_SHA\}" origin\/main/, 'release commit must be an ancestor of main');
requireMatch(release, /^\s{2}quality-gate:\s*\r?\n(?:.|\r?\n)*?uses: \.\/\.github\/workflows\/ci\.yml/m, 'release.yml must call the reusable CI workflow');
requireMatch(release, /^\s{2}build:\s*\r?\n\s{4}needs:\s*\r?\n\s{6}- validate-release-ref\s*\r?\n\s{6}- quality-gate\s*$/m, 'build must depend on ref validation and the quality gate');
requireMatch(release, /- Linux:\s+`\.AppImage` \/ `\.deb` \/ `\.rpm`/, 'release body must list the RPM artifact');
requireMatch(release, /group:\s*release-pipeline/, 'release runs must be serialized before publishing the updater channel');
requireMatch(release, /^\s{2}publish-release-and-update-channel:\s*$/m, 'release.yml must publish the release and fixed updater channel');
requireMatch(release, /publish-release-and-update-channel:\s*\r?\n\s{4}needs:\s*\r?\n\s{6}- validate-release-ref\s*\r?\n\s{6}- quality-gate\s*\r?\n\s{6}- build/m, 'release and updater publication must depend on every release gate');
requireMatch(release, /gh release download "\$\{GITHUB_REF_NAME\}"/, 'updater channel must use the manifest from the current release');
requireMatch(release, /releases\?per_page=100/, 'draft release lookup must use the authenticated release list');
requireMatch(release, /select\(\.tag_name == \$tag and \.draft\)/, 'draft release lookup must match the current tag exactly');
if (/releases\/tags\/\$\{GITHUB_REF_NAME\}/.test(release)) {
  failures.push('draft release lookup must not use the published-release-only tags endpoint');
}
requireMatch(release, /-F draft=false/, 'the completed draft release must be published before the updater channel');
requireMatch(release, /git\/ref\/heads\/update-channel/, 'the fixed channel branch must be initialized when absent');
requireMatch(release, /-f branch=update-channel/, 'updater manifest must be published to the fixed channel branch');
requireMatch(release, /raw\.githubusercontent\.com\/\$\{GITHUB_REPOSITORY\}\/update-channel\/latest\.json/, 'the public updater channel must be verified');
requireMatch(
  release,
  /prerelease:\s*\$\{\{\s*contains\(github\.ref_name,\s*'-'\)\s*\}\}/,
  'pre-release tags must create GitHub prereleases',
);

const signingStep = release.indexOf('TAURI_SIGNING_PRIVATE_KEY:');
const buildJob = release.indexOf('\n  build:');
if (signingStep === -1 || buildJob === -1 || signingStep < buildJob) {
  failures.push('signing credentials must only be used inside the gated build job');
}

const publishReleaseStep = release.indexOf('-F draft=false');
const publishChannelStep = release.indexOf('-f branch=update-channel');
if (publishReleaseStep === -1 || publishChannelStep === -1 || publishReleaseStep > publishChannelStep) {
  failures.push('the release must become public before the updater channel is advanced');
}

if (failures.length > 0) {
  console.error('Release workflow contract check failed:');
  for (const failure of failures) console.error(`- ${failure}`);
  process.exit(1);
}

console.log('Release workflow contract check passed.');
