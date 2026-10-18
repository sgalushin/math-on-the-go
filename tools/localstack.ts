import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { setTimeout } from 'node:timers/promises';
import { loadEnvironment } from './environment';

const endpoint = 'http://127.0.0.1:4566';

async function healthy(): Promise<boolean> {
  try {
    const response = await fetch(`${endpoint}/_localstack/health`, {
      signal: AbortSignal.timeout(2000),
    });
    if (!response.ok) return false;
    const body = (await response.json()) as { services?: Record<string, string> };
    return ['dynamodb', 's3'].every((service) =>
      ['running', 'available'].includes(body.services?.[service] ?? ''),
    );
  } catch {
    return false;
  }
}

export async function readyInfrastructure(): Promise<void> {
  loadEnvironment();
  if (!(await healthy())) {
    // An existing service owns this port: do not restart it or change another app's container.
    try {
      const response = await fetch(`${endpoint}/_localstack/health`, {
        signal: AbortSignal.timeout(2000),
      });
      if (response.ok) {
        throw new Error(
          'Existing LocalStack on :4566 must enable healthy dynamodb and s3 services.',
        );
      }
    } catch (error) {
      if (error instanceof Error && error.message.startsWith('Existing LocalStack')) throw error;
    }
    const started = spawnSync('docker', ['compose', 'up', '-d', '--wait', 'localstack'], {
      stdio: 'inherit',
      env: process.env,
    });
    if (started.status !== 0)
      throw new Error('Start Docker or provide a healthy LocalStack on :4566.');
  }
  const deadline = Date.now() + 90_000;
  while (!(await healthy())) {
    if (Date.now() >= deadline)
      throw new Error('LocalStack did not become ready within 90 seconds.');
    await setTimeout(1000);
  }
  const env = {
    ...process.env,
    AWS_ACCESS_KEY_ID: 'test',
    AWS_SECRET_ACCESS_KEY: 'test',
    AWS_SESSION_TOKEN: '',
    AWS_DEFAULT_REGION: 'us-east-2',
    AWS_REGION: 'us-east-2',
    AWS_PAGER: '',
  };
  const aws = (args: string[]) =>
    spawnSync('aws', ['--endpoint-url', endpoint, 'dynamodb', ...args], {
      env,
      encoding: 'utf8',
    });
  const existing = aws(['describe-table', '--table-name', 'math-on-the-go']);
  if (existing.error) throw new Error('AWS CLI v2 is required for local table initialization.');
  if (existing.status !== 0) {
    if (!existing.stderr.includes('ResourceNotFoundException')) {
      throw new Error('Cannot inspect the local DynamoDB table; check LocalStack and AWS CLI.');
    }
    const created = aws([
      'create-table',
      '--table-name',
      'math-on-the-go',
      '--attribute-definitions',
      'AttributeName=pk,AttributeType=S',
      'AttributeName=sk,AttributeType=S',
      '--key-schema',
      'AttributeName=pk,KeyType=HASH',
      'AttributeName=sk,KeyType=RANGE',
      '--billing-mode',
      'PAY_PER_REQUEST',
    ]);
    if (created.status !== 0 && !created.stderr.includes('ResourceInUseException')) {
      throw new Error('Cannot create the local math-on-the-go table.');
    }
  }
  if (aws(['wait', 'table-exists', '--table-name', 'math-on-the-go']).status !== 0) {
    throw new Error('The local math-on-the-go table did not become ready.');
  }
  console.log('LocalStack ready; math-on-the-go table available.');
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  readyInfrastructure().catch((error: unknown) => {
    console.error(error instanceof Error ? error.message : 'Local infrastructure startup failed.');
    process.exitCode = 1;
  });
}
