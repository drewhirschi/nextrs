# Setting up daisyUI in a project

daisyUI 5 requires **Tailwind CSS 4**. Tailwind 4 dropped `tailwind.config.js` — everything is configured in CSS now via `@import` and `@plugin` directives.

## Decision: npm dependency vs. CDN

| | npm dependency (recommended) | CDN |
|---|---|---|
| Production-ready | yes | for prototypes only |
| Needs Node toolchain | yes (Tailwind CLI or Vite/Next/etc.) | no |
| Output size | small (purged) | full unpurged stylesheet |
| Use when… | you have any build step | static page, no Node, want it working in 30 seconds |

## Path A — npm dependency (recommended)

1. Install:
   ```bash
   npm i -D tailwindcss @tailwindcss/cli daisyui@latest
   ```
   (Use the `@tailwindcss/cli` package for the standalone CLI. If you're inside a framework like Next/Vite/Astro, install the framework's Tailwind plugin instead — `@tailwindcss/vite`, `@tailwindcss/postcss`, etc.)

2. Create a CSS entry file (e.g. `src/styles/app.css` or `assets/app.css`):
   ```css
   @import "tailwindcss";
   @plugin "daisyui";
   ```
   That's the minimum. For themes/prefix/scope options, see [config.md](config.md).

3. Build the CSS — see [build.md](build.md) for the exact commands and how to wire it into different kinds of projects (Rust/Askama, plain HTML, etc.).

4. Link the **built** CSS from your HTML/template `<head>`:
   ```html
   <link rel="stylesheet" href="/static/app.css">
   ```

## Path B — CDN (prototypes only)

Drop these into the `<head>`. No build step needed:

```html
<link href="https://cdn.jsdelivr.net/npm/daisyui@5" rel="stylesheet" type="text/css" />
<script src="https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4"></script>
```

Caveats:
- Ships the entire Tailwind + daisyUI stylesheet uncompressed — heavy.
- No way to customize themes via `@plugin "daisyui" { ... }` (no CSS file in your control).
- Fine for a one-off page or a `mockup-*` demo; don't ship it to users.

## Refresh this skill's vendored snapshot

```bash
curl -fsSL https://daisyui.com/llms.txt -o .claude/skills/daisy-ui/reference/llms.txt
```

Run this when daisyUI bumps a minor version (5.6, 5.7, …) and you want the category files to reflect new components or class names. After refreshing, diff `reference/llms.txt` against the category files in `components/` and update any stale entries.
