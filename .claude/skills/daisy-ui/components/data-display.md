# Data-display components

For showing content, data, and information.

## table

- **classes:** `table`, `table-zebra`, `table-pin-rows`, `table-pin-cols`, `table-{size}`

```html
<div class="overflow-x-auto">
  <table class="table table-zebra">
    <thead>
      <tr><th>Name</th><th>Email</th><th>Role</th></tr>
    </thead>
    <tbody>
      <tr><td>Alice</td><td>alice@example.com</td><td>Admin</td></tr>
      <tr><td>Bob</td><td>bob@example.com</td><td>User</td></tr>
    </tbody>
  </table>
</div>
```

Always wrap in `overflow-x-auto` so it scrolls on narrow screens.

## list

Vertical row layout.

- **classes:** `list`, `list-row`, `list-col-wrap`, `list-col-grow`

```html
<ul class="list bg-base-100 rounded-box shadow-md">
  <li class="list-row">
    <img class="size-10 rounded-box" src="/avatar.webp" />
    <div>
      <div>Dio Lupa</div>
      <div class="text-xs uppercase opacity-60">Remaining Reason</div>
    </div>
    <button class="btn btn-square btn-ghost">▶</button>
  </li>
</ul>
```

By default the **second** child of `list-row` fills the remaining space. Add `list-col-grow` to a different child to override. Use `list-col-wrap` to force a child onto a new line.

## stat

Big number / KPI block.

- **classes:** `stats`, `stat`, `stat-title`, `stat-value`, `stat-desc`, `stat-figure`, `stat-actions`, `stats-vertical`, `stats-horizontal`

```html
<div class="stats shadow">
  <div class="stat">
    <div class="stat-title">Total Page Views</div>
    <div class="stat-value">89,400</div>
    <div class="stat-desc">21% more than last month</div>
  </div>
  <div class="stat">
    <div class="stat-figure text-primary"><svg>…</svg></div>
    <div class="stat-title">New Users</div>
    <div class="stat-value text-primary">4,200</div>
    <div class="stat-actions"><button class="btn btn-sm">View</button></div>
  </div>
</div>
```

Default direction is horizontal. Use `stats-vertical` for stacked.

## badge

Status / count chip.

- **classes:** `badge`, `badge-outline`, `badge-dash`, `badge-soft`, `badge-ghost`, `badge-{color}`, `badge-{size}`

```html
<span class="badge">Default</span>
<span class="badge badge-primary">Primary</span>
<span class="badge badge-error badge-outline">Critical</span>
```

Empty badge: just leave the content blank — `<span class="badge badge-xs badge-primary"></span>`.

## kbd

Keyboard shortcut display.

- **classes:** `kbd`, `kbd-{size}`

```html
<kbd class="kbd">⌘</kbd>
<kbd class="kbd kbd-sm">K</kbd>
<p>Press <kbd class="kbd kbd-sm">⌘</kbd> + <kbd class="kbd kbd-sm">K</kbd> to search</p>
```

## avatar

Thumbnail image (often a user photo).

- **classes:** `avatar`, `avatar-group`, `avatar-online`, `avatar-offline`, `avatar-placeholder`

```html
<div class="avatar">
  <div class="w-12 rounded-full">
    <img src="/u.webp" />
  </div>
</div>

<div class="avatar avatar-online">
  <div class="w-24 rounded-full"><img src="/u.webp" /></div>
</div>

<div class="avatar-group -space-x-6">
  <div class="avatar"><div class="w-12"><img src="/a.webp" /></div></div>
  <div class="avatar"><div class="w-12"><img src="/b.webp" /></div></div>
  <div class="avatar avatar-placeholder">
    <div class="w-12 bg-neutral text-neutral-content"><span>+9</span></div>
  </div>
</div>
```

Combine with `mask` shapes (`mask-squircle`, `mask-hexagon`, `mask-triangle`) for non-circular avatars.

## timeline

Chronological events.

- **classes:** `timeline`, `timeline-start`, `timeline-middle`, `timeline-end`, `timeline-snap-icon`, `timeline-box`, `timeline-compact`, `timeline-vertical`, `timeline-horizontal`

```html
<ul class="timeline timeline-vertical">
  <li>
    <div class="timeline-start">1984</div>
    <div class="timeline-middle">
      <svg class="h-5 w-5 text-primary">…</svg>
    </div>
    <div class="timeline-end timeline-box">First Macintosh</div>
    <hr/>
  </li>
  <li>
    <hr/>
    <div class="timeline-start">2007</div>
    <div class="timeline-middle"><svg>…</svg></div>
    <div class="timeline-end timeline-box">iPhone</div>
  </li>
</ul>
```

- Default is vertical.
- `timeline-snap-icon` snaps the icon to the start instead of centering.
- `timeline-compact` puts all items on one side.

## chat

Conversation bubbles.

- **classes:** `chat`, `chat-image`, `chat-header`, `chat-footer`, `chat-bubble`, `chat-start`, `chat-end`, `chat-bubble-{color}`

```html
<div class="chat chat-start">
  <div class="chat-image avatar">
    <div class="w-10 rounded-full"><img src="/u.webp" /></div>
  </div>
  <div class="chat-header">Anakin <time class="text-xs opacity-50">12:45</time></div>
  <div class="chat-bubble">I have the high ground.</div>
  <div class="chat-footer opacity-50">Delivered</div>
</div>

<div class="chat chat-end">
  <div class="chat-bubble chat-bubble-error">You underestimate my power!</div>
</div>
```

Placement (`chat-start` or `chat-end`) is required.

## accordion

Show-and-hide where only one item stays open at a time. Built from radio inputs.

- **classes:** `collapse`, `collapse-title`, `collapse-content`, `collapse-arrow`, `collapse-plus`, `collapse-open`, `collapse-close`

```html
<div class="collapse collapse-arrow bg-base-200">
  <input type="radio" name="my-acc" checked />
  <div class="collapse-title font-medium">Click to open #1</div>
  <div class="collapse-content"><p>Hidden content #1</p></div>
</div>
<div class="collapse collapse-arrow bg-base-200">
  <input type="radio" name="my-acc" />
  <div class="collapse-title font-medium">Click to open #2</div>
  <div class="collapse-content"><p>Hidden content #2</p></div>
</div>
```

All radios in one accordion group must share a `name`. Different accordions on the same page → different names.

## collapse

Show-and-hide for a single item (no group behavior).

- **classes:** same as accordion (`collapse`, `collapse-title`, `collapse-content`, modifiers)

```html
<div tabindex="0" class="collapse collapse-plus bg-base-200">
  <div class="collapse-title font-medium">Click to expand</div>
  <div class="collapse-content">Hidden content here.</div>
</div>
```

Or use `<details>`/`<summary>`, or an `<input type="checkbox">` as first child.

## countdown

Animated number 0–999 (CSS animation, no JS needed for the transition — but you do need JS to change the value).

- **classes:** `countdown`

```html
<span class="countdown">
  <span style="--value:42;" aria-live="polite" aria-label="42">42</span>
</span>
```

`--value` must equal the visible text. Update both via JS.

## diff

Side-by-side comparison slider.

- **classes:** `diff`, `diff-item-1`, `diff-item-2`, `diff-resizer`

```html
<figure class="diff aspect-16/9">
  <div class="diff-item-1"><img src="/a.webp" /></div>
  <div class="diff-item-2"><img src="/b.webp" /></div>
  <div class="diff-resizer"></div>
</figure>
```

Add an aspect-ratio utility (`aspect-16/9`, `aspect-square`, …) to keep the slider proportional.

## calendar

daisyUI styles three third-party calendar libraries — it doesn't ship its own.

- **classes:** `cally` (for Cally), `pika-single` (for Pikaday input), `react-day-picker` (for DayPicker)

```html
<!-- Cally web component -->
<calendar-date class="cally">…</calendar-date>

<!-- Pikaday -->
<input type="text" class="input pika-single" />

<!-- React Day Picker -->
<DayPicker className="react-day-picker" />
```

## rating

Star (or other shape) rating built from radios.

- **classes:** `rating`, `rating-half`, `rating-hidden`, `rating-{size}`

```html
<div class="rating">
  <input type="radio" name="rating-1" class="rating-hidden" />
  <input type="radio" name="rating-1" class="mask mask-star" />
  <input type="radio" name="rating-1" class="mask mask-star" />
  <input type="radio" name="rating-1" class="mask mask-star" checked />
  <input type="radio" name="rating-1" class="mask mask-star" />
  <input type="radio" name="rating-1" class="mask mask-star" />
</div>
```

- Each rating group needs a unique `name`.
- Use `rating-hidden` on a first radio to allow clearing the rating.
- Combine with mask shapes (`mask-heart`, `mask-star-2`, etc.) for non-star ratings.
