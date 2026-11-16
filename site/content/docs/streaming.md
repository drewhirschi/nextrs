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

## Preload on intent

You write plain `<a href>` links. The app shell works out which one you are
about to use and preloads it — the route's JS chunk, plus its `prefetch.rs`
data when it has one — before you click. There is nothing to configure and no
`<Link>` component to adopt.

It reads the same signals [ForesightJS](https://foresightjs.com/) does, because
that library is vendored into nextrs:

| Signal | Fires when |
| --- | --- |
| Mouse trajectory | the pointer's projected path crosses the link |
| Hover | the pointer is on the link |
| Tab | keyboard focus is a couple of tab stops away |
| Scroll | the link is about to scroll under the pointer |
| Touch | a finger goes down on the link |

Each URL preloads at most once every 30 seconds, whichever signal gets there
first. Prediction switches itself off on data-saver and slower-than-3G
connections; plain hover preload still works there. The predictor loads as its
own chunk after the page is interactive, so it never delays first paint.

A link marked `data-no-soft` is left alone entirely — no soft navigation, no
preload.

### See the prediction

Open the browser console on any nextrs app and run:

```js
__nx_preload_debug__()
```

A pink line now runs from the pointer to where it is predicted to be (blue
while scrolling), a green box flashes around a link the moment it is
preloaded, and the console logs each preload with the signal that caused it —
`[nx preload] mouse/trajectory /about`. The setting is remembered across
reloads; `__nx_preload_debug__(false)` turns it off.
