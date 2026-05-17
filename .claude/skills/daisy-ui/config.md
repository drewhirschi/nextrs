# daisyUI config

All daisyUI configuration goes inside the `@plugin "daisyui"` block in your CSS file (Tailwind 4 has no JS config).

## Minimal

```css
@import "tailwindcss";
@plugin "daisyui";
```

Defaults: `light` (default), `dark` (when `prefers-color-scheme: dark`), no prefix, logging on.

## Common configurations

**Light theme only:**
```css
@plugin "daisyui" {
  themes: light --default;
}
```

**Pick which built-in themes to ship; choose defaults:**
```css
@plugin "daisyui" {
  themes: light, dark, cupcake, bumblebee --default, synthwave --prefersdark, retro;
}
```
- A theme marked `--default` is applied when no `data-theme` attribute is set.
- A theme marked `--prefersdark` is applied when the OS reports dark mode.
- Other listed themes can be activated by setting `<html data-theme="cupcake">`.

**All daisyUI built-in themes** (34 total):
```
light, dark, cupcake, bumblebee, emerald, corporate, synthwave, retro, cyberpunk,
valentine, halloween, garden, forest, aqua, lofi, pastel, fantasy, wireframe,
black, luxury, dracula, cmyk, autumn, business, acid, lemonade, night, coffee,
winter, dim, nord, sunset, caramellatte, abyss, silk
```

## Full default config

```css
@plugin "daisyui" {
  themes: light --default, dark --prefersdark;
  root: ":root";
  include: ;
  exclude: ;
  prefix: ;
  logs: true;
}
```

| option | meaning |
|---|---|
| `themes` | comma-separated list of theme names; flag with `--default` and `--prefersdark` |
| `root` | which CSS selector themes attach to (default `:root`); useful to scope daisyUI to a subtree |
| `include` | comma-separated list — only generate these component class groups |
| `exclude` | comma-separated list — skip these (e.g. `exclude: rootscrollgutter, checkbox;`) |
| `prefix` | add a prefix to every daisyUI class (e.g. `prefix: daisy-;` → `daisy-btn`, `daisy-card`) |
| `logs` | print daisyUI build info; set `false` for cleaner CI logs |

## Custom themes

Define a custom theme alongside (or instead of) the built-ins:

```css
@import "tailwindcss";
@plugin "daisyui";
@plugin "daisyui/theme" {
  name: "mytheme";
  default: true;
  prefersdark: false;
  color-scheme: light;

  --color-base-100: oklch(98% 0.02 240);
  --color-base-200: oklch(95% 0.03 240);
  --color-base-300: oklch(92% 0.04 240);
  --color-base-content: oklch(20% 0.05 240);
  --color-primary: oklch(55% 0.3 240);
  --color-primary-content: oklch(98% 0.01 240);
  --color-secondary: oklch(70% 0.25 200);
  --color-secondary-content: oklch(98% 0.01 200);
  --color-accent: oklch(65% 0.25 160);
  --color-accent-content: oklch(98% 0.01 160);
  --color-neutral: oklch(50% 0.05 240);
  --color-neutral-content: oklch(98% 0.01 240);
  --color-info: oklch(70% 0.2 220);
  --color-info-content: oklch(98% 0.01 220);
  --color-success: oklch(65% 0.25 140);
  --color-success-content: oklch(98% 0.01 140);
  --color-warning: oklch(80% 0.25 80);
  --color-warning-content: oklch(20% 0.05 80);
  --color-error: oklch(65% 0.3 30);
  --color-error-content: oklch(98% 0.01 30);

  --radius-selector: 1rem;
  --radius-field: 0.25rem;
  --radius-box: 0.5rem;

  --size-selector: 0.25rem;
  --size-field: 0.25rem;

  --border: 1px;
  --depth: 1;
  --noise: 0;
}
```

**Rules:**
- All `--color-*` variables above are required.
- Colors can be `oklch()`, hex, or any CSS color format. `oklch` is preferred — preserves perceptual lightness across hues.
- `--radius-*` preferred values: `0rem`, `0.25rem`, `0.5rem`, `1rem`, `2rem`.
- `--depth` is `0` or `1` only; `1` adds a subtle 3D shadow effect.
- `--noise` is `0` or `1`; `1` adds a subtle grain texture.
- Visual generator: <https://daisyui.com/theme-generator/>

## Switching themes at runtime

Set `data-theme` on `<html>`:
```html
<html data-theme="dark">
```

Or use a `theme-controller` input (see `components/actions.md`):
```html
<input type="checkbox" value="synthwave" class="theme-controller" />
```
