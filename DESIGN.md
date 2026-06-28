# Design System — nextrs

> Source of truth for the nextrs docs site (rebuilt in React, dogfooding nextrs).
> Read this before any visual/UI decision. Don't deviate without explicit approval.

## Product Context
- **What this is:** a Rust web framework for building React apps — Next.js DX, engineered for the agent era.
- **Who it's for:** React/JS developers building app-style products who want Rust performance without Rust pain.
- **Space:** web frameworks / developer tools. Peers: Next.js, TanStack Start, Vercel, Astro, Bun.
- **Project type:** developer marketing + documentation site.

## Aesthetic Direction
- **Direction:** technical-precise, "engineered for agents." **Dark-first.**
- **Decoration:** intentional — sharp surfaces, hairline borders, subtle grain. No ornament, no gradients-as-decoration.
- **Mood:** fast, confident, systems-grade — a foundation that keeps up with agent-speed development. Rust substance + modern React polish.
- **Positioning between peers:** warmer/sharper than Vercel's austere black-and-white, more serious than TanStack's candy color.

## Typography
- **Display/Hero:** Cabinet Grotesk (700/800) — geometric, confident, characterful.
- **Body:** Geist (400/500) — clean, technical, built for dev UIs.
- **UI/Labels:** Geist (500).
- **Data/Tables:** Geist with `font-variant-numeric: tabular-nums`.
- **Code & accents:** Berkeley Mono (licensed, aspirational); **Geist Mono / JetBrains Mono** as the free fallback. Mono is a brand surface — code blocks, the wordmark, and headline eyebrows.
- **Loading:** Cabinet Grotesk via Fontshare; Geist + Geist Mono via Google Fonts.
- **Scale (rem):** 0.75 / 0.875 / 1 / 1.125 / 1.375 / 1.75 / 2.5 / 3.5 / 4.75.

## Color
- **Approach:** restrained — one accent + warm neutrals; accent is rare and meaningful.
- **Canvas:** `#0C0B0E` · **Surface:** `#15141A` · **Raised:** `#1E1C24`
- **Text:** `#ECEAE4` (primary) · `#9A968C` (muted) · `#6E6A60` (faint)
- **Accent (ember):** `#FF6A2C` · hover `#FF7E48` — Rust heritage + heat/speed, deliberately not the framework-default blue/purple.
- **Hairlines/borders:** `#2A2832`
- **Semantic (sparing):** success `#46C57E` · warning `#E8B84B` · error `#F4685B` · info `#5AA9F0`
- **Light mode (offered, dark is hero):** paper `#F7F5F0`, ink `#1A1820`, accent deepened to `#E2551B` for contrast.

## Spacing
- **Base unit:** 8px (4px half-step).
- **Density:** comfortable (marketing) / compact (docs body).
- **Scale:** 2xs(2) xs(4) sm(8) md(16) lg(24) xl(32) 2xl(48) 3xl(64) 4xl(96).

## Layout
- **Approach:** hybrid — poster/creative marketing, grid-disciplined 3-pane docs.
- **Grid:** 12-col marketing; docs = `[sidebar 260px] [content ≤760px] [TOC 220px]`.
- **Max width:** 1200px marketing shell; 760px docs prose.
- **Border radius:** sm 6px / md 10px / lg 14px / pill 9999px — tight and engineered, never bubbly.

## Motion
- **Approach:** minimal-functional + *genuine* speed.
- The site dogfoods nextrs soft-nav + hover-prefetch — real instant navigation is the motion story. No gratuitous scroll choreography.
- **Easing:** enter ease-out / exit ease-in / move ease-in-out.
- **Duration:** micro 80ms / short 160ms / medium 240ms / long 400ms.

## Signature Risks (where nextrs gets a face)
1. **Mono headlines / wordmark** — terminal/systems identity, breaks the sans-everything framework norm.
2. **Ember accent** over default framework blue.
3. **Ship the site's own speed as the hero** — a live instant-nav demo + a real cold-start number. Proof over claims.

## Anti-patterns (never)
Purple/violet gradients, 3-column icon-in-circle feature grids, centered-everything, uniform bubbly radii, gradient CTA buttons, stock-photo heroes.

## Decisions Log
| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-06-27 | Initial system | Created by /design-consultation from locked positioning (Rust+React, TanStack client stack, app-style, speed-first). Dark-first, ember accent, mono-forward. |
