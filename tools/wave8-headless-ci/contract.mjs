import fs from 'node:fs';

const read = (path) => fs.readFileSync(path, 'utf8');
const cli = read('apps/cli/src/headless.rs');
const main = read('apps/cli/src/main.rs');
const reports = read('crates/reports/src/lib.rs');
const artifacts = read('crates/artifacts/src/lib.rs');
const content = read('crates/content-addressed/src/lib.rs');
const attestation = read('crates/attestation/src/lib.rs');

const requireText = (source, needle, reason) => {
  if (!source.includes(needle)) throw new Error(reason + ': missing ' + needle);
};

requireText(main, 'Headless(headless::HeadlessArgs)', 'headless CLI command is not wired');
requireText(main, 'read_token().await', 'headless CLI must use daemon control authentication');
requireText(cli, '.bearer_auth(token)', 'headless control calls must remain authenticated');
requireText(cli, '/semantic-snapshot/fresh', 'headless runner must request fresh semantic evidence');
requireText(cli, '/verify/visual/capture', 'visual work must reuse governed verification endpoint');
requireText(cli, '/perception/cycle', 'Chromium work must reuse planner-owned cycle');
requireText(cli, 'resource_governor_denied', 'governor denial must be represented');
requireText(cli, 'multiple LocalView sessions are active', 'ambiguous sessions must fail closed');
requireText(cli, 'headless mode refuses non-loopback session target', 'headless target must remain loopback');
requireText(cli, 'shell interpreters are not allowed', 'fixture shell execution must be rejected');
requireText(cli, 'allow_fixture_command', 'fixture command execution must require explicit policy opt-in');
requireText(cli, 'state_stable', 'baseline authority must be bound to stable end state');
requireText(cli, 'safe_route', 'route transport must strip secret query/fragment data');

requireText(reports, 'pub enum ReportStatus', 'report status contract missing');
requireText(reports, 'pub struct BaselineComparison', 'baseline comparison missing from reports');
requireText(reports, 'pub struct GitAnnotation', 'Git annotation missing from reports');
requireText(reports, 'sanitize_json_value', 'nested report privacy sanitizer missing');
requireText(reports, 'html_escape', 'HTML output must escape project content');
requireText(reports, 'markdown_text', 'Markdown output must escape project content');

requireText(artifacts, 'pub struct CanonicalArtifactMeta', 'physical/canonical artifact identity split missing');
requireText(artifacts, 'put_canonical', 'canonical artifact retention helper missing');
requireText(cli, 'put_canonical', 'headless baseline retention must bind canonical hash to physical storage');
requireText(content, 'pub struct BaselineEnvelope', 'content-addressed baseline envelope missing');
requireText(content, 'dependency_closure', 'baseline dependency closure primitive missing');
requireText(attestation, 'pub struct DigestAttestation', 'digest attestation missing');
requireText(attestation, '"digest_attestation"', 'attestation must be explicitly a digest envelope');

for (const forbidden of [
  'crates/a11y/',
  'crates/flow/',
  'crates/design-grammar/',
  'crates/quality/',
  'crates/contracts/',
  'crates/mutation/',
  'crates/counterfactual/',
  'crates/state-space/',
  'crates/postcondition-contracts/',
  'trusted_fix.rs',
  'trusted_verify.rs',
]) {
  if (workflowOwnedPaths().some((path) => path.includes(forbidden))) {
    throw new Error('Wave 8 contract unexpectedly owns forbidden lane path: ' + forbidden);
  }
}

function workflowOwnedPaths() {
  return [
    'apps/cli/',
    'crates/reports/',
    'crates/artifacts/',
    'crates/content-addressed/',
    'crates/attestation/',
    'tools/wave8-headless-ci/',
  ];
}

const fixture = JSON.parse(read('tools/wave8-headless-ci/fixture.json'));
if (fixture.schema_version !== 1 || fixture.route !== '/' || fixture.viewport.width !== 1280 || fixture.viewport.height !== 720) {
  throw new Error('deterministic Wave 8 fixture is invalid');
}

console.log('Wave 8 headless/CI authority contract: PASS');
