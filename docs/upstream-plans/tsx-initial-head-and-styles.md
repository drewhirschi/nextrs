# TSX Initial Head And Styles

## Problem

TSX shells currently stream the React mount div and script before React has a
chance to render layout-level `<link>` tags. If `layout.tsx` includes
`<link rel="stylesheet" href="/style.css" />`, CSS arrives only after the
bundle loads and React mounts, causing a flash of unstyled content.

## Current Behavior

`nextrs/src/build.rs` injects the bundled stylesheet before the page and
loading mount points:

```html
<link rel="stylesheet" href="/dist/style-<content-hash>.css" />
```

The generated asset table resolves the exact URL from the bundle manifest, so
the initial response is styled without a late React-rendered link or a cache
revalidation. Explicit application-defined document-head support remains a
separate API direction below.

## Proposed Direction

Add explicit document-head support for TSX shells:

- Start with app-level static head support such as `app/head.html`.
- Consider `app/head.rs` later if dynamic or config-driven head output is
  needed.
- Consider a config option for global stylesheets, especially for generated
  scaffold defaults.
- Ensure head output is present in the initial server response before any page
  or loading TSX script runs.

## Implementation Notes

- Head support should apply to page shells and loading shells.
- It should compose with nested layouts without requiring React to mount first.
- The generated stylesheet link remains the default when explicit head support
  is added.

## Validation

- Build test: TSX page shell includes configured/static head before the page
  mount div and script.
- Build test: TSX loading shell includes the same head before the loading mount
  div and script.
- Browser check: first paint includes styles without waiting for React mount.
