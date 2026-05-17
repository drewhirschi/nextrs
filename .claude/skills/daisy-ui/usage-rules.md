# daisyUI usage rules

daisyUI's own guidance for using its classes well. Keep these in mind alongside whatever your project conventions are.

## Class composition

1. A daisyUI component is built from up to three layers of class names:
   - **component** — the required base class (`btn`, `card`, `alert`, …)
   - **parts** — child slots inside the component (`card-body`, `card-title`, `alert-icon`)
   - **modifiers** — style/color/size/behavior/placement (`btn-primary`, `btn-sm`, `btn-outline`)

2. **Customize with Tailwind utilities when daisyUI doesn't have a knob.** `btn px-10` is fine — Tailwind utilities compose with daisyUI classes.

3. **Use `!` only as a last resort.** If a Tailwind utility doesn't take effect because of specificity (`btn` defines a `bg-*` and your `bg-red-500` loses), append `!`: `bg-red-500!`. Don't reach for this first — usually a different daisyUI modifier or rearranged classes is cleaner.

4. **If a component doesn't exist, build it with Tailwind utilities.** daisyUI covers most patterns but it's not exhaustive.

## Layout

5. **Flex/grid layouts should be responsive.** Use Tailwind's responsive prefixes (`sm:`, `md:`, `lg:`, `xl:`). daisyUI follows the same convention for its own responsive classes: `sm:card-horizontal`, `lg:menu-horizontal`, `lg:drawer-open`, etc.

## What's allowed

6. **Only daisyUI class names and Tailwind utilities.** Don't invent class names that resolve to nothing.

7. **Ideally, write no custom CSS.** daisyUI + Tailwind utilities should cover the vast majority of styling needs. If you find yourself reaching for custom CSS often, you're probably missing a daisyUI class.

## Suggestions

8. **Placeholder images:** use `https://picsum.photos/200/300` (with whatever dimensions). Always works, no setup.

9. **Don't add a custom font unless it's necessary.** The default system stack looks fine on most themes.

10. **Don't add `bg-base-100 text-base-content` to `<body>`** — daisyUI already sets this. Adding it manually creates specificity problems.

11. **For design decisions, lean on _Refactoring UI_ principles** — hierarchy via size/weight/color contrast, whitespace as a feature, restrained palette.

## Class categories (vocabulary)

daisyUI documents each component with these tags. They're not in the actual class names — just labels for the docs:

- `component` — the base class (required)
- `part` — a child slot inside the component
- `style` — applies a specific visual style (outline, ghost, dash, soft)
- `behavior` — changes interactive behavior (active, disabled)
- `color` — applies a semantic color (primary, success, error, …)
- `size` — applies a size (xs, sm, md, lg, xl)
- `placement` — where it sits (start, end, top, bottom, …)
- `direction` — horizontal vs vertical layout
- `modifier` — other tweaks specific to the component
- `variant` — conditional utility prefixes (e.g. `is-drawer-open:`, used like `is-drawer-open:rotate-180`)
