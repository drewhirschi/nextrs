# daisyUI semantic colors

daisyUI adds **semantic** color names on top of Tailwind. The same class name resolves to different actual colors per theme — `bg-primary` is one color in `light`, another in `dark`, another in `synthwave`.

## The palette

Each color has a paired `*-content` color meant for text/icons sitting on top of it.

| color | meaning |
|---|---|
| `primary` / `primary-content` | main brand color |
| `secondary` / `secondary-content` | secondary brand color (optional) |
| `accent` / `accent-content` | accent brand color (optional) |
| `neutral` / `neutral-content` | dark grey, for non-saturated UI parts |
| `base-100` / `base-content` | page background — blank surface |
| `base-200` | one shade darker than `base-100` — for elevation/grouping |
| `base-300` | two shades darker — deeper elevation |
| `info` / `info-content` | informative messages |
| `success` / `success-content` | success / safe |
| `warning` / `warning-content` | warning / caution |
| `error` / `error-content` | error / danger / destructive |

## How to use them

Use them anywhere a Tailwind color works — `bg-`, `text-`, `border-`, `ring-`, `fill-`, `stroke-`, `from-`/`to-`, `divide-`, etc:

```html
<button class="bg-primary text-primary-content">Click</button>
<div class="border border-error/20 bg-error/10 text-error">…</div>
<aside class="bg-base-200 text-base-content">…</aside>
```

Opacity modifiers work too: `bg-primary/10`, `text-error/80`.

## Rules (from upstream — important)

1. **Prefer daisyUI semantic names** over raw Tailwind colors so themes work. `bg-primary` ✅, `bg-blue-500` ⚠️.
2. **Never `dark:` for daisyUI colors.** They already adapt to the theme. `dark:bg-base-200` is wrong — `bg-base-200` already does the right thing in dark mode.
3. **Raw Tailwind colors are fixed across themes.** `text-gray-800` is the same gray on every theme — and unreadable on a dark theme where the background is also dark. Avoid for text.
4. **`*-content` colors are pre-paired for contrast.** `primary-content` is guaranteed to be readable on `primary`. Don't put `primary-content` on `base-100` — pairing is undefined.
5. **Page-level rule of thumb:** use `base-*` for the majority of the page; use `primary` sparingly for important/calls-to-action elements.
6. **Don't put `bg-base-100 text-base-content` on `<body>`** — daisyUI already does this for you. Adding it manually causes specificity headaches when you want to override a section.

## Quick mental model

- `base-100` is the page (lightest non-content surface).
- `base-200`/`base-300` are progressively "elevated" surfaces (cards, footers, sidebars).
- `primary` is your brand — buttons, headers, focus rings.
- `error` / `warning` / `success` / `info` are the four feedback colors. Use the `*-content` variant for any text/icon sitting *on* one of these colors.

## When you do need a fixed color

Sometimes you genuinely want "blue, the same blue, forever" — illustrations, code highlights, brand logos. Use raw Tailwind colors there, knowingly:

```html
<span class="bg-sky-500">Always sky blue</span>
```

That's fine. Just don't use them for general UI surfaces or text.
