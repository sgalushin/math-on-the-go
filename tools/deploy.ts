import { spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { deploymentStage, loadEnvironment } from './environment';

loadEnvironment();
const stage = deploymentStage();
const key = process.env['OPENROUTER_API_KEY']?.trim();
if (!key)
  throw new Error(
    'Set server-side OPENROUTER_API_KEY in the environment or root .env before deploying.',
  );
const region = process.env['AWS_REGION'] ?? 'us-east-2';
const secretName = `math-on-the-go/${stage}/openrouter`;
const env: NodeJS.ProcessEnv = { ...process.env, AWS_REGION: region, AWS_PAGER: '' };
// Do not pass the key to child environments or command-line arguments.
delete env['OPENROUTER_API_KEY'];
const identity = spawnSync(
  'aws',
  ['sts', 'get-caller-identity', '--query', 'Account', '--output', 'text'],
  {
    env,
    encoding: 'utf8',
  },
);
if (identity.status !== 0)
  throw new Error('AWS CLI authentication failed; configure a deployment profile first.');
env['CDK_DEFAULT_ACCOUNT'] = identity.stdout.trim();
const existing = spawnSync(
  'aws',
  ['secretsmanager', 'describe-secret', '--secret-id', secretName],
  {
    env,
    encoding: 'utf8',
  },
);
if (existing.status !== 0 && !existing.stderr.includes('ResourceNotFoundException')) {
  throw new Error('Unable to inspect the deployment secret; check Secrets Manager permissions.');
}
const create = existing.status !== 0;
const temporary = mkdtempSync(join(tmpdir(), 'math-on-the-go-deploy-'));
let secretVersion: string;
try {
  // AWS CLI opens file:// inputs itself; /dev/stdin cannot reliably reopen Node's pipe.
  const inputFile = join(temporary, 'secret.json');
  writeFileSync(
    inputFile,
    JSON.stringify(
      create
        ? {
            Name: secretName,
            Description: 'Math on the Go server-side OpenRouter API key',
            SecretString: key,
          }
        : { SecretId: secretName, SecretString: key },
    ),
    { mode: 0o600 },
  );
  const stored = spawnSync(
    'aws',
    [
      'secretsmanager',
      create ? 'create-secret' : 'put-secret-value',
      '--cli-input-json',
      `file://${inputFile}`,
      '--query',
      'VersionId',
      '--output',
      'text',
    ],
    { env, encoding: 'utf8' },
  );
  if (stored.status !== 0) {
    // Only report error codes: raw CLI errors can contain the input's secret payload.
    const stderr = stored.stderr ?? '';
    const failure =
      stderr.match(/An error occurred \(([A-Za-z0-9]+)\)/)?.[1] ??
      stderr.match(/\[Errno \d+\]/)?.[0] ??
      `exit ${stored.status ?? 'unknown'}`;
    throw new Error(`Unable to store the deployment key in Secrets Manager (${failure}).`);
  }
  secretVersion = stored.stdout.trim();
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
env['OPENROUTER_SECRET_VERSION'] = secretVersion;
console.log(`Deploying MathOnTheGo-${stage}; server key stored in Secrets Manager.`);
const deployed = spawnSync(
  'pnpm',
  ['exec', 'cdk', 'deploy', '--all', '--require-approval', 'never'],
  { env, stdio: 'inherit' },
);
if (deployed.status !== 0) process.exitCode = deployed.status ?? 1;
