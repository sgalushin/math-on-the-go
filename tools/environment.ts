import { existsSync } from 'node:fs';

export function loadEnvironment(): void {
  if (existsSync('.env')) process.loadEnvFile('.env');
}

export function deploymentStage(): string {
  const stage = process.env['STAGE'] ?? 'prod';
  if (!/^[a-z][a-z0-9-]{0,20}$/.test(stage)) {
    throw new Error('STAGE must be a short lowercase name, such as prod.');
  }
  return stage;
}
