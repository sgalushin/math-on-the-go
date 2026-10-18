import { spawnSync } from 'node:child_process';
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
// Do not pass the key to child environments or command-line arguments. Only AWS CLI stdin receives it.
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
const stored = spawnSync(
  'aws',
  [
    'secretsmanager',
    create ? 'create-secret' : 'put-secret-value',
    '--cli-input-json',
    'file:///dev/stdin',
    '--query',
    'VersionId',
    '--output',
    'text',
  ],
  {
    env,
    encoding: 'utf8',
    input: JSON.stringify(
      create
        ? {
            Name: secretName,
            Description: 'Math on the Go server-side OpenRouter API key',
            SecretString: key,
          }
        : { SecretId: secretName, SecretString: key },
    ),
  },
);
// Never forward provider output/errors that could include a secret payload.
if (stored.status !== 0) throw new Error('Unable to store the deployment key in Secrets Manager.');
env['OPENROUTER_SECRET_VERSION'] = stored.stdout.trim();
console.log(`Deploying MathOnTheGo-${stage}; server key stored in Secrets Manager.`);
const deployed = spawnSync(
  'pnpm',
  ['exec', 'cdk', 'deploy', '--all', '--require-approval', 'never'],
  { env, stdio: 'inherit' },
);
if (deployed.status !== 0) process.exitCode = deployed.status ?? 1;
