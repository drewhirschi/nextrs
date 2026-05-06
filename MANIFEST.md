# Manifest: nextrs

## Purpose

This repository explores a **Next.js-like application routing model implemented in Rust**: file- or folder-based routes, **`page` vs `loading` semantics**, and layouts/templates for HTML—**not** React or the Next.js bundler—but with **Rust serverless handlers on Vercel** (`vercel_runtime`) as the execution environment.

The goal is a small, repeatable pattern for ergonomic server-rendered navigation and staged UI (immediate shell, deferred body) analogous to Next’s `loading.tsx` / `page.tsx`, without implying feature parity with Next.js itself.

## Vision

Establish conventions and reference implementations—where routes live, how URLs map to binaries, how “loading shells” compose with “real pages”—that could grow into a clearer framework boundary (generators, shared middleware, conventions over one-off `rewrite` plumbing) while staying honest about Rust + Vercel constraints.

## Non-goals

- **Not** a fork or reimplementation of Next.js/React/SSR streaming as shipped by Vercel for Node.
- **Not** prescribing a stable public API yet; pieces may move as conventions harden.

## Patterns in this tree

| Area | Where to look |
|------|----------------|
| **Route handlers as Cargo bins** | `Cargo.toml` `[[bin]]` entries; handler sources under `api/` (e.g. `api/page.rs`, `api/landing.rs`, `api/readyz.rs`). |
| **Clean URLs → functions** | `vercel.json` `rewrites` (e.g. `/` → `/api/page`, `/landing` → `/api/landing`). |
| **Loading shell + delayed body (htmx)** | `api/landing.rs` (non-htmx → loading template; `HX-Request` → page fragment), with templates in `templates/landing/` and `htmx` loaded from `templates/layout.html`. |
| **Documented folder convention** | `README.md` describes the intended per-route layout (`page` / `loading` modules, rewrites, htmx swap targets). The current `landing` route condenses that idea into one binary plus Askama templates—use it as the working reference alongside the README. |

For operational detail (local `vc dev`, adding routes), continue to rely on **`README.md`**.
