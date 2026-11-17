+++
title = "Loading and Prefetch"
description = "Keep React navigation responsive while route code and server data are prepared"
section = "Guides"
order = 3
+++

nextrs uses React loading components and server-warmed React Query data to keep
navigation responsive. The supported frontend conventions are `page.tsx`,
`layout.tsx`, and `loading.tsx`.

## Loading UI

Add `loading.tsx` beside a page:

```tsx
export default function LoadingTodos() {
  return <p>Loading todos…</p>;
}
```

The app shell can show this component while the route bundle and data become
available. Keep it small and independent of the data it is waiting for.

## Prefetch server data

Add `prefetch.rs` beside the same `page.tsx` to warm its React Query cache. On
a hard load, nextrs puts those entries into the page shell before React mounts.
On link intent and soft navigation, the app shell preloads the target route and
calls the same prefetch path automatically.

The page itself continues using an ordinary generated hook:

```tsx
import { useGetApiTodos } from "@mysite/client/react-query";

export default function TodosPage() {
  const { data, isPending } = useGetApiTodos();

  if (isPending) return <p>Loading todos…</p>;
  return <ul>{data?.data.map((todo) => <li key={todo.id}>{todo.title}</li>)}</ul>;
}
```

Delete `prefetch.rs` and the component still works; its hook simply fetches on
mount. Prefetch is an optimization, not a second frontend data model.

See [React Pages & Server Prefetch](/docs/react-server-props) for the complete
server-seeding flow.

## Preload

You write plain `<a href>` links. The app shell preloads their destinations
before you click — nothing to configure, no `<Link>` component to adopt. It
treats the two halves of a route differently, by what they cost:

- **The route's JS chunk** is a static, content-hashed file, so it is warmed
  for every app link as soon as the link is on screen. Fifty links to
  `/todos/{id}` are one chunk.
- **The route's `prefetch.rs` data** is a function invocation that is only
  fresh for 30 seconds, so it is warmed only when you show intent to go there.

Intent is read from the same signals [ForesightJS](https://foresightjs.com/)
uses, because that library is vendored into nextrs:

| Signal | Fires when |
| --- | --- |
| Mouse trajectory | the pointer's projected path crosses the link |
| Hover | the pointer is on the link |
| Tab | keyboard focus is a couple of tab stops away |
| Scroll | the link is about to scroll under the pointer |
| Touch | a finger goes down on the link |

Each URL preloads at most once every 30 seconds, whichever signal gets there
first. On data-saver and very slow connections the on-screen chunk warming and
the predictors switch themselves off; plain hover still works. The predictor
loads as its own chunk after the page is interactive, so it never delays first
paint.

A link marked `data-no-soft` is left alone entirely — no soft navigation, no
preload.

### See the prediction

Open the browser console on any nextrs app and run:

```js
__nx_preload_debug__()
```

A pink line now runs from the pointer to where it is predicted to be (blue
while scrolling), a green box flashes around a link the moment a predictor
fires for it, and the console logs each preload with the signal that started
it — `[nx preload] mouse/trajectory /todos/3`. The setting is remembered
across reloads; `__nx_preload_debug__(false)` turns it off.

### Is it helping? Measure it

Click around your app for a minute, then run:

```js
__nx_preload_stats__()
```

For every soft navigation the shell recorded how long the destination took to
load and how much head start hover and the predictors each had before your
click. From those it works out the wait you would have felt under each
strategy, and what each one cost:

```
[nx preload] 14 soft navigation(s) this page session
┌────────────┬──────────────────┬───────────────┬─────────────┐
│            │ median wait (ms) │ p90 wait (ms) │ instant (%) │
├────────────┼──────────────────┼───────────────┼─────────────┤
│ no preload │ 212              │ 480           │ 0           │
│ hover only │ 31               │ 290           │ 43          │
│ predictive │ 0                │ 140           │ 71          │
└────────────┴──────────────────┴───────────────┴─────────────┘
┌───────────┬────────────────┬─────────────────────┬────────────┐
│           │ preloads fired │ followed by a click │ wasted (%) │
├───────────┼────────────────┼─────────────────────┼────────────┤
│ hover     │ 6              │ 4                   │ 33         │
│ predicted │ 21             │ 10                  │ 52         │
└───────────┴────────────────┴─────────────────────┴────────────┘
```

*(Illustrative numbers.)* If "predictive" barely beats "hover only" while
wasting far more preloads, prediction is not paying for itself in your app.
To collect this from real users instead of your own clicks, see
[Preload stats](/docs/telemetry#preload-stats).
