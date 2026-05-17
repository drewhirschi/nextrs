# Mockup and decorative components

For visual frames around content (browser/phone/code windows) and standalone visual effects.

## mockup-browser

A frame that looks like a browser window.

- **classes:** `mockup-browser`, `mockup-browser-toolbar`

```html
<div class="mockup-browser border border-base-300 bg-base-200">
  <div class="mockup-browser-toolbar">
    <div class="input">https://daisyui.com</div>
  </div>
  <div class="bg-base-100 flex justify-center px-4 py-16">
    Hello!
  </div>
</div>
```

The URL "input" is just an element with the `input` class for visual styling.

## mockup-code

Code-editor-style frame.

- **classes:** `mockup-code`

```html
<div class="mockup-code">
  <pre data-prefix="$"><code>npm install daisyui</code></pre>
  <pre data-prefix=">" class="text-warning"><code>installing…</code></pre>
  <pre data-prefix=">" class="text-success"><code>Done!</code></pre>
</div>
```

- `data-prefix="…"` adds a label before each line (line number, `$`, `>`).
- For syntax highlighting, drop in any code-highlighting library — daisyUI just provides the frame.

## mockup-phone

iPhone-style frame.

- **classes:** `mockup-phone`, `mockup-phone-camera`, `mockup-phone-display`

```html
<div class="mockup-phone">
  <div class="mockup-phone-camera"></div>
  <div class="mockup-phone-display">
    <img src="/screenshot.webp" alt="App screenshot" />
  </div>
</div>
```

Anything goes inside `mockup-phone-display` — images, full pages, live components.

## mockup-window

Generic OS window frame.

- **classes:** `mockup-window`

```html
<div class="mockup-window border border-base-300 bg-base-200">
  <div class="bg-base-100 flex justify-center px-4 py-16">
    Hello!
  </div>
</div>
```

## hover-3d

A wrapper that applies a subtle 3D tilt on mouse hover. Works via 8 invisible hover zones.

- **classes:** `hover-3d`

```html
<div class="hover-3d my-12 mx-2">
  <figure class="max-w-100 rounded-2xl">
    <img src="/card.webp" alt="3D card" />
  </figure>
  <div></div><div></div><div></div><div></div>
  <div></div><div></div><div></div><div></div>
</div>
```

**Rules:**
- Can be a `<div>` or `<a>` (the `<a>` form makes the whole thing clickable, which is the supported way to make it interactive).
- Must have **exactly 9 direct children** — the first is your content, the other 8 are empty `<div>`s used as hover zones.
- Don't put interactive elements (buttons, inputs, links) **inside** the content — they conflict with the hover detection. If the whole card needs to be clickable, make the `hover-3d` element itself an `<a>`.

## hover-gallery

A frame holding multiple images. The first shows by default; hovering horizontally reveals the others.

- **classes:** `hover-gallery`

```html
<figure class="hover-gallery max-w-60">
  <img src="/hat-1.webp" />
  <img src="/hat-2.webp" />
  <img src="/hat-3.webp" />
  <img src="/hat-4.webp" />
</figure>
```

**Rules:**
- Up to 10 images.
- Needs a `max-w-*` constraint, otherwise it fills its parent.
- All images should be the same dimensions for clean alignment.

## text-rotate

A loop-animated text rotation. 2–6 lines, swaps automatically every 10 seconds.

- **classes:** `text-rotate`

```html
<span class="text-rotate text-7xl font-bold max-md:text-3xl">
  <span class="justify-items-center">
    <span>DESIGN</span>
    <span>DEVELOP</span>
    <span>DEPLOY</span>
    <span>SCALE</span>
    <span>MAINTAIN</span>
    <span>REPEAT</span>
  </span>
</span>
```

Inline with surrounding text:
```html
<span>
  Built for
  <span class="text-rotate">
    <span>
      <span class="bg-teal-400 text-teal-800 px-2">Designers</span>
      <span class="bg-red-400 text-red-800 px-2">Developers</span>
      <span class="bg-blue-400 text-blue-800 px-2">Managers</span>
    </span>
  </span>
</span>
```

**Rules:**
- `text-rotate` must have a single span/div inside, which contains 2–6 inner span/divs (one per line).
- Default loop duration is 10 seconds. Override with `duration-{ms}` (Tailwind utility) — e.g. `duration-12000` for 12s.
- Animation pauses on hover.
