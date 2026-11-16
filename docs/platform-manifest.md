# Manifest: open-source Vercel on infrastructure you own

- **Date:** 2026-10-02
- **Status:** draft, nothing built. Working name TBD; called "the platform" below.
- **Builds on:** ROADMAP.md (AWS Lambda deploy target, durable execution on
  S3/R2), [build-artifact-cache.md](build-artifact-cache.md)

## Purpose

Vercel's deploy experience (push a build, get a URL, roll back instantly,
read your logs) on infrastructure **you own and pay for directly**. Like
Coolify, the platform is software you run against your own resources. Unlike
Coolify, the first target is serverless (AWS Lambda), not a VPS. Your own
hardware comes later, as a second target behind the same CLI.

Why: Vercel's free-tier logs are too thin to operate on. Coolify and Dokploy
manage long-running containers on servers, and neither deploys to Lambda.
SST is the closest thing, but it's a general IaC framework, not a
"deploy, URL, logs" product.

**There is no hosted control plane.** The platform is a CLI plus a handful
of resources in your AWS account. All state lives in that account, so there
is nothing to run, pay for, or trust beyond AWS itself.

## Principles

1. **Your account, your bill.** No middleman service, and no platform
   resources outside the account.
2. **Deploys take seconds.** Builds happen before deploy (locally, in CI, or
   from the artifact cache). A deploy uploads and flips a pointer, never
   compiles.
3. **Deployments are immutable.** Every deploy is a new Lambda version with
   its own URL. Promote and rollback just move an alias.
4. **Logs are first-class.** Querying logs from the CLI is a day-one
   feature, not an add-on.
5. **Boring AWS primitives.** Lambda, S3, CloudFront, EventBridge, SSM,
   CloudWatch. Nothing proprietary to the platform sits in the request path.
6. **Runtime-agnostic contract.** Anything that serves HTTP on `$PORT` works,
   via the Lambda Web Adapter. nextrs is the first and best-supported
   consumer, not the only one. It gets a tighter, first-party runtime adapter
   for background work (see below).

## The basics (v0 feature set)

The minimum that makes it "a Vercel," mapped to the AWS primitive behind each
piece:

| Vercel feature | Platform equivalent | AWS primitive |
|---|---|---|
| Deploy a prebuilt output | `deploy` uploads a zip, publishes a version | Lambda (`provided.al2023`, arm64) + Web Adapter layer |
| Production URL | `prod` alias → Function URL | Lambda alias + Function URL |
| Custom domain + TLS | `domains add` | CloudFront + ACM (DNS stays wherever it is) |
| Static assets / CDN | content-hashed files, cached forever | S3 + CloudFront behavior on `/dist/*`, `/public/*` |
| Instant rollback | `rollback` / `promote <id>` repoint the alias | Lambda alias update |
| Env vars / secrets | `env set/pull`, per environment | SSM Parameter Store → Lambda env at deploy |
| Logs | `logs [--follow] [--route] [--status 5xx]` | CloudWatch Logs (JSON format) + Logs Insights |
| Basic metrics | `stats`: requests, errors, p50/p99, cold starts by route | Logs Insights over structured logs |
| Crons | from `nextrs.toml` / platform config | EventBridge Scheduler → Function URL with secret |

**Deliberately not in v0:** preview deployments per branch, a web
dashboard, multiple regions, teams/auth, build servers, the own-hardware
target.

## Accounts, projects, and isolation

**Default: one dedicated platform account, with projects isolated inside it.**
The platform account is a member account in an AWS Organization, and nothing
else lives there. The deploy target is `(account, region)` in config, so a
project can move to its own account later without redesign.

Account-per-project is supported but not the default:
- New accounts can start with a reduced Lambda concurrency quota (as low as
  10) until an increase is granted.
- The free tier is shared across an Organization, so extra accounts don't add
  free usage.
- Each account needs its own bootstrap, budget alarm, and SSO setup, and
  account closure is rate-limited.
- Use one account per project for different clients or trust levels.
  Cost-allocation tags already give per-project bills.

**Hierarchy:**

| Concept | AWS mapping |
|---|---|
| Project | One CloudFormation stack `nx-<project>`: `nx-<project>-web` + `-tasks` functions, CloudFront distribution, log groups, state prefix |
| Environment | Lambda alias (`prod`; per-branch preview aliases in v1) |
| Deployment | Immutable Lambda version |
| Region | Per project |

**Fencing (it works the same in a dedicated account or a shared, everyday
account):**

1. **Names and tags.** Every resource is `nx-<project>-*`, tagged
   `nx:project` and `nx:managed=true`. The tags are activated for cost
   allocation.
2. **A scoped deployer role.** Bootstrap creates `nx-deployer`, and the CLI
   only ever assumes it. Its policy is limited to `function:nx-*`,
   `s3:::nx-*`, the `nx-*` log groups, `stack/nx-*`, and `/nx/*` SSM
   parameters.
3. **A permission boundary.** The deployer can only create or pass roles
   under the IAM path `/nx/` that carry the `nx-boundary` boundary (enforced
   by an `iam:PermissionsBoundary` condition). No deploy can escalate past
   the boundary.
4. **Per-project function roles.** Each project reads only its own
   `/nx/<project>/*` parameters and `state/<project>/` prefix.
5. **Quota fencing.** All projects share the regional concurrency pool, so
   each function gets reserved concurrency. Outgrowing this is the signal to
   move a project to its own account.
6. **Optional SCP guardrail.** On the platform account, deny everything
   outside the allowed regions and outside `nx-*` resources.

## How it works

### Bootstrap: once per account, then once per project

**Account (`bootstrap`)** is one CloudFormation stack, `nx-platform`:

- State/artifact bucket `nx-state-<account-id>`. S3 or R2; it also serves
  as the [build artifact cache](build-artifact-cache.md).
- The `nx-deployer` role and the `nx-boundary` permission boundary (see
  fencing above).
- A budget alarm.

**Project (`init`)** is one stack per project, `nx-<project>`, so teardown
is one command:

- `nx-<project>-web` and `nx-<project>-tasks` functions with roles under
  `/nx/` (Web Adapter layer for non-nextrs apps; nextrs's own runtime
  adapter for nextrs apps).
- CloudFront distribution: default origin = the `prod` alias's Function URL,
  plus an S3 origin for static assets.
- CloudWatch log groups with explicit retention (default 14 days).

### Every deploy (`deploy`)

Direct AWS SDK calls (`aws-sdk-rust`), not IaC, so it finishes in seconds:

1. Resolve the artifact: the output-cache hit by key, or the local
   `.vercel/output`-style build.
2. Upload static assets to S3. They're content-hashed, so uploads only add
   files, never overwrite.
3. `UpdateFunctionCode` + `UpdateFunctionConfiguration` (env from SSM), then
   `PublishVersion`. That version is the immutable deployment.
4. Record the deployment in the state bucket (below).
5. Production deploys move the `prod` alias, which is the promotion. Without
   `--prod`, the version gets its own Function URL for testing.

### State: the account is the database

There's no platform database. Deployment history lives in the state bucket:

```text
state/<app>/deployments/<id>.json   # version, commit, artifact key, builder, time (write-once)
state/<app>/current.json            # { prod: <id> } — updated by CAS (If-Match)
```

These are the same S3 conditional writes the durable-execution and artifact
cache designs use. Concurrent `promote` calls can't clobber each other.

### Logs and observability

- **Contract for apps:** structured JSON logs to stdout with `route`,
  `status`, `duration_ms`, and `request_id`. nextrs emits this from its
  router, and other runtimes get it from a small middleware.
- **The CLI ships canned Logs Insights queries**, so `logs` and `stats` work
  without writing queries.
- **Escape hatch:** a subscription filter to Axiom or Grafana Cloud when
  CloudWatch's UI isn't enough.

### Background work: `WaitUntil` and tasks (decided 2026-10-02: tight coupling)

The platform and nextrs are deliberately coupled here. nextrs apps on Lambda
run under **nextrs's own runtime adapter** (`lambda_runtime`), not the Web
Adapter, because background work needs control of the invocation lifecycle.
The Web Adapter remains the path for non-nextrs apps.

- **Tier 1: `wait_until(future)`, unchanged API.** The adapter posts the
  response, then drains pending futures before asking for the next event. The
  client already has its response while the instance keeps working. This
  matches what `vercel_runtime` provides today.
  - **Spike first:** confirm the response reaches the caller before `/next`,
    and measure the billed duration.
  - **Why not reuse `vercel_runtime`'s mechanism:** checked 2026-10-02
    against v2.4.0. It isn't a Lambda Runtime API client. It's an HTTP
    server on `127.0.0.1` that talks to Vercel's closed host over a
    unix-socket IPC (`VERCEL_IPC_PATH`, start/end messages). Its `Awaiter`
    drains `wait_until` futures at SIGTERM, which only works because
    Vercel's Fluid host keeps instances thawed between requests and signals
    shutdown. On plain Lambda the instance freezes after `/next`, and the
    runtime only sees SIGTERM when an extension is registered, with a short
    window.
  - **What we copy and what we change:** copy the `Awaiter` semantics (spawn
    immediately, swallow panics, loop until the set is empty), but drain per
    invocation between `/response` and `/next`. This is the custom-runtime
    pattern in AWS's "Running code after returning a response" post.
  - **The cost:** the instance takes no new invocation until the drain ends,
    and that time is billed. Fine for short work; anything long goes to
    tier 2.
  - **Owning the loop removes our Vercel patches.** `nextrs::lambda` is a
    small Runtime API client (`/next`, `/response` buffered or streaming,
    `/error`), shaped like `nextrs::vercel`. With no upstream adapter in the
    middle, the `StreamingVercelLayer` HTML-streaming workaround and the
    `AppState` re-plumbing aren't needed.
- **Tier 2: `#[nextrs::task]`, fired to another Lambda.** Arbitrary futures
  can't cross a process boundary, so this is a typed, serializable task:

  ```rust
  #[nextrs::task]
  async fn send_welcome(args: Welcome) -> anyhow::Result<()> { /* ... */ }

  wait.enqueue(send_welcome, Welcome { user_id });
  ```

  - **Transport:** an async invoke (`InvocationType=Event`) with
    `{task, args}`, issued during the tier-1 drain, so no added response
    latency. Lambda's built-in async retries plus an on-failure destination
    provide basic durability. Each task gets a full 15-minute budget.
  - **Deployment:** the same zip deploys as a separate `<app>-tasks` function
    with reserved concurrency, so a task backlog can't starve web traffic. The
    adapter routes events by shape (Function URL event vs task event), so
    there is no public task route.
  - **Other environments:** `tokio::spawn` locally; on Vercel, a POST to an
    authenticated internal `/__nx/task` route (a fresh invocation); a local
    worker on own hardware.
  - **Limits:** async payloads are capped at a few hundred KB, so pass IDs,
    not blobs. Check whether Lambda's recursive-loop detection trips on tasks
    that enqueue tasks.
  - **Path to durable execution:** a task is a workflow step without the
    journal. Durable execution (ROADMAP) adds the S3/R2 journal on top
    without changing `#[nextrs::task]`.

## Layout (proposed)

```text
platform/
├── crates/cli/          # the binary: init, deploy, promote, rollback, logs, stats, env, domains
├── crates/core/         # config, state-bucket protocol, artifact keys (shared, testable)
├── crates/aws/          # AWS target: bootstrap stack + deploy-path SDK calls
├── templates/bootstrap.yaml   # CloudFormation for init
└── docs/
```

Targets are a trait (`bootstrap`, `deploy`, `promote`, `logs`), so the
own-hardware target is a second implementation, not a fork.

## Relationship to nextrs

It's a separate project, with nextrs as its first consumer. `nextrs deploy
--target aws` builds (or pulls from cache) and hands off to the platform. The
`[build] command` verification and the artifact cache already in nextrs carry
over unchanged.

## Phases

1. **v0: single app, prod only, AWS.** The table above. Dogfood on hhh,
   which already has cold-start telemetry to compare against Vercel.
2. **v1: previews + domains.** One deployment per branch via a wildcard
   domain. A CloudFront Function picks the origin by hostname using a
   CloudFront KeyValueStore. Also `env` per environment.
3. **v2: dashboard.** A static page (or a Lambda in your account) that reads
   the state bucket and CloudWatch, giving you the Vercel UI without a hosted
   service.
4. **v3: own-hardware target.** The Coolify part: SSH to your servers, run
   the same artifact under systemd (or Docker for apps that need system
   libraries), put Caddy in front for TLS, and ship logs to a local store.
   Same CLI, same state protocol.
5. **Later:** durable execution, a Cloudflare Workers target (WASM), and
   multiple regions.

## Non-goals

- Multi-tenant hosting. It's never a service other people's apps run on.
- Kubernetes.
- Building code inside the platform. Builds stay local, in CI, or in the
  cache.
- Next.js / Node SSR parity in v0. Any HTTP server works through the Web
  Adapter, but framework-specific magic (ISR, image optimization) is out of
  scope.

## Open questions

- **Name**, and whether it lives in the nextrs repo as `crates/` or in its
  own repo (leaning toward its own repo, since it's runtime-agnostic).
- **Bootstrap in CloudFormation vs Terraform vs pure SDK.** CloudFormation
  gives one-command teardown and no extra tool to install. Pure SDK is faster
  but needs our own reconcile and cleanup logic.
- **Bootstrap for the account itself:** create the platform member account via Organizations from the CLI, or document it as a manual one-time step?
- **One function per app vs per-route functions.** Start with one, as on
  Vercel today.
- **Cost guardrails.** Reserved-concurrency caps and a budget alarm by
  default, so a traffic spike or loop can't produce a surprise bill.
