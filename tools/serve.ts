import { spawn, type ChildProcess } from 'node:child_process';
import { readyInfrastructure } from './localstack';

const children: ChildProcess[] = [];
let stopping = false;
function stop(code: number): void {
  if (stopping) return;
  stopping = true;
  process.exitCode = code;
  for (const child of children) {
    if (!child.pid) continue;
    try {
      // Terminate the whole cargo/Nx process tree, not just the launcher.
      process.kill(-child.pid, 'SIGTERM');
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ESRCH') throw error;
    }
  }
}
process.on('SIGINT', () => stop(0));
process.on('SIGTERM', () => stop(0));

try {
  await readyInfrastructure();
  const env: NodeJS.ProcessEnv = {
    ...process.env,
    DEV_USER: process.env['DEV_USER'] ?? 'local',
    TABLE_NAME: process.env['TABLE_NAME'] ?? 'math-on-the-go',
    DYNAMODB_ENDPOINT_URL: 'http://127.0.0.1:4566',
    AWS_ACCESS_KEY_ID: 'test',
    AWS_SECRET_ACCESS_KEY: 'test',
    AWS_SESSION_TOKEN: '',
    AWS_REGION: 'us-east-2',
    AWS_DEFAULT_REGION: 'us-east-2',
    RUST_LOG: 'info',
  };
  // The local API executes AI jobs in-process; never invoke a deployed worker from development.
  delete env['AI_WORKER_FUNCTION'];
  for (const [command, args, cwd] of [
    ['cargo', ['run', '--bin', 'api'], 'apps/api'],
    ['pnpm', ['exec', 'nx', 'serve', 'web'], '.'],
  ] as const) {
    const childEnv = { ...env };
    delete childEnv['LOCALSTACK_TOKEN'];
    if (command === 'pnpm') delete childEnv['OPENROUTER_API_KEY'];
    const child = spawn(command, [...args], {
      cwd,
      env: childEnv,
      stdio: 'inherit',
      detached: true,
    });
    children.push(child);
    child.on('error', () => {
      console.error(`Cannot start ${command}; check the documented prerequisites.`);
      stop(1);
    });
    child.on('exit', (code) => {
      if (!stopping) stop(code ?? 1);
    });
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : 'Local startup failed.');
  stop(1);
}
