# Loading Entry Throws When Its Container Is Already Gone

- **Reported-in:** onenote-extractor (2026-11-22, nextrs 0.6.0; the generated
  code is unchanged at 0.7.0 / `4b915a9`; seen again 2026-11-25 on 0.8.0,
  `bundle.rs` still mounts with the non-null assertion)
- **Status:** fixed in branch `fix/loading-entry-missing-container`

## Problem

Every hard load of a route that has both `loading.tsx` and `prefetch.rs` logs an
uncaught error in the browser:

```
Error: Target container is not a DOM element.
    at exports.createRoot (/dist/chunks/client-….js)
    at /dist/j-_jid_-source-_id_.loading-….js:8:30
```

Seen on `app/j/[jid]/source/[id]/` in onenote-extractor (debug build, local
SQLite, so the prefetch resolves in a few milliseconds). The page itself renders
fine, so nothing looks broken — but it is an uncaught `pageerror` on every
visit, which fails any e2e check that asserts a clean console and buries real
errors.

The loading shell is `<div id="__nx_loading_root__"></div><script>import("…loading.js")</script>`
(`tsx_loading_shell_with_src`, `build.rs`), and the generated entry mounts with
a non-null assertion (`loading_entry_wrapper`, `bundle.rs`):

```tsx
createRoot(document.getElementById("__nx_loading_root__")!).render(<Loading />);
```

`import()` resolves asynchronously. `getElementById` returning null means the
loading root is no longer in the document by the time the module runs — most
likely the prefetch finished first and the real shell replaced it. That exact
ordering was not traced; what is certain is that the container is null when the
entry executes, on every hard load, not intermittently.

## Proposed Direction

A loading entry that arrives after its container is gone has nothing left to do.
Make the wrapper a no-op in that case instead of asserting:

```tsx
const el = document.getElementById("__nx_loading_root__");
if (el) createRoot(el).render(<Loading />);
```

Worth checking at the same time whether the loading UI is ever visible on a
fast prefetch, or whether the shell is always swapped before the import lands —
if the latter, the loading module is being fetched for nothing on those loads.

## Implementation Notes

- `loading_entry_wrapper` in `crates/nextrs/src/bundle.rs` guards `document.getElementById("__nx_loading_root__")`
  and registers `window.__nx_loading_unmount = () => root.unmount()` when mounted.
- `NX_SWAP_SCRIPT` in `crates/nextrs/src/router.rs` invokes and deletes
  `window.__nx_loading_unmount` before replacing `#__nx_slot__` so a mounted
  loading root is cleanly unmounted rather than orphaned.
- The island wrapper already iterates `querySelectorAll`, so it has no
  equivalent problem.

## Validation

- Unit: `loading_entry_wrapper_mounts_loading_component` in `crates/nextrs/src/bundle.rs`
  asserts the null guard and unmount hook; `test_loading_stream_contains_loading_then_page`
  in `crates/nextrs/src/router.rs` verifies the swap script unmounts before swapping.
- Browser: an app route with `loading.tsx` + a fast `prefetch.rs`, hard-loaded
  with a `pageerror` listener, logs nothing.
- Browser: the same route with a slow prefetch still shows the loading UI.
