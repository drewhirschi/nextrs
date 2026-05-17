# Feedback components

For communicating state, progress, and notifications.

## alert

Inline notification box.

- **classes:** `alert`, `alert-outline`, `alert-dash`, `alert-soft`, `alert-{info|success|warning|error}`, `alert-vertical`, `alert-horizontal`

```html
<div role="alert" class="alert alert-info">
  <svg class="h-6 w-6">…</svg>
  <span>New software update available.</span>
</div>

<div role="alert" class="alert alert-error alert-soft">
  <span>Could not connect to server.</span>
</div>
```

Use `sm:alert-horizontal` for responsive layouts.

## toast

Stacked notifications anchored to a corner.

- **classes:** `toast`, `toast-{start|center|end}`, `toast-{top|middle|bottom}`

```html
<div class="toast toast-top toast-end">
  <div class="alert alert-success">
    <span>Saved.</span>
  </div>
  <div class="alert alert-info">
    <span>New message.</span>
  </div>
</div>
```

Stack `alert`s inside. Show/hide via JS (add/remove the toast container or the alerts inside it).

## modal

Dialog / overlay. **Prefer the native `<dialog>` element** — it gets free focus trapping, ESC-to-close, and stacking.

- **classes:** `modal`, `modal-box`, `modal-action`, `modal-backdrop`, `modal-toggle`, `modal-open`, `modal-{top|middle|bottom|start|end}`

Recommended (HTML5 `<dialog>`):
```html
<button class="btn" onclick="my_modal.showModal()">Open</button>
<dialog id="my_modal" class="modal">
  <div class="modal-box">
    <h3 class="font-bold text-lg">Hello</h3>
    <p class="py-4">Press ESC or click outside to close.</p>
    <div class="modal-action">
      <form method="dialog">
        <button class="btn">Close</button>
      </form>
    </div>
  </div>
  <form method="dialog" class="modal-backdrop"><button>close</button></form>
</dialog>
```

Legacy (checkbox-controlled, for environments without `<dialog>`):
```html
<label for="m1" class="btn">Open</label>
<input type="checkbox" id="m1" class="modal-toggle" />
<div class="modal">
  <div class="modal-box">…</div>
  <label for="m1" class="modal-backdrop">close</label>
</div>
```

Use unique `id`s per modal.

## tooltip

Hover-message.

- **classes:** `tooltip`, `tooltip-content`, `tooltip-open`, `tooltip-{top|bottom|left|right}`, `tooltip-{color}`

```html
<div class="tooltip" data-tip="Save to library">
  <button class="btn">Save</button>
</div>

<div class="tooltip tooltip-right tooltip-primary" data-tip="More info">
  <button class="btn btn-circle">?</button>
</div>
```

`data-tip` is required for hover text. `tooltip-open` forces it open (e.g. for visible help text).

## progress

Linear progress bar. Use the native `<progress>` element.

- **classes:** `progress`, `progress-{color}`

```html
<progress class="progress" value="40" max="100"></progress>
<progress class="progress progress-success" value="70" max="100"></progress>
```

Always set `value` and `max`. Omit `value` for an indeterminate bar.

## radial-progress

Circular progress indicator. Uses a `<div>` (not `<progress>`) so text fits inside.

- **classes:** `radial-progress`

```html
<div class="radial-progress text-primary" style="--value:70;" aria-valuenow="70" role="progressbar">
  70%
</div>
```

CSS variables: `--value` (0–100), `--size` (default `5rem`), `--thickness`.

Always include `aria-valuenow` and `role="progressbar"` for screen readers.

## loading

Spinner / loading animation.

- **classes:** `loading`, `loading-{spinner|dots|ring|ball|bars|infinity}`, `loading-{size}`

```html
<span class="loading loading-spinner"></span>
<span class="loading loading-dots loading-lg text-primary"></span>
<button class="btn"><span class="loading loading-spinner"></span> Saving…</button>
```

## skeleton

Placeholder block for content that's still loading.

- **classes:** `skeleton`, `skeleton-text`

```html
<div class="skeleton h-32 w-32"></div>
<div class="skeleton h-4 w-full"></div>
<div class="skeleton skeleton-text">Loading data…</div>
```

Size with Tailwind `h-*` / `w-*`.

## status

A tiny colored dot to show online/offline/etc. Renders nothing visible by itself — it's just the dot.

- **classes:** `status`, `status-{color}`, `status-{size}`

```html
<span class="status status-success"></span>
<span class="status status-warning status-lg"></span>
<span>Online <span class="status status-success"></span></span>
```
