# Math on the go

Private, phone-first MST124/MST125 practice. Angular 22, TypeScript, Nx and pnpm on the frontend; Rust, Axum and Tokio on the backend. DynamoDB stores your work. Flashcards use the Rust FSRS engine and learning steps adapted from Moonrabbit; authentication and AWS deployment follow RedRat.

## Practise

1. Open the **Account** menu → **Studied units** (also linked from **Profile**) and mark the units you have covered. This is learning history only, not a practice selection. Prerequisite units must also be studied.
2. **Review cards** uses due/new flashcards. Reveal the answer, then choose **Remembered** or **Not yet**. FSRS updates only flashcards, not tasks. Initial learning reviews can recur after one minute.
3. **Practice** starts with **Everything you've studied** or **Choose specific units**. The default uses your current studied history, not a saved subset. Specific units and optional topic filters apply only to that session and never change studied history. Choose **Tasks**, **Formula flashcards** or **Mixed** directly. Formula cards open immediately from the fixed handbook deck; only tasks are generated and independently checked. **Session options** holds length for all modes, with style/difficulty controls only for tasks. **Next question** consumes the prepared queue. Sessions can be paused and resumed after reloading the same browser.
4. Use staged hints, submit an answer, and reveal the worked explanation. Numerical tasks accept arithmetic, fractions, powers, `sqrt(...)` and `pi`; there is no claim of general symbolic algebra equivalence. Choice answers are exact choices. Conceptual reasoning is recorded before comparing with the model answer and self-assessing.

**Formula flashcards** mode recalls hardcoded handbook formulas, identities and mathematical rules with their assumptions and page references. It never calls AI and does not require a provider key. Stable card IDs preserve FSRS schedules across sessions. Wide formulas and tables scroll inside their cards on phones rather than widening the page. Formula practice prioritizes due cards, then new cards, then future-due cards for extra recall; scheduled **Review cards** uses due/new cards only. Both are finite, distinct-card queues up to your chosen length and ignore task difficulty. **Mixed** uses up to half its slots for available handbook cards and generates tasks for the remaining slots. **New session on this topic** opens task setup with the topic selected; generation starts only when you begin.

Correct answers after assistance are distinct from independent success. Self-assessment, incorrect answers, skips and unfinished attempts stay distinct. Historical question exclusions remain respected without deleting old attempts; the app no longer offers a report-problem action.

**History** filters attempts by unit, topic, kind, difficulty, outcome and dates. Every presented attempt retains the exact question, answer, explanation, diagram, source and verification provenance used at the time. Preparing unseen questions does not create history entries. JSON export includes all matching attempts and immutable action events, preserving legacy reports as historical data. **Progress** shows topic/difficulty counts, coverage and recent independent accuracy rather than an invented mastery score. The recent and previous samples contain at most ten automatically assessed task outcomes each. Flashcard counts remain separate.

Unstarted legacy formula sessions—including failed AI-generated sessions—switch to the fixed deck when resumed. Incompatible sessions that already presented old generated cards are retired with a notice; their attempts and schedules remain in history. Old AI flashcards are not offered as active handbook cards, and their unrelated schedules are not assigned to new formula IDs.

## Content and AI

The fixed deck has **355 formula cards**: **185 from MST124** and **170 from MST125**, covering the 22 represented units and their quick-reference material. `data/questions.json` contains 90 task references used to guide task generation, not a fallback task queue. The supplied MST125 handbook represents its own Units 3–12; Units 1–2 are not invented, and its repeated MST124 chapters do not create duplicate cards. Supported numerical tasks are independently reconstructed and calculated before acceptance.

`data/catalog.json`, `data/questions.json`, `data/formulas-mst124.json` and `data/formulas-mst125.json` are embedded into both Rust binaries. Formula entries contain fixed mathematical content, study prompts, explanations, conditions and 1-based PDF page/heading references. The copyrighted PDFs in `materials/` are not published, uploaded to a model, or needed at runtime. Task questions are original topic-aligned practice; their source references identify the underlying topic rather than an exercise copied from the handbook.

Only task generation incurs OpenRouter usage. All requested task pipelines start in parallel; each pipeline authors its task before independent validation. Formula slots are already selected from the fixed deck. A task or mixed session is published only after all of its generated tasks pass; a failed task does not produce a shorter session or a bank fallback. The backend hardcodes:

- Generator: **Claude Opus 5.5** — `anthropic/claude-opus-5.5`
- Blind independent solver: **GPT 6.1 Sol** — `openai/gpt-6.1-sol`

Models are not configurable through the UI or account settings. Old saved model overrides are ignored without resetting studied history, difficulty or session length.

The verifier sees the problem, not the proposed answer. Supported numerical families are independently reconstructed and calculated; numeric disagreements are rejected. Conceptual answers receive an independent solution and comparison but remain explicitly self-assessed, not certified mathematical proofs. Invalid, ambiguous, unsupported or inconsistent candidates fail preparation. Different models agreeing is not a correctness guarantee.

Task preparation runs asynchronously: locally in the API process, on AWS in a separate worker Lambda. Each model call has a 45-second deadline, complete task preparation has a 360-second deadline, and the worker/stale-session window is 420 seconds. Interrupted sessions are marked failed rather than left running indefinitely. Conditional creation and durable worker claims prevent duplicate generation; generation IDs and queue-state checks prevent stale workers from replacing migrated sessions. Formula sessions do not dispatch workers. Generated tasks and historical exclusions remain private to the account.

## Run locally

Prerequisites:

- Node version in `.node-version`; pnpm 12.4.2.
- Rust stable with `rustfmt` and `clippy`.
- AWS CLI v2.
- Docker when no healthy LocalStack already runs on port 4566.

```sh
pnpm install
pnpm serve
```

Open **http://localhost:8200**. The Angular server proxies `/api` to the Rust API, which listens only on loopback port 8300. LocalStack supplies DynamoDB on port 4566; startup reuses a healthy existing service and creates only the application's `math-on-the-go` table. Authentication is disabled locally using `/config.json` with `auth: null`, and the API uses the explicit local development user. Lambda never accepts that development-user bypass.

The ignored root `.env` holds server-only `OPENROUTER_API_KEY` and an optional `LOCALSTACK_TOKEN`; `.env.example` lists the keys without credentials. The existing app's OpenRouter key has been copied into the local `.env`. Do not commit or place it in frontend configuration. Without a provider key, formula practice and scheduled handbook-card review still work; only sessions requiring newly generated tasks are unavailable.

`DEV_USER` and `TABLE_NAME` may override local defaults for isolated smoke sessions. Stop the command to stop both API and frontend. Persistent learning data remains in DynamoDB; local storage holds only the current browser session's IDs and settings, not the authoritative exercise log. The app needs an internet connection; installation icons/manifest do not imply offline support.

## Checks and builds

```sh
pnpm format
pnpm check
pnpm build
pnpm synth
```

`pnpm check` runs Nx lint/typecheck/test targets, including Rust behavioral tests and real KaTeX rendering of every handbook card's mathematical content. API checks/builds and the rendering regression include the handbook data in their Nx cache inputs. `pnpm build` builds the production Angular app and both ARM64 Linux Lambda binaries. Lambda builds additionally require `cargo-lambda` and Zig. `pnpm synth` depends on those builds and synthesizes the AWS stack; synthesis does not require an OpenRouter key or AWS deployment credentials. For an explicitly credential-free synthesis after a build:

```sh
AWS_EC2_METADATA_DISABLED=true CDK_DEFAULT_ACCOUNT=000000000000 \
  AWS_REGION=us-east-2 pnpm exec cdk synth --strict --quiet
```

## Deploy to AWS

The CDK stack creates private S3 frontend storage, CloudFront HTTPS with its generated `*.cloudfront.net` domain, API Gateway with a Cognito JWT authorizer, invite-only Cognito, retained DynamoDB with point-in-time recovery, an ARM64 API Lambda and a separate AI worker. No custom domain, DNS registration or Route 53 zone is needed. The generated address is emitted as **SiteUrl** and is registered as the Cognito callback/logout origin.

Configure an AWS CLI profile/role with CDK deployment permissions. Bootstrap the account once in the chosen region (default `us-east-2`):

```sh
pnpm exec cdk bootstrap aws://ACCOUNT/us-east-2
pnpm deploy
pnpm exec tsx tools/verify-deployment.ts
```

`STAGE` defaults to `prod`; another stage uses a separate `MathOnTheGo-<stage>` stack and generated domain. `AWS_REGION` selects the region.

Deployment stores the OpenRouter key in Secrets Manager under `math-on-the-go/<stage>/openrouter`. The deploy helper gives AWS CLI a mode-0600 temporary JSON file inside a private temporary directory, never the key in command-line arguments or child environments. The file is removed in `finally`, including on failure; diagnostics expose only error codes, not secret payloads. CloudFormation receives a versioned dynamic reference, not the plaintext key. The API can invoke the worker; only the worker needs the production provider key. Required permissions include secret describe/create/put, CloudFormation secret resolution, CDK bootstrap asset access and normal resource provisioning. Treat deployment access and generated templates as sensitive infrastructure access even though plaintext provider credentials are not embedded.

Create your account using the emitted **UserPoolId**; self-registration is disabled:

```sh
aws cognito-idp admin-create-user \
  --user-pool-id USER_POOL_ID \
  --username YOUR_EMAIL \
  --user-attributes Name=email,Value=YOUR_EMAIL Name=email_verified,Value=true \
  --desired-delivery-mediums EMAIL \
  --region us-east-2
```

Visit **SiteUrl**, complete the temporary-password flow, and sign in. Production uses authorization-code flow with PKCE and refresh-token rotation. DynamoDB/Cognito are retained when the stack is removed; deleting a stack is not a learning-data deletion operation.

### GitHub Actions

`check.yml` runs on pull requests and as a reusable prerequisite of deployment. It runs formatting, lint/type checks, Rust tests, the DynamoDB session-persistence regression against an isolated DynamoDB Local service, production builds, and credential-free CDK synthesis. PR checks do not receive production AWS or provider secrets.

`deploy.yml` deploys pushes to `main`, with manual dispatch also restricted to `main`. Like Moonrabbit, it targets Ohio (`us-east-2`), uses the `production` environment, and reads `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, and `OPENROUTER_API_KEY` from GitHub repository/environment secrets. An OIDC role secret is not required. Deployments are serialized and reuse the frontend plus both ARM64 Lambda artifacts produced by the successful check job.

After deployment, `tools/verify-deployment.ts` checks the generated HTTPS site and runtime configuration, the live Cognito invitation-only policy and callback/logout URLs, and rejection of unauthenticated API requests. It also invokes the deployed ARM64 API through IAM with a fresh synthetic subject to exercise a read-only DynamoDB-backed settings request. It creates no Cognito users or application records and makes no model calls. Results and the generated site URL appear in the deployment job summary. Run the same command locally with deployment-capable AWS credentials to repeat the smoke check.

Opening a PR runs checks only; merging it to `main` starts the production deployment.

## Storage and operating limits

Writes use conditional DynamoDB transactions. Session creation, queue advancement and every attempt action have idempotency IDs. Replaying a successful request does not regenerate questions, open another attempt, apply another hint or grade, or reschedule twice. Historical outcomes cannot be replaced by a later correct answer. Prepared queues and presented attempts are persisted separately; finishing a session retains its exercise history. User keys isolate all records.

History/progress currently read all matching DynamoDB pages and aggregate in memory; this is intentional for a private single-user app. Very large histories/export payloads may eventually need streaming or pre-aggregation. Export periodically if you want a portable copy. AI verification reduces errors but cannot eliminate them.
