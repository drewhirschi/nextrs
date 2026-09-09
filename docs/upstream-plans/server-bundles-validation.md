# Server bundles validation — 2026-09-07

Application source: `fe74d96e3e23402da5a4ff926943a64cd699fd0a`.
Follow-up changes add validation scripts and this report; they do not change the deployed application.

## What the sample actually demonstrates

The Rust `csv` crate writes a fixed sample CSV **export**, not an import and not
an export of the application's in-memory Todo store. Two bundles are built:

- `default`: `/`, `/about`, `/api/cron/heartbeat`, `/api/todos`,
  `/api/todos/{id}`, `/todos/{id}`, `/todos/{id}/test`. No optional application
  features or private assets.
- `exports`: `/api/exports`, feature `exports` enabling `csv`, and private
  directory `resources/exports`.

## Size measurements

Executable file bytes (not compressed transfer size or process memory):

| Build | default | exports | monolith |
| --- | ---: | ---: | ---: |
| Existing native debug artifacts | 15,249,712 | 13,835,216 | not measured |
| Fresh optimized Linux/Vercel build | 4,272,584 | 3,768,936 | 4,292,088 |

The release baseline used the same source/toolchain and
`cargo zigbuild --locked -p react-todos --bin index --release --target x86_64-unknown-linux-gnu.2.26`
with ordinary default features and no bundle selection. CSV is small: the default
function saves 19,504 bytes (about 0.45%). The two executables total 8,041,520 bytes;
shared runtime code is duplicated. This is an isolation example, not evidence of
large size savings or reduced cold-start latency.

SHA-256 of freshly deployed executables:

- default: `46f0a24ab15c4c4e615f171b3be122f4193e9ebc09ec80fc4db9ad26aa646275`
- exports: `009bd6cc659ff8418b511b9108879ccc96510bb3ee6b86f3c30a3bc9d2d8809b`
- monolith baseline: `02061946f0ed58b31221b391c333f370da6b6b92047ab2fc897b4a3a33697d90`

## Isolation evidence

Both native and optimized Vercel artifacts passed `scripts/test-server-bundles.py`
with `--skip-build` (plus `--vercel` for optimized artifacts):

- Manifest assigns the CSV endpoint only to exports.
- Normal Cargo dependency graphs contain neither `csv` nor `csv-core` in default;
  both are present with the exports feature.
- The compiled sample-export payload appears in exports and is absent in default.
  This is corroborating evidence, not a general binary dependency auditor.
- Exact function file inventories: default contains only executable and, for
  Vercel, `.vc-config.json`; exports adds only `resources/exports/README.txt`.
  Its bytes match the source file, and no resources directory exists in static output.
- Running each function directly: default returns 404 for `/api/exports`;
  exports returns 404 for `/api/todos` and `/`; owning functions succeed.
- CSV HEAD has no body; unsupported POST returns 405.
- A separate default/heavy fixture exercises inherited authorization, dynamic
  parameters, query, binary request body, cookies, status/headers, and incremental
  streaming through the generated routing table and actual local runtime adapters.

The private README is packaged but not read by the handler. These tests prove
placement and public inaccessibility, not runtime consumption of a template.
Shared initialization/dependencies remain shared unless explicitly feature-gated.
No live streaming/auth fixture deployment or cold-start benchmark is claimed.

## Real Vercel Todo preview

Deployed through the source-built `nextrs deploy --preview --root examples/react-todos`:

https://nextrs-react-todos-osjy4j8qv-ashirsc.vercel.app

Deployment: `dpl_FWsHySwdrZbCTnEaL8PZd9WGvoTY`, status **Ready**, target **preview**.
Vercel inspection lists exactly `__nextrs_functions/default` and
`__nextrs_functions/exports`, both in `pdx1` (CLI displayed sizes 1.7 MB and 1.51 MB;
these are distinct from the executable file measurements above).

Run the repeatable live check with an authenticated Vercel CLI and linked example:

```sh
python3 scripts/test-server-bundles-preview.py https://nextrs-react-todos-osjy4j8qv-ashirsc.vercel.app
```

Live checks verify CSV records/content headers, HEAD, POST 405, Todo query and
path-parameter APIs, four page responses, their referenced JavaScript/CSS assets, and 404s for private resources,
artifact manifests, internal function URLs and unknown paths. The authenticated
CLI handles preview protection without changing project protection settings.

Browser interaction validation is pending Vercel sign-in in the in-app browser.
HTTP page success does not establish React hydration or working browser mutations.

## Separate PR check failure

The automatic **docs** cloud preview `dpl_3qhKjPymFMFqZSt9esG3pJXizEhx`
failed in `site/build.rs:31` during `bundle_pages` with OS error 2 (missing file).
The log does not identify the missing file. This is the stock cloud-build path,
not the successful prebuilt Todo preview above. GitHub CI's workspace tests and
typechecks passed at inspection time; later steps were still running.

## Middleware-order follow-up

The fixture now authenticates in root middleware, inserts user/chain context,
and checks the admin role in nested heavy middleware. The same requests run
against an unsplit native executable, the split function directly, and the
split generated routing table. Both native and optimized local Vercel adapter
runs pass: root-before-admin order, context propagation, missing/invalid token
401, non-admin 403, and a handler counter proving rejected requests do not execute
handler code. Existing streaming and forwarding checks continue to pass. These
new fixture assertions have not been exercised on live Vercel infrastructure.

The frontend missing-input diagnostic is fixed in b072f01; all 37 bundler tests
pass and both apps rebuilt with bundling enabled. See
[the root-cause report](docs-missing-generated-client.md). The cloud deployment
configuration is still open; a diagnostic improvement is not a deployment fix.
