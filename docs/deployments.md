# Deployments

Every live Vercel project in the nextrs ecosystem, what it is, and where its
source lives. All projects are under the **`ashirsc`** Vercel team.

## Projects

| Vercel project | Live URL | What it is | Stack | Source | Git target |
|---|---|---|---|---|---|
| **`nextrs-docs`** | [nextrs-docs.vercel.app](https://nextrs-docs.vercel.app) | The docs site (this repo's `site/`) | nextrs | `drewhirschi/nextrs` · root `.` (`api/index.rs` + `site/`) | `nextrs` @ `main`, root `.` — **connected** ✅ |
| **`nextrs-react-todos`** | [nextrs-react-todos.vercel.app](https://nextrs-react-todos.vercel.app) | Small demo: nextrs todos. The **nextrs side of the small benchmark**. | nextrs | `examples/react-todos/` | `nextrs` @ `main`, root `examples/react-todos` |
| **`bench-nextjs-todos`** | [bench-nextjs-todos.vercel.app](https://bench-nextjs-todos.vercel.app) | Small benchmark: Next.js todos. The **Next.js side of the small benchmark**. | Next.js 15 | `benchmarks/apps/nextjs/` | `nextrs` @ `main`, root `benchmarks/apps/nextjs` |
| **`hhh-nextrs`** | [hhh-nextrs.vercel.app](https://hhh-nextrs.vercel.app) | Real-app demo: a production app converted to nextrs. The **nextrs side of the real-app benchmark**. | nextrs | `drewhirschi/hhh-next`, branch `nextrs`, root `.` | `hhh-next` @ `nextrs`, root `.` |
| **`hhh-next`** | [hhh-next.vercel.app](https://hhh-next.vercel.app) | The original production Next.js app (the real product). Also the **Next.js side of the real-app benchmark**. | Next.js | `drewhirschi/hhh-next`, branch `main`, root `.` | `hhh-next` @ `main`, root `.` |

## The benchmark pairs

The performance story compares the *same app built two ways*, at two sizes:

- **Small app** (todos): `nextrs-react-todos`  vs  `bench-nextjs-todos`
- **Real app** (production): `hhh-nextrs`  vs  `hhh-next`

See [`../benchmarks/README.md`](../benchmarks/README.md) and
[`../benchmarks/methodology.md`](../benchmarks/methodology.md) for what's measured.

## Git connection

Goal: no floating projects — every project auto-deploys from a connected repo.
Because several projects share the `drewhirschi/nextrs` monorepo, each needs its
**Root Directory** set so a push only rebuilds the relevant app.

| Project | Repo | Production branch | Root Directory | Connected? |
|---|---|---|---|---|
| `nextrs-docs` | `drewhirschi/nextrs` | `main` | `.` | ✅ |
| `nextrs-react-todos` | `drewhirschi/nextrs` | `main` | `examples/react-todos` | ⬜ |
| `bench-nextjs-todos` | `drewhirschi/nextrs` | `main` | `benchmarks/apps/nextjs` | ⬜ |
| `hhh-nextrs` | `drewhirschi/hhh-next` | `nextrs` | `.` | ⬜ |
| `hhh-next` | `drewhirschi/hhh-next` | `main` | `.` | (real product — leave as-is) |

## Per-stack deploy notes

- **Next.js projects** (`bench-nextjs-todos`, `hhh-next`) deploy with stock
  Vercel settings — nothing special.
- **nextrs projects** (`nextrs-docs`, `nextrs-react-todos`, `hhh-nextrs`) build
  on Vercel's Rust runtime and have non-obvious requirements (toolchain pin,
  prebuilt page bundle, runtime declaration, region as a project setting). The
  gotchas are documented per app:
  - docs site / framework: [`vercel-deploy.md`](vercel-deploy.md)
  - `nextrs-react-todos`: [`../examples/react-todos/README.md`](../examples/react-todos/README.md#deploy-to-vercel)
  - `hhh-nextrs`: the migration guide [`migrating-nextjs-to-nextrs.md`](migrating-nextjs-to-nextrs.md) §11
