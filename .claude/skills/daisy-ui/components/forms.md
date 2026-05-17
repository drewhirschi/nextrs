# Form components

Components for user input. All form-control components support `*-xs / *-sm / *-md / *-lg / *-xl` for size and `*-primary / *-secondary / *-accent / *-info / *-success / *-warning / *-error / *-neutral` for color.

## input

Text input.

- **classes:** `input`, `input-ghost`, `input-{color}`, `input-{size}`

```html
<input type="text" placeholder="Type here" class="input input-bordered" />
<input type="email" class="input input-primary" placeholder="you@example.com" />
```

When you have multiple elements inside an input (icon + field), put `input` on the **parent** wrapper:
```html
<label class="input">
  <svg class="h-4 w-4 opacity-50">…</svg>
  <input type="text" placeholder="Search" />
</label>
```

## textarea

Multi-line text input. Same modifiers as `input`.

- **classes:** `textarea`, `textarea-ghost`, `textarea-{color}`, `textarea-{size}`

```html
<textarea class="textarea" placeholder="Bio"></textarea>
```

## select

Dropdown picker.

- **classes:** `select`, `select-ghost`, `select-{color}`, `select-{size}`

```html
<select class="select">
  <option disabled selected>Pick one</option>
  <option>Red</option>
  <option>Blue</option>
</select>
```

## checkbox

- **classes:** `checkbox`, `checkbox-{color}`, `checkbox-{size}`

```html
<input type="checkbox" class="checkbox checkbox-primary" />
```

## radio

Each group must share a unique `name`.

- **classes:** `radio`, `radio-{color}`, `radio-{size}`

```html
<input type="radio" name="plan" class="radio" />
<input type="radio" name="plan" class="radio radio-primary" checked />
```

## toggle

A checkbox styled as a switch.

- **classes:** `toggle`, `toggle-{color}`, `toggle-{size}`

```html
<input type="checkbox" class="toggle toggle-success" />
```

## range

Slider. Needs `min` and `max`.

- **classes:** `range`, `range-{color}`, `range-{size}`

```html
<input type="range" min="0" max="100" value="40" class="range range-primary" />
```

## file-input

- **classes:** `file-input`, `file-input-ghost`, `file-input-{color}`, `file-input-{size}`

```html
<input type="file" class="file-input file-input-bordered" />
```

## label

Two flavors:

**Inline label** (the parent gets `input` to merge label and field visually):
```html
<label class="input">
  <span class="label">Email</span>
  <input type="text" placeholder="you@example.com" />
</label>
```

**Floating label** (floats above on focus):
```html
<label class="floating-label">
  <input type="text" placeholder="Type here" class="input" />
  <span>Email</span>
</label>
```

## fieldset

Group related fields with a title and description.

```html
<fieldset class="fieldset">
  <legend class="fieldset-legend">Contact info</legend>
  <input type="text" class="input" placeholder="Name" />
  <input type="email" class="input" placeholder="Email" />
  <p class="label">We'll never share your email.</p>
</fieldset>
```

## validator

Auto-colors a field based on native HTML5 validation.

```html
<input type="email" class="input validator" required />
<p class="validator-hint">Please enter a valid email</p>
```

Works with `input`, `select`, `textarea`. Uses `:invalid`/`:valid` under the hood — provide `required`, `pattern`, `min`, `max`, `type`, etc.
