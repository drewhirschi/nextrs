# Layout components

Containers, grouping, and structural pieces.

## card

The workhorse content container.

- **classes:** `card`, `card-body`, `card-title`, `card-actions`, `card-border`, `card-dash`, `card-side`, `image-full`, `card-{xs|sm|md|lg|xl}`

```html
<div class="card bg-base-100 shadow-md">
  <figure><img src="/cover.webp" alt="Cover" /></figure>
  <div class="card-body">
    <h2 class="card-title">Title</h2>
    <p>Description goes here.</p>
    <div class="card-actions justify-end">
      <button class="btn btn-primary">Buy now</button>
    </div>
  </div>
</div>
```

- `<figure>` and `card-body` are both optional.
- Use `sm:card-side` (image left, body right) for a horizontal layout on larger screens.
- Put the image **after** `card-body` to make it appear at the bottom.

## hero

Big banner with a title and description.

- **classes:** `hero`, `hero-content`, `hero-overlay`

```html
<div class="hero min-h-screen bg-base-200">
  <div class="hero-content text-center">
    <div class="max-w-md">
      <h1 class="text-5xl font-bold">Hello there</h1>
      <p class="py-6">Sub-headline goes here.</p>
      <button class="btn btn-primary">Get started</button>
    </div>
  </div>
</div>
```

With a background image + overlay:
```html
<div class="hero min-h-screen" style="background-image:url(/bg.webp)">
  <div class="hero-overlay bg-opacity-60"></div>
  <div class="hero-content text-center text-neutral-content">…</div>
</div>
```

## divider

Horizontal or vertical separator. Can contain text.

- **classes:** `divider`, `divider-{color}`, `divider-vertical`, `divider-horizontal`, `divider-start`, `divider-end`

```html
<div class="divider">OR</div>

<div class="flex">
  <div class="grid h-32 flex-grow place-items-center">Left</div>
  <div class="divider divider-horizontal">OR</div>
  <div class="grid h-32 flex-grow place-items-center">Right</div>
</div>
```

Omit text for a plain rule.

## stack

Visually stacks children on top of each other (z-axis).

- **classes:** `stack`, `stack-top`, `stack-bottom`, `stack-start`, `stack-end`

```html
<div class="stack">
  <div class="card bg-primary text-primary-content">Top</div>
  <div class="card bg-accent text-accent-content">Middle</div>
  <div class="card bg-secondary text-secondary-content">Bottom</div>
</div>
```

Use `w-*` / `h-*` to size the stack — all children inherit that size.

## join

Glues elements together visually (rounds the outer corners of the group, removes inner radii).

- **classes:** `join`, `join-item`, `join-vertical`, `join-horizontal`

```html
<div class="join">
  <button class="btn join-item">One</button>
  <button class="btn join-item">Two</button>
  <button class="btn join-item">Three</button>
</div>
```

```html
<div class="join">
  <input class="input join-item" placeholder="Search" />
  <button class="btn join-item">Go</button>
</div>
```

Use `lg:join-horizontal` for responsive vertical-then-horizontal layouts.

## footer

Page footer with optional titled sections.

- **classes:** `footer`, `footer-title`, `footer-center`, `footer-horizontal`, `footer-vertical`

```html
<footer class="footer bg-base-200 p-10">
  <nav>
    <h6 class="footer-title">Services</h6>
    <a class="link link-hover">Branding</a>
    <a class="link link-hover">Design</a>
  </nav>
  <nav>
    <h6 class="footer-title">Company</h6>
    <a class="link link-hover">About</a>
  </nav>
</footer>
```

- Use `sm:footer-horizontal` to switch layout per breakpoint.
- Suggestion: `bg-base-200`.

## drawer

Sidebar-with-overlay layout. Uses a hidden checkbox for open/close state.

- **classes:** `drawer`, `drawer-toggle`, `drawer-content`, `drawer-side`, `drawer-overlay`, `drawer-end`, `drawer-open`
- **variants:** `is-drawer-open:`, `is-drawer-close:` (conditional utility prefixes)

```html
<div class="drawer">
  <input id="main-drawer" type="checkbox" class="drawer-toggle" />
  <div class="drawer-content">
    <!-- Navbar, page, footer all live here -->
    <label for="main-drawer" class="btn drawer-button">Open menu</label>
  </div>
  <div class="drawer-side">
    <label for="main-drawer" class="drawer-overlay" aria-label="close sidebar"></label>
    <ul class="menu bg-base-200 min-h-full w-80 p-4">
      <li><a>Item 1</a></li>
      <li><a>Item 2</a></li>
    </ul>
  </div>
</div>
```

Always-visible-on-desktop, toggleable-on-mobile:
```html
<div class="drawer lg:drawer-open">
  …
  <div class="drawer-content">
    <label for="main-drawer" class="btn drawer-button lg:hidden">Menu</label>
  </div>
  …
</div>
```

Collapsible (icons-only when closed, icons+text when open):
```html
<div class="drawer lg:drawer-open">
  <input id="d" type="checkbox" class="drawer-toggle" />
  <div class="drawer-content">…</div>
  <div class="drawer-side is-drawer-close:overflow-visible">
    <div class="is-drawer-close:w-14 is-drawer-open:w-64 bg-base-200 min-h-full">
      <ul class="menu">
        <li><button class="is-drawer-close:tooltip is-drawer-close:tooltip-right" data-tip="Home">
          🏠 <span class="is-drawer-close:hidden">Home</span>
        </button></li>
      </ul>
      <label for="d" class="btn btn-ghost btn-circle">↔️</label>
    </div>
  </div>
</div>
```

**Rules:**
- The `drawer-toggle` input needs a unique `id`. Change `main-drawer` per drawer.
- All page content (navbar, footer, everything) must live inside `drawer-content`.
- Use `lg:drawer-open` to permanently show the drawer at the `lg` breakpoint up.

## indicator

Places a small element on the corner of another (notification dots, "new" badges).

- **classes:** `indicator`, `indicator-item`, `indicator-start`, `indicator-center`, `indicator-end`, `indicator-top`, `indicator-middle`, `indicator-bottom`

```html
<div class="indicator">
  <span class="indicator-item badge badge-error">99+</span>
  <button class="btn">Inbox</button>
</div>
```

Place all `indicator-item` elements **before** the main content. Default placement is `indicator-end indicator-top`.

## mask

Clips an element to a shape.

- **classes:** `mask`, plus a shape — `mask-squircle`, `mask-heart`, `mask-hexagon`, `mask-hexagon-2`, `mask-decagon`, `mask-pentagon`, `mask-diamond`, `mask-square`, `mask-circle`, `mask-star`, `mask-star-2`, `mask-triangle`, `mask-triangle-2`, `mask-triangle-3`, `mask-triangle-4`
- **modifiers:** `mask-half-1`, `mask-half-2`

```html
<img class="mask mask-squircle w-24 h-24" src="/avatar.webp" />
<div class="mask mask-hexagon bg-primary w-32 h-32"></div>
```

Set size with Tailwind's `w-*` / `h-*`.
