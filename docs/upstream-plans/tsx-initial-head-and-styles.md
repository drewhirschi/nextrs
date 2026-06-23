# TSX Initial Head And Styles

## Problem

TSX shells currently stream the React mount div and script before React has a
chance to render layout-level `<link>` tags. If `layout.tsx` includes
`<link rel="stylesheet" href="/style.css" />`, CSS arrives only after the
bundle loads and React mounts, causing a flash of unstyled content.

## Current Local Patch

`nextrs/src/build.rs` has a local first-pass patch that injects:

```html
<link rel="stylesheet" href="/style.css" />
```

before both the TSX page mount div and the TSX loading mount div. Tests were
added locally to keep the stylesheet before the client script path.

This is useful as a quick framework fix, but hard-coding `/style.css` should not
be the final API.

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
- The existing hard-coded stylesheet patch can be replaced once the explicit API
  exists.

## Validation

- Build test: TSX page shell includes configured/static head before the page
  mount div and script.
- Build test: TSX loading shell includes the same head before the loading mount
  div and script.
- Browser check: first paint includes styles without waiting for React mount.
