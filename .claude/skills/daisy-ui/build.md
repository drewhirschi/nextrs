# Building and running daisyUI

Tailwind CSS 4 scans your template files for class names and emits a CSS file containing only the classes you actually used. daisyUI's classes are picked up the same way. The shape of "build" depends on your project.

## Tailwind CLI: the universal path

If you don't have a JS bundler, use the Tailwind standalone CLI. It's the simplest setup and works in any project (Rust, Go, plain HTML, …).

**One-shot build** (for production):
```bash
npx @tailwindcss/cli -i ./src/styles/app.css -o ./static/app.css --minify
```

**Watch mode** (for development):
```bash
npx @tailwindcss/cli -i ./src/styles/app.css -o ./static/app.css --watch
```

Flags:
- `-i` input — your CSS file with `@import "tailwindcss";` and `@plugin "daisyui";`.
- `-o` output — what your server serves.
- `--watch` — re-build on change.
- `--minify` — for production.

### Telling Tailwind which files to scan

Tailwind 4 auto-detects sources via the `@source` directive in your CSS. By default it scans the directory containing the input CSS, but for templates living elsewhere (e.g. an `app/` route directory of Askama `.html` files), add explicit sources:

```css
@import "tailwindcss";
@source "../app";              /* scan ../app for class names */
@source "../templates";        /* and ../templates */
@plugin "daisyui";
```

Without this, Tailwind won't see your template classes and they'll be purged out of the output.

## Framework-specific

| Stack | How |
|---|---|
| **Next.js / Vite / Astro** | Install `@tailwindcss/postcss` or `@tailwindcss/vite`, add to the bundler config. daisyUI is just `@plugin "daisyui";` in your global CSS. |
| **Rust + Askama / Tera / Maud** | Use the standalone CLI (above). Point `@source` at your templates dir. Serve the built CSS as a static asset (e.g. `ServeDir::new("static")` in axum). Run the CLI's `--watch` alongside `cargo run` in a separate terminal, or wrap both in a `Makefile` / `justfile`. |
| **Go html/template, plain HTML** | Same as Rust — standalone CLI + static file serving. |
| **CDN path** | No build. Tailwind scans the live DOM at runtime. |

## Dev loop (manual two-terminal)

```bash
# terminal 1
npx @tailwindcss/cli -i ./src/styles/app.css -o ./static/app.css --watch

# terminal 2
<your server command>     # e.g. cargo run, npm run dev, hugo server
```

## Dev loop (one command, justfile / Makefile)

```make
dev:
	npx @tailwindcss/cli -i ./src/styles/app.css -o ./static/app.css --watch & \
	cargo run -p site; \
	kill %1
```

Or use a process manager like `overmind` / `foreman` with a `Procfile`:

```
css:    npx @tailwindcss/cli -i ./src/styles/app.css -o ./static/app.css --watch
server: cargo run -p site
```

## Production build

Run the one-shot build as part of your release pipeline, before shipping the binary or container:

```bash
npx @tailwindcss/cli -i ./src/styles/app.css -o ./static/app.css --minify
```

Then bake `static/app.css` into the artifact (or upload to your CDN). The Rust binary doesn't need Node at runtime — only at build time.

## Common pitfalls

- **Class names appearing as raw strings in output but no styles applied** → Tailwind isn't scanning that file. Add an `@source` directive.
- **Dynamic class names (`btn-${color}`) don't work** → Tailwind only finds full string matches. Either list possible values somewhere it can scan, or use a `safelist`-style comment, or build the full class name as a literal in your template.
- **Updated daisyUI but classes still look old** → kill the watcher, delete the output CSS, restart.
- **CDN version doesn't pick up custom themes** → you can't customize via `@plugin` directives over CDN. Switch to the npm path.
