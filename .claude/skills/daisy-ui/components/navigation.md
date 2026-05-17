# Navigation components

For moving between pages and sections.

## navbar

Top bar. Use the three slots (`navbar-start`, `navbar-center`, `navbar-end`) to position content horizontally.

- **classes:** `navbar`, `navbar-start`, `navbar-center`, `navbar-end`

```html
<div class="navbar bg-base-200">
  <div class="navbar-start">
    <a class="btn btn-ghost text-xl">Brand</a>
  </div>
  <div class="navbar-center hidden lg:flex">
    <ul class="menu menu-horizontal">
      <li><a>Docs</a></li>
      <li><a>Blog</a></li>
    </ul>
  </div>
  <div class="navbar-end">
    <button class="btn btn-primary">Sign in</button>
  </div>
</div>
```

Suggestion: `bg-base-200`.

## menu

Vertical or horizontal list of links/buttons. Often nested inside a `drawer`, `navbar`, or `dropdown`.

- **classes:** `menu`, `menu-title`, `menu-dropdown`, `menu-dropdown-toggle`, `menu-disabled`, `menu-active`, `menu-focus`, `menu-dropdown-show`, `menu-{size}`, `menu-vertical`, `menu-horizontal`

```html
<ul class="menu bg-base-200 rounded-box w-56">
  <li><a>Item 1</a></li>
  <li><a class="menu-active">Item 2</a></li>
  <li><a class="menu-disabled">Item 3</a></li>
</ul>
```

Horizontal:
```html
<ul class="menu menu-horizontal bg-base-200 rounded-box">
  <li><a>Home</a></li>
  <li><a>About</a></li>
</ul>
```

With submenu (collapsible via `<details>`):
```html
<ul class="menu">
  <li>
    <details open>
      <summary>Parent</summary>
      <ul>
        <li><a>Child 1</a></li>
        <li><a>Child 2</a></li>
      </ul>
    </details>
  </li>
</ul>
```

- Use `lg:menu-horizontal` for responsive layouts.
- `menu-title` for non-clickable section headers.
- For JS-controlled submenus, use `menu-dropdown` / `menu-dropdown-toggle`.

## tab

Tabbed content navigation.

- **classes:** `tabs`, `tab`, `tab-content`, `tabs-box`, `tabs-border`, `tabs-lift`, `tab-active`, `tab-disabled`, `tabs-top`, `tabs-bottom`

Using buttons (no actual content switching — handle that with JS or CSS):
```html
<div role="tablist" class="tabs tabs-bordered">
  <button role="tab" class="tab">Tab 1</button>
  <button role="tab" class="tab tab-active">Tab 2</button>
  <button role="tab" class="tab">Tab 3</button>
</div>
```

Using radios (built-in content switching via CSS sibling selectors):
```html
<div role="tablist" class="tabs tabs-lift">
  <input type="radio" name="t" role="tab" class="tab" aria-label="Tab 1" />
  <div role="tabpanel" class="tab-content bg-base-100 p-6">Content 1</div>

  <input type="radio" name="t" role="tab" class="tab" aria-label="Tab 2" checked />
  <div role="tabpanel" class="tab-content bg-base-100 p-6">Content 2</div>
</div>
```

## breadcrumbs

Path navigation. Just the wrapper class.

- **classes:** `breadcrumbs`

```html
<div class="breadcrumbs text-sm">
  <ul>
    <li><a>Home</a></li>
    <li><a>Documents</a></li>
    <li>Add Document</li>
  </ul>
</div>
```

Scrolls horizontally if it overflows. Icons inside `<a>` work fine.

## pagination

Just `join` + `btn`s under the hood — no dedicated class.

- **classes:** `join`, `join-item`

```html
<div class="join">
  <button class="join-item btn">«</button>
  <button class="join-item btn">1</button>
  <button class="join-item btn btn-active">2</button>
  <button class="join-item btn">3</button>
  <button class="join-item btn">»</button>
</div>
```

## steps

Sequential process visualization.

- **classes:** `steps`, `step`, `step-icon`, `step-{color}`, `steps-vertical`, `steps-horizontal`

```html
<ul class="steps">
  <li class="step step-primary">Register</li>
  <li class="step step-primary">Choose plan</li>
  <li class="step">Purchase</li>
  <li class="step">Done</li>
</ul>
```

Vertical:
```html
<ul class="steps steps-vertical">
  <li class="step step-primary">Step 1</li>
  <li class="step">Step 2</li>
</ul>
```

With custom icon content via `data-content`:
```html
<ul class="steps">
  <li data-content="?" class="step step-neutral">Question</li>
  <li data-content="!" class="step step-warning">Warning</li>
  <li data-content="✓" class="step step-success">Done</li>
</ul>
```

## dock

Bottom navigation bar (mobile-style).

- **classes:** `dock`, `dock-label`, `dock-active`, `dock-{size}`

```html
<div class="dock">
  <button>
    <svg>…</svg>
    <span class="dock-label">Home</span>
  </button>
  <button class="dock-active">
    <svg>…</svg>
    <span class="dock-label">Search</span>
  </button>
  <button>
    <svg>…</svg>
    <span class="dock-label">Profile</span>
  </button>
</div>
```

For iOS safe-area: add `<meta name="viewport" content="viewport-fit=cover">` to `<head>`.

## link

Adds the standard underline back to `<a>` (Tailwind 4 resets it).

- **classes:** `link`, `link-hover`, `link-{color}`

```html
<a class="link link-primary">Read more</a>
<a class="link link-hover">Hover to underline</a>
```
