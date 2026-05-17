---
name: daisy-ui
description: Use whenever styling UI — writing or editing HTML, JSX, Askama/Tera templates, or any Tailwind class work. Provides daisyUI 5 component classes (on top of Tailwind CSS 4). Source-of-truth snapshot of daisyui.com/llms.txt is in reference/llms.txt.
---

# daisyUI 5

daisyUI is a component library for **Tailwind CSS 4**. It gives you semantic class names (`btn`, `card`, `alert`, …) instead of long Tailwind class soup, and a theme system with semantic colors (`primary`, `base-100`, `error`, …) that swap automatically per theme.

## How to use this skill (progressive disclosure)

**Step 1 — check setup before writing classes.** Confirm the project is wired up:
- Look for a CSS file with `@import "tailwindcss";` and `@plugin "daisyui";` (typically `src/styles/*.css`, `app.css`, or similar).
- Look for `daisyui` in `package.json` devDependencies, **or** a daisyUI CDN `<link>` in the root HTML.
- If neither is present → read [setup.md](setup.md) and walk the user through wiring it in. For Rust/Askama/static-HTML projects, also read [build.md](build.md) — there's no JS bundler doing it for you.

**Step 2 — pick the right category file** and load only that one:

| You're building… | Read |
|---|---|
| inputs, selects, checkboxes, radios, ranges, toggles, file uploads, labels, fieldsets, validators | [components/forms.md](components/forms.md) |
| cards, heroes, dividers, stacks, joins, footers, drawers, indicators, masks | [components/layout.md](components/layout.md) |
| navbars, menus, tabs, breadcrumbs, pagination, steps, docks, links | [components/navigation.md](components/navigation.md) |
| alerts, toasts, modals, tooltips, progress bars, loading spinners, skeletons, statuses | [components/feedback.md](components/feedback.md) |
| tables, lists, stats, badges, kbd, avatars, timelines, chat, accordions, collapses, diff, calendars, countdowns, ratings | [components/data-display.md](components/data-display.md) |
| buttons, dropdowns, swaps, FABs, theme controllers, filters, carousels | [components/actions.md](components/actions.md) |
| browser/code/phone/window mockups, hover-3d, hover-gallery, text-rotate | [components/mockups.md](components/mockups.md) |

**Step 3 — colors and themes.** When choosing colors, read [colors.md](colors.md). The short version: prefer semantic names (`bg-primary`, `text-base-content`, `bg-error/10`) over raw Tailwind colors (`bg-blue-500`) so the page works across themes.

**Step 4 — config / themes.** Only when you need to add/customize themes, change the prefix, or scope daisyUI to a subtree: read [config.md](config.md).

**Step 5 — the rules.** [usage-rules.md](usage-rules.md) lists daisyUI's own do/don't guidance. Skim it once per session if you haven't.

## Flat component index

If you're not sure which category file holds a component, find it here, then open the file.

```
forms       — checkbox, fieldset, file-input, input, label, radio, range, select, textarea, toggle, validator
layout      — card, divider, drawer, footer, hero, indicator, join, mask, stack
navigation  — breadcrumbs, dock, link, menu, navbar, pagination, steps, tab
feedback    — alert, loading, modal, progress, radial-progress, skeleton, status, toast, tooltip
data-display — accordion, avatar, badge, calendar, chat, collapse, countdown, diff, kbd, list, rating, stat, table, timeline
actions     — button, carousel, dropdown, fab, filter, swap, theme-controller
mockups     — hover-3d, hover-gallery, mockup-browser, mockup-code, mockup-phone, mockup-window, text-rotate
```

## Source of truth

The full upstream document is vendored at [reference/llms.txt](reference/llms.txt). To refresh it:

```bash
curl -fsSL https://daisyui.com/llms.txt -o .claude/skills/daisy-ui/reference/llms.txt
```

If a component looks like it's missing or out of date in the category files, check `reference/llms.txt` first before assuming.
