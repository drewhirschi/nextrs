# Action components

For triggering things — clicks, toggles, theme changes.

## button

The single most important class in daisyUI.

- **classes:** `btn`, `btn-{neutral|primary|secondary|accent|info|success|warning|error}`, `btn-{outline|dash|soft|ghost|link}`, `btn-{active|disabled}`, `btn-{xs|sm|md|lg|xl}`, `btn-{wide|block|square|circle}`

```html
<button class="btn">Default</button>
<button class="btn btn-primary">Primary</button>
<button class="btn btn-outline btn-error">Delete</button>
<button class="btn btn-ghost btn-sm">Cancel</button>
<button class="btn btn-circle btn-primary">+</button>
<button class="btn btn-square"><svg>…</svg></button>
<button class="btn btn-block">Full width</button>
```

- Use `btn` on any element: `<button>`, `<a>`, `<input type="submit">`, `<label>`.
- Disabling with a class (not the `disabled` attribute) needs ARIA: `<a class="btn btn-disabled" tabindex="-1" role="button" aria-disabled="true">`.
- Icons before/after text are common: `<button class="btn"><svg>…</svg> Save</button>`.

## dropdown

A click-triggered menu. Three implementation flavors.

- **classes:** `dropdown`, `dropdown-content`, `dropdown-{start|center|end|top|bottom|left|right}`, `dropdown-{hover|open|close}`

**Using `<details>`/`<summary>`** (simplest, native):
```html
<details class="dropdown">
  <summary class="btn">Menu</summary>
  <ul class="dropdown-content menu bg-base-100 rounded-box w-52 p-2 shadow">
    <li><a>Item 1</a></li>
    <li><a>Item 2</a></li>
  </ul>
</details>
```

**Using Popover API** (modern, recommended for anchored dropdowns):
```html
<button popovertarget="menu1" style="anchor-name:--m1" class="btn">Menu</button>
<ul class="dropdown dropdown-content menu bg-base-100 rounded-box w-52 p-2 shadow"
    popover id="menu1" style="position-anchor:--m1">
  <li><a>Item 1</a></li>
</ul>
```

**Using CSS focus** (legacy):
```html
<div class="dropdown">
  <div tabindex="0" role="button" class="btn">Menu</div>
  <ul tabindex="-1" class="dropdown-content menu bg-base-100 rounded-box w-52 p-2 shadow">
    <li><a>Item 1</a></li>
  </ul>
</div>
```

Content doesn't have to be a `<ul>` — can be any element.

## swap

Toggle between two pieces of content with optional animation.

- **classes:** `swap`, `swap-on`, `swap-off`, `swap-indeterminate`, `swap-active`, `swap-rotate`, `swap-flip`

```html
<label class="swap swap-rotate">
  <input type="checkbox" />
  <svg class="swap-on">🌞</svg>
  <svg class="swap-off">🌙</svg>
</label>
```

Or, controlled by a class:
```html
<div class="swap swap-active">
  <div class="swap-on">ON</div>
  <div class="swap-off">OFF</div>
</div>
```

Use only the checkbox **or** the `swap-active` class — not both.

## fab

Floating Action Button — bottom-corner action with a speed-dial expansion.

- **classes:** `fab`, `fab-close`, `fab-main-action`, `fab-flower`

**Single FAB:**
```html
<div class="fab">
  <button class="btn btn-lg btn-circle btn-primary">+</button>
</div>
```

**FAB with 3 speed-dial buttons:**
```html
<div class="fab">
  <div tabindex="0" role="button" class="btn btn-lg btn-circle btn-primary">+</div>
  <button class="btn btn-lg btn-circle">🖼️</button>
  <button class="btn btn-lg btn-circle">📁</button>
  <button class="btn btn-lg btn-circle">🎵</button>
</div>
```

**FAB with labels:**
```html
<div class="fab">
  <div tabindex="0" role="button" class="btn btn-lg btn-circle btn-primary">+</div>
  <div>Add image <button class="btn btn-lg btn-circle">🖼️</button></div>
  <div>Add file <button class="btn btn-lg btn-circle">📁</button></div>
</div>
```

**Flower layout** (quarter-circle arrangement):
```html
<div class="fab fab-flower">
  <div tabindex="0" role="button" class="btn btn-lg btn-circle btn-primary">+</div>
  <button class="fab-main-action btn btn-circle btn-lg">⭐</button>
  <div class="tooltip tooltip-left" data-tip="Image">
    <button class="btn btn-lg btn-circle">🖼️</button>
  </div>
  <div class="tooltip tooltip-left" data-tip="File">
    <button class="btn btn-lg btn-circle">📁</button>
  </div>
</div>
```

**Close button** (replaces the main button when open):
```html
<div class="fab">
  <div tabindex="0" role="button" class="btn btn-lg btn-circle btn-primary">+</div>
  <div class="fab-close">Close <span class="btn btn-circle btn-lg btn-error">✕</span></div>
  …
</div>
```

## theme-controller

A checkbox/radio that sets `data-theme` on `<html>` when checked. Pure CSS — no JS needed.

- **classes:** `theme-controller`

```html
<input type="checkbox" value="dark" class="theme-controller toggle" />

<select class="select theme-controller">
  <option value="light">Light</option>
  <option value="dark">Dark</option>
  <option value="cupcake">Cupcake</option>
</select>
```

The `value` must be a theme name enabled in your daisyUI config.

## filter

A group of radio buttons where choosing one hides the others and shows a reset button.

- **classes:** `filter`, `filter-reset`

```html
<form class="filter">
  <input class="btn btn-square" type="reset" value="×" />
  <input class="btn" type="radio" name="frameworks" aria-label="React" />
  <input class="btn" type="radio" name="frameworks" aria-label="Vue" />
  <input class="btn" type="radio" name="frameworks" aria-label="Svelte" />
</form>
```

Without a `<form>` (use only if you can't use a form):
```html
<div class="filter">
  <input class="btn filter-reset" type="radio" name="x" aria-label="×" />
  <input class="btn" type="radio" name="x" aria-label="A" />
  <input class="btn" type="radio" name="x" aria-label="B" />
</div>
```

Each filter group needs a unique `name`.

## carousel

Scroll-snap horizontal/vertical scroller.

- **classes:** `carousel`, `carousel-item`, `carousel-{start|center|end}`, `carousel-{horizontal|vertical}`

```html
<div class="carousel rounded-box">
  <div class="carousel-item w-full"><img src="/1.webp" class="w-full" /></div>
  <div class="carousel-item w-full"><img src="/2.webp" class="w-full" /></div>
  <div class="carousel-item w-full"><img src="/3.webp" class="w-full" /></div>
</div>
```

Each item is a `carousel-item` div. Add `w-full` to make each slide full-width. For partial-width slides (multi-slide-visible), use a fractional width like `w-1/2`.
