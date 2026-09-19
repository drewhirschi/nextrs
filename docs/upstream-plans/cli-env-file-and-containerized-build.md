# The CLI reads process env only, and `deploy` always builds on the host

- **Reported-in:** daily_mirror (a private repo; a NextRS server deployed to
  Vercel with three server bundles and a Cloudflare-provider cron)
- **Date:** 2026-09-19
- **Status:** open
- **Pins in the reporting app:** `nextrs` 0.6.1 at rev `9fa4141d`;
  `cargo-nextrs` 0.3.0 (same rev — the CLI and library version numbers differ,
  which is itself confusing when reporting a bug).

## Context for readers without the app

daily_mirror's `server/` is a nextrs app linked to a Vercel project. Its
`nextrs.toml` declares three bundles:

```toml
[app]
name = "daily-mirror"
url  = "https://daily-mirror-pearl.vercel.app"

[vercel]
git_deploys = false
regions = ["pdx1"]

[bundles.images]
features = ["image-processing"]

[bundles.vision]
features = ["face-inference"]
assets   = ["resources/vision"]
```

The `vision` bundle links OpenCV and a private MediaPipe `.so`. Because the
workstation's glibc (2.44) is far newer than Vercel's runtime, that bundle
**must** be compiled inside a Debian 11 image; a host build produces binaries
Vercel cannot exec. The app therefore does not run `nextrs deploy`. It runs a
wrapper, `server/scripts/deploy-prebuilt.sh`, driven by a `just deploy` recipe.

The owner's goal, verbatim: *he doesn't want `deploy-prebuilt.sh` at all — he
wants to run one `nextrs deploy` that is configured via `nextrs.toml` and "just
works", and the tool should know to read the env file.*

## 1. Summary and the exact failure

On 2026-09-19 `just deploy` built the bundles in the image and succeeded
through `vercel deploy --prebuilt --target=production`. The last step failed:

```
$ nextrs cron deploy --root .
nextrs: Cloudflare cron routes are configured, but CRON_SECRET is missing;
        set it to the same value configured on the Vercel app
```

`CRON_SECRET` **is** configured on the Vercel app, and it is present locally in
`server/.vercel/.env.production.local`, which `vercel pull` / `vercel env pull`
wrote. The CLI never looks at that file.

Minimal repro, no container needed: in any nextrs app with a
`provider = "cloudflare"` cron, run `vercel pull --yes
--environment=production` so `.vercel/.env.production.local` contains
`CRON_SECRET`, then run `nextrs cron deploy --root .` from a shell where
`CRON_SECRET` is not exported. It fails. `env $(...) nextrs cron deploy` works.
`nextrs deploy` fails the same way, just earlier — it runs the same preflight
before building (`crates/cargo-nextrs/src/deploy.rs:35-43`).

## 2. Issue A — the CLI never loads an env file

### Current behaviour

- `crates/cargo-nextrs/src/cron.rs:315-330`
  (`preflight_cloudflare_credentials`) reads `CRON_SECRET`,
  `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` through
  `std::env::var` only.
- `crates/cargo-nextrs/src/cron.rs:441-449` (`deploy`) reads the same two
  Cloudflare variables again to pick the API-vs-wrangler transport.
- `crates/cargo-nextrs/src/deploy.rs:35-43` calls the same preflight, and
  `deploy.rs:307-313` reads `VERCEL_TOKEN` for `--token`.
- `crates/cargo-nextrs/src/cron.rs:656` reads `NEXTRS_CRON_SKIP_PREFLIGHT`.
- There is **no dotenv loading anywhere in `cargo-nextrs`**. Repo-wide,
  `dotenvy` appears only in `site/src/main.rs:11-13` and in the strings
  `create-nextrs-app` emits (`crates/create-nextrs-app/src/lib.rs:404, 981,
  1049`) — i.e. only in generated *applications*, never in the CLI.
- There is no env-file key in `nextrs.toml`: `NextrsConfig`
  (`crates/cargo-nextrs/src/cron.rs:34-43`) has `app`, `bundles`, `deployment`
  and `vercel`, and `VercelConfig` (`:51-66`) has `regions`, `runtime`,
  `install_command`, `build_command`, `git_deploys` and an `extra` table.
  Nothing names a file.

The sharp edge: `nextrs deploy` itself runs `vercel pull
--environment=production` (`deploy.rs:97-104`), which *writes*
`.vercel/.env.<target>.local`. The CLI creates the file holding the answer and
then refuses to read it.

This is the same root cause already recorded in
[PORT and .env consistency](port-and-dotenv-consistency.md), one level up: that
note is about generated apps not loading `.env`; this one is about the CLI.
Worth fixing under one stated rule.

### Proposed behaviour

1. Before any command that reads credentials, load env files into the process
   env **without overriding what is already set** — `dotenvy::from_path_iter`
   plus an explicit `if env::var_os(k).is_none() { set_var(k, v) }`, *not*
   `from_path` semantics you have to think about. Process env must win so CI
   secret stores and `CRON_SECRET=… nextrs …` keep working.
2. Default search, in order, first hit wins per key — target-specific files
   first, generic `.env` as the fallback (Drew's adjustment, 2026-09-19), where
   target is `production` for a normal deploy / `cron deploy` and `preview` for
   `--preview`:
   - `<root>/.vercel/.env.<target>.local` (plus the same path in the Vercel
     project dir when a Root Directory is set — that is where `vercel pull`
     writes);
   - `<root>/.env.<target>.local`, then `<root>/.env.<target>`;
   - `<root>/.env.local`, then `<root>/.env`.
3. Add an optional override in `nextrs.toml` so a project can be explicit:

   ```toml
   [deploy]
   env_file = ".vercel/.env.production.local"   # or a list, first wins
   ```

   When set, it replaces the default search rather than adding to it, and a
   missing explicit file is a hard error (a typo should not silently fall back).
4. Never echo values. Log at most `nextrs: loaded 3 variables from
   server/.vercel/.env.production.local`. Values must not reach stderr, and the
   existing "never print `VERCEL_TOKEN`" care in `deploy.rs:307-313` should be
   stated as a rule covering this too.
5. Make the failure message name what was searched:

   ```
   nextrs: Cloudflare cron routes are configured, but CRON_SECRET is not set.
     searched: .vercel/.env.production.local (not found)
               .env.local (not found)
               .env (no CRON_SECRET)
     It must match the value on the Vercel app. Pull it with:
       vercel env pull .vercel/.env.production.local --environment=production
     or set [deploy] env_file in nextrs.toml.
   ```

### Implementation sketch

- Add `dotenvy = "0.15"` to `crates/cargo-nextrs/Cargo.toml` (already a
  transitive-friendly choice — the site uses it).
- New `crates/cargo-nextrs/src/env_file.rs`:
  `pub fn load(root: &Path, target: Target, config: &NextrsConfig) ->
  Vec<LoadedFile>` returning what was searched and how many keys each file
  supplied (names optional, **values never**), for the error message.
- Call it in exactly one place per entry point, in `lib.rs` where
  `CommandLine::Deploy` and `CommandLine::CronDeploy` are dispatched
  (`crates/cargo-nextrs/src/lib.rs:61-72`). Calling it there means `deploy`,
  `cron deploy` and the shared `preflight_cloudflare_credentials` all benefit
  without touching `cron.rs`'s reads.
- **As implemented:** `env_file::load` is called inside `cron::deploy` and in
  `deploy::deploy` right after `vercel pull`; the Cloudflare credential
  preflight moved to just after that load (still ahead of the build), which
  removes the double-load question below.
- Subtlety worth deciding: for `nextrs deploy`, `vercel pull` runs *after*
  dispatch and may create/refresh the file. Either load once before and once
  again right after the `vercel pull` step (idempotent, since process env wins),
  or document that the first deploy in a fresh clone needs a second run. The
  double load is cheap and removes a genuinely confusing first-run failure.
- `unsafe { std::env::set_var }` on recent Rust: keep it to the single-threaded
  startup path, as `crates/nextrs/src/cron.rs:168` already does in tests.

### Tests to add

- `env_file::load` prefers an already-set process variable over the file value.
- `.vercel/.env.production.local` written with Vercel's actual quoting —
  `CRON_SECRET="abc#def"`, a value containing `=`, an escaped `\n`, a blank
  line, a `#` comment — parses to the exact expected values.
- `--preview` selects `.env.preview.local` and never reads the production file.
- An explicit `[deploy] env_file` that does not exist is an error naming the
  path; an explicit one that exists suppresses the default search.
- The error string for a missing `CRON_SECRET` lists every path searched.
- A test asserting no loaded *value* appears in captured stderr (guards against
  a future debug print).

## 3. Issue B — what blocks deleting `deploy-prebuilt.sh`

The claim that "everything else is already implemented" is **wrong**, but
narrower than it first looked. `nextrs deploy` covers more of the script than
expected; one thing it structurally cannot do is the container build.

`deploy.rs:26-170` does: `bundles::plan` → `cron::generate` → Cloudflare
preflight → read `.vercel/project.json` and resolve Root Directory → `vercel
pull --environment=<target>` → (bundles path) `npm ci`, `npm run
client:prepare`, `bundles::build(--vercel)`, `npm run client:build` → `vercel
deploy --prebuilt [--prod]` → `cron::deploy` for production.

| Responsibility (script / justfile) | Covered by `nextrs deploy` today? | Proposal |
| --- | --- | --- |
| Generate frontend + typed client (`npm run client:prepare`) | **Yes** — `deploy.rs:116-117` (and it additionally runs `npm ci` and `client:build`, which the script does not) | none |
| **Build the bundles in a glibc-pinned Docker image** (`scripts/native/build.sh`) | **No.** `bundles::build` shells `cargo` on the host (`bundles.rs:150-300`). This is the blocker. | New `[build]` section in `nextrs.toml` — see below |
| `NEXTRS_SKIP_BUNDLE=1` inside the container, reusing host-built `public/dist` | **No** — the CLI never sets it; only the app's `build.rs` reads it (`crates/nextrs/src/bundle.rs:116-125`) | Falls out of the `[build]` hook: nextrs sets it for the containerized step, since the frontend was already built on the host |
| Assert every planned bundle produced an executable | **Effectively yes** — `bundles.rs:288-291` errors with "cargo returned no executable for `<name>`", and `:279-286` refuses a bundle whose codegen receipt disagrees with the plan. The script's `for name in $(nextrs bundles plan …)` loop is belt-and-braces. | none; maybe promote the receipt check into the docs so wrappers stop re-implementing it |
| Refuse to run if a hand-written `server/vercel.json` shadows the managed `.nextrs/vercel.json` | **Partly** — `legacy_vercel_warning` (called from `cron::generate`) warns; the script and `just deploy-check` make it fatal | Make it an error in `deploy` (not just `generate`), since silently rebuilding the default-features monolith is exactly the bug commit `7860cde` fixed |
| Require `.vercel/project.json` (linked project) | **Yes** — `deploy.rs:50-59`, with a good message | none |
| `--preview` / `--skip-cron`, and skipping cron on preview | **Yes** — `DeployOptions`, `deploy.rs:157-165` | none |
| Cron failure leaves the app deployed and prints a retry-only-that-phase command | **Yes** — `cron_recovery_error`, `deploy.rs:167-176` | none |
| Precondition: private vision assets present (`resources/vision/lib/libmediapipe.so`) | **No**, and it should stay out — the file is gitignored and unregenerable | Project-specific. Keep in this repo, or express generically as a preflight on declared `assets` paths (`bundles.assets` already lists `resources/vision`) — a "declared asset missing" check would cover it |
| Toolchain doctor (`docker`, `node`, `npm`, `vercel` on PATH) | **Partly** — `run()` has a nice "is it installed?" message (`deploy.rs:315-317`) | Project-specific; `just doctor` should keep it |
| Full test suite before deploying (`just deploy-check` → `just check`) | **No**, and should not be | Project-specific. Stays in the justfile |
| **Env file loading for the cron step** | **No** | Issue A |

So: **one upstream feature blocks deleting the script** — a way to run the
bundle compile somewhere other than the host — plus Issue A. Everything else is
either already covered or genuinely belongs to the project.

### Proposed `[build]` hook

```toml
[build]
# Either a prebuilt/Dockerfile-hashed image nextrs runs the compile in …
image = { dockerfile = "scripts/native/Dockerfile", tag_from_hash = true }
volumes = ["daily-mirror-build:/build"]
env = { CARGO_TARGET_DIR = "/build/target", RUSTFLAGS = "…" }

# … or, the low-ambition version: an escape-hatch command nextrs invokes
# instead of `cargo build`, with the bundle name/features/output passed in.
# command = "scripts/native/build.sh"
```

The command hook is the cheaper, more honest first step: nextrs keeps owning
plan, receipt verification, `.func` staging and deploy, and only the *compile*
is delegated. It would let daily_mirror delete `deploy-prebuilt.sh` entirely
while `scripts/native/` shrinks to a Dockerfile plus a thin compile script —
which is a project-specific glibc problem and should stay project-specific.

Worth noting for the roadmap: "Vercel's runtime glibc is older than a modern
Linux workstation's" is not a daily_mirror problem. Any nextrs app linking a
system C/C++ library will hit it, and today the only answer is to stop using
`nextrs deploy`.

## 4. Proposed end state

`server/nextrs.toml`:

```toml
[app]
name = "daily-mirror"
url  = "https://daily-mirror-pearl.vercel.app"

[vercel]
git_deploys = false
regions = ["pdx1"]

[deploy]
env_file = ".vercel/.env.production.local"

[build]
command = "scripts/native/build.sh"   # glibc-pinned container compile

[bundles.images]
features = ["image-processing"]

[bundles.vision]
features = ["face-inference"]
assets   = ["resources/vision"]
```

The single command: `nextrs deploy --root server` (`--preview` for a preview).

The justfile recipe shrinks from a script invocation plus a nine-line
`deploy-check` to:

```just
deploy: check
    cd server && PATH="node_modules/.bin:$PATH" nextrs deploy --root .
```

and `server/scripts/deploy-prebuilt.sh` is deleted.

## 5. Interim workaround in the reporting app

Already applied there (2026-09-19): `server/scripts/deploy-prebuilt.sh` now
reads `server/.vercel/.env.production.local` itself for production deploys and
exports only `CRON_SECRET`, `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID`
— the three variables `cron.rs:315-330` and `cron.rs:441-449` consume — before
`nextrs cron deploy`, with already-set process variables winning, values never
printed, and the file parsed rather than sourced. **Delete that block, and the
note beside it, as soon as the CLI loads env files itself.** Anyone hitting
this before the fix lands can do the same in one line by exporting those three
variables from the pulled file without echoing them.
