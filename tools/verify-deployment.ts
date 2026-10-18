import { spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { appendFileSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { deploymentStage, loadEnvironment } from './environment';

loadEnvironment();
const stage = deploymentStage();
const region = process.env['AWS_REGION'] ?? 'us-east-2';
const stackName = `MathOnTheGo-${stage}`;
const env: NodeJS.ProcessEnv = { ...process.env, AWS_REGION: region, AWS_PAGER: '' };
// No provider key is needed by this read-only smoke command or its children.
delete env['OPENROUTER_API_KEY'];
const evidence: string[] = [];

function requireCheck(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function json<T>(text: string, description: string): T {
  try {
    return JSON.parse(text) as T;
  } catch {
    // Do not echo arbitrary remote responses or Lambda errors into CI logs.
    throw new Error(`${description} did not return valid JSON.`);
  }
}

function aws<T>(args: string[]): T {
  const result = spawnSync('aws', [...args, '--region', region, '--output', 'json'], {
    env,
    encoding: 'utf8',
    timeout: 120_000,
    maxBuffer: 4 * 1024 * 1024,
  });
  requireCheck(
    !result.error && result.status === 0,
    `AWS CLI ${args[0]} ${args[1]} failed; check credentials, permissions and AWS CLI availability.`,
  );
  return json<T>(result.stdout, `AWS CLI ${args[0]} ${args[1]}`);
}

function record(message: string): void {
  evidence.push(message);
  console.log(message);
}

async function https(path: string, status: number): Promise<Response> {
  let response: Response;
  try {
    response = await fetch(path, {
      redirect: 'error',
      cache: 'no-store',
      signal: AbortSignal.timeout(30_000),
    });
  } catch {
    throw new Error(`HTTPS request to ${path} failed (redirects are not accepted).`);
  }
  requireCheck(
    response.status === status,
    `HTTPS ${path} returned ${response.status}; expected ${status}.`,
  );
  return response;
}

async function verify(): Promise<void> {
  requireCheck(region === 'us-east-2', 'Production smoke requires AWS_REGION=us-east-2 (Ohio).');
  const stacks = aws<{
    Stacks: {
      StackStatus: string;
      Outputs?: { OutputKey: string; OutputValue: string }[];
    }[];
  }>(['cloudformation', 'describe-stacks', '--stack-name', stackName]);
  requireCheck(stacks.Stacks?.length === 1, `Expected one deployed ${stackName} stack.`);
  const stack = stacks.Stacks[0];
  requireCheck(
    ['CREATE_COMPLETE', 'UPDATE_COMPLETE', 'IMPORT_COMPLETE'].includes(stack.StackStatus),
    `${stackName} is not successfully deployed: ${stack.StackStatus}.`,
  );
  record(`${stackName}: ${stack.StackStatus} in ${region} (Ohio).`);
  const outputs = new Map(
    stack.Outputs?.map(({ OutputKey, OutputValue }) => [OutputKey, OutputValue]),
  );
  const siteUrl = outputs.get('SiteUrl');
  const userPoolId = outputs.get('UserPoolId');
  requireCheck(
    typeof siteUrl === 'string' && /^https:\/\/[a-z0-9]+\.cloudfront\.net$/.test(siteUrl),
    'Stack SiteUrl must be the generated HTTPS CloudFront origin.',
  );
  requireCheck(
    typeof userPoolId === 'string' && /^us-east-2_[A-Za-z0-9]+$/.test(userPoolId),
    'Stack UserPoolId must identify a Cognito pool in Ohio (us-east-2).',
  );
  record(`SiteUrl: ${siteUrl}`);
  record(`UserPoolId: ${userPoolId}`);

  const site = await https(`${siteUrl}/`, 200);
  requireCheck(
    site.headers.get('content-type')?.includes('text/html') &&
      /<!doctype html/i.test(await site.text()),
    'The deployed site did not return an HTML document.',
  );
  record('Generated HTTPS site: HTTP 200 with HTML.');
  const configResponse = await https(`${siteUrl}/config.json`, 200);
  requireCheck(
    configResponse.headers.get('content-type')?.includes('application/json'),
    'Runtime config did not return application/json.',
  );
  const config = json<{
    auth?: { authority?: string; clientId?: string; domain?: string } | null;
  }>(await configResponse.text(), 'Runtime config');
  const auth = config?.auth;
  requireCheck(auth && typeof auth === 'object', 'Runtime Cognito auth is missing or null.');
  requireCheck(
    auth.authority === `https://cognito-idp.${region}.amazonaws.com/${userPoolId}`,
    'Runtime Cognito authority does not match the deployed Ohio user pool.',
  );
  requireCheck(
    typeof auth.clientId === 'string' && /^[a-z0-9]+$/.test(auth.clientId),
    'Runtime Cognito clientId is missing or invalid.',
  );

  const pool = aws<{
    Id: string;
    Domain?: string;
    AdminCreateUserConfig?: { AllowAdminCreateUserOnly?: boolean };
  }>([
    'cognito-idp',
    'describe-user-pool',
    '--user-pool-id',
    userPoolId,
    '--query',
    'UserPool.{Id:Id,Domain:Domain,AdminCreateUserConfig:AdminCreateUserConfig}',
  ]);
  requireCheck(pool.Id === userPoolId, 'Cognito returned a different user pool.');
  requireCheck(
    pool.AdminCreateUserConfig?.AllowAdminCreateUserOnly === true,
    'Cognito public signup is not disabled: AllowAdminCreateUserOnly must be true.',
  );
  requireCheck(
    typeof pool.Domain === 'string' &&
      auth.domain === `https://${pool.Domain}.auth.${region}.amazoncognito.com`,
    'Runtime Cognito hosted-login domain does not match the deployed Ohio pool.',
  );
  record('Runtime Cognito authority/domain match the deployed Ohio pool; public signup disabled.');

  const client = aws<{
    UserPoolId: string;
    ClientId: string;
    CallbackURLs?: string[];
    LogoutURLs?: string[];
  }>([
    'cognito-idp',
    'describe-user-pool-client',
    '--user-pool-id',
    userPoolId,
    '--client-id',
    auth.clientId,
    '--query',
    'UserPoolClient.{UserPoolId:UserPoolId,ClientId:ClientId,CallbackURLs:CallbackURLs,LogoutURLs:LogoutURLs}',
  ]);
  requireCheck(
    client.UserPoolId === userPoolId && client.ClientId === auth.clientId,
    'Runtime Cognito client does not belong to the deployed pool.',
  );
  requireCheck(
    client.CallbackURLs?.length === 1 && client.CallbackURLs[0] === `${siteUrl}/auth/callback`,
    'Cognito callback URLs must contain only the generated-site /auth/callback URL.',
  );
  requireCheck(
    client.LogoutURLs?.length === 1 && client.LogoutURLs[0] === `${siteUrl}/`,
    'Cognito logout URLs must contain only the generated-site root URL.',
  );
  record('Cognito callback/logout URLs match the generated HTTPS site.');

  const unauthorized = await https(`${siteUrl}/api/catalog`, 401);
  await unauthorized.arrayBuffer();
  record('Unauthenticated HTTPS GET /api/catalog: HTTP 401.');

  const resources = aws<{
    StackResourceSummaries: {
      LogicalResourceId: string;
      PhysicalResourceId?: string;
      ResourceType: string;
    }[];
  }>(['cloudformation', 'list-stack-resources', '--stack-name', stackName]);
  // Match the CDK Api construct, not AiWorker or either deployment custom-resource Lambda.
  const apis = resources.StackResourceSummaries?.filter(
    (resource) =>
      resource.ResourceType === 'AWS::Lambda::Function' &&
      /^Api[A-F0-9]{8}$/.test(resource.LogicalResourceId),
  );
  requireCheck(apis?.length === 1, 'Expected exactly one API Lambda in stack resources.');
  const functionName = apis[0].PhysicalResourceId;
  requireCheck(functionName, 'The API Lambda has no physical function ID.');
  const lambda = aws<{ Architectures?: string[]; Runtime?: string; State?: string }>([
    'lambda',
    'get-function-configuration',
    '--function-name',
    functionName,
    '--query',
    '{Architectures:Architectures,Runtime:Runtime,State:State}',
  ]);
  requireCheck(
    lambda.Architectures?.length === 1 && lambda.Architectures[0] === 'arm64',
    'The deployed API Lambda is not ARM64.',
  );
  requireCheck(
    lambda.Runtime === 'provided.al2023' && lambda.State === 'Active',
    'The deployed API Lambda must be Active on provided.al2023.',
  );

  const temporary = mkdtempSync(join(tmpdir(), 'math-on-the-go-smoke-'));
  try {
    const responseFile = join(temporary, 'response.json');
    const requestId = randomUUID();
    // IAM authorizes this direct invoke. This synthetic authorizer context is not a public JWT
    // or a Cognito user: GET settings only executes DynamoDB GetItem and returns defaults.
    const subject = `deployment-smoke-${randomUUID()}`;
    const payload = {
      version: '2.0',
      routeKey: 'ANY /api/{proxy+}',
      rawPath: '/api/settings',
      rawQueryString: '',
      headers: { host: new URL(siteUrl).hostname, 'x-forwarded-proto': 'https' },
      requestContext: {
        accountId: '000000000000',
        apiId: 'deployment-smoke',
        authorizer: { jwt: { claims: { sub: subject }, scopes: [] } },
        domainName: new URL(siteUrl).hostname,
        domainPrefix: 'deployment-smoke',
        http: {
          method: 'GET',
          path: '/api/settings',
          protocol: 'HTTP/1.1',
          sourceIp: '127.0.0.1',
          userAgent: 'math-on-the-go-deployment-smoke',
        },
        requestId,
        routeKey: 'ANY /api/{proxy+}',
        stage: '$default',
        time: new Date().toISOString(),
        timeEpoch: Date.now(),
      },
      isBase64Encoded: false,
    };
    const invocation = aws<{ StatusCode?: number; FunctionError?: string }>([
      'lambda',
      'invoke',
      '--function-name',
      functionName,
      '--invocation-type',
      'RequestResponse',
      '--cli-binary-format',
      'raw-in-base64-out',
      '--payload',
      JSON.stringify(payload),
      '--query',
      '{StatusCode:StatusCode,FunctionError:FunctionError}',
      responseFile,
    ]);
    requireCheck(
      invocation.StatusCode === 200 && !invocation.FunctionError,
      'API Lambda invocation failed; the deployed ARM64 runtime did not complete successfully.',
    );
    const response = json<{
      statusCode?: number;
      headers?: Record<string, string>;
      body?: string;
      isBase64Encoded?: boolean;
    }>(readFileSync(responseFile, 'utf8'), 'API Lambda response');
    requireCheck(
      response.statusCode === 200,
      'API Lambda GET /api/settings did not return HTTP 200.',
    );
    requireCheck(
      Object.entries(response.headers ?? {}).some(
        ([name, value]) =>
          name.toLowerCase() === 'content-type' && value.includes('application/json'),
      ),
      'API Lambda GET /api/settings did not return application/json.',
    );
    requireCheck(typeof response.body === 'string', 'API Lambda settings response has no body.');
    const settings = json<{
      studiedUnitIds?: unknown[];
      defaultDifficulty?: string;
      sessionLength?: number;
    }>(
      response.isBase64Encoded
        ? Buffer.from(response.body, 'base64').toString('utf8')
        : response.body,
      'API settings body',
    );
    requireCheck(
      Array.isArray(settings?.studiedUnitIds) &&
        settings.studiedUnitIds.length === 0 &&
        ['any', 'quick', 'standard', 'stretch'].includes(settings.defaultDifficulty ?? '') &&
        typeof settings.sessionLength === 'number' &&
        Number.isInteger(settings.sessionLength) &&
        settings.sessionLength >= 1 &&
        settings.sessionLength <= 20,
      'API settings response did not contain valid settings for a fresh subject.',
    );
    record(
      `ARM64 API Lambda ${functionName}: IAM invoke GET /api/settings returned HTTP 200 JSON with fresh-subject defaults (DynamoDB GetItem succeeded).`,
    );
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
  record(
    'Read-only smoke passed: no Cognito users, application records or model-provider calls created.',
  );
}

try {
  await verify();
} catch (error) {
  const message = error instanceof Error ? error.message : 'Unexpected verification failure.';
  console.error(`Deployment verification failed: ${message}`);
  evidence.push(`FAILED: ${message}`);
  process.exitCode = 1;
} finally {
  const summary = process.env['GITHUB_STEP_SUMMARY'];
  if (summary) {
    appendFileSync(
      summary,
      `## Production deployment verification\n\n${evidence.map((line) => `- ${line}`).join('\n')}\n`,
    );
  }
}
