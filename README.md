---
name: Rust Hello World
slug: rust-hello-world
description: Next.js-style routing in Rust serverless functions, with htmx-driven loading states.
framework:
  - Other
type:
  - Starter
css:
  - None
githubUrl: https://github.com/vercel/examples/tree/main/rust/hello-world
demoUrl: https://rust-hello-world.vercel.dev
deployUrl: https://vercel.com/new/clone?repository-url=https://github.com/vercel/examples/tree/main/rust/hello-world&project-name=rust-hello-world&repository-name=rust-hello-world
publisher: Vercel
relatedTemplates:
  - rust-axum
---

# Rust Hello World

Vercel Rust serverless functions arranged in a Next.js-like route folder convention, with htmx providing the `loading.tsx` + `page.tsx` experience.

## The pattern

Each route is a folder under `api/` containing:

- `loading.rs` — returned immediately, renders a shell with htmx attributes that fetch the real content.
- `page.rs` — returned by the htmx swap, contains the actual page body.

Hitting `/landing` returns the loading shell. The shell contains:

```html
<main hx-get="/api/landing/page" hx-trigger="load" hx-swap="innerHTML">
  ...spinner...
</main>
```

On load, htmx fetches `/api/landing/page` and swaps the response into the main element. Same idea as Next.js streaming with `loading.tsx` — instant first paint, content streams in.

## Project structure

```
api/
├── page.rs              # /     (homepage)
└── landing/
    ├── loading.rs       # /landing  (loading shell, served first)
    └── page.rs          # /api/landing/page (htmx swap target)
vercel.json              # rewrites: /landing → /api/landing/loading
Cargo.toml               # one [[bin]] per .rs file
```

## Why `api/` and not `app/`?

Vercel's Rust runtime hard-codes `api/` for function discovery — `vercel.json` `builds` doesn't redirect it. So sources live in `api/`, but the *folder convention inside it* is borrowed from Next.js: each route is a folder with `page.rs` and (optionally) `loading.rs`. `rewrites` give clean URLs (`/landing` instead of `/api/landing/loading`).

## Adding a route

1. Create `api/<route>/page.rs` and (optionally) `api/<route>/loading.rs`.
2. Add `[[bin]]` entries to `Cargo.toml` for each file.
3. Add a rewrite to `vercel.json`: `{ "source": "/<route>", "destination": "/api/<route>/loading" }`.
4. In `loading.rs`, point htmx at `/api/<route>/page`.

## Develop locally

```bash
# Install Rust if you haven't:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Vercel CLI:
npm i -g vercel

# From this directory:
vc dev
```

Open http://localhost:3000 and click through to `/landing`.
