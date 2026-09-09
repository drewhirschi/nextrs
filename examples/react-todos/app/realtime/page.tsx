import { BTreeIndex, createCollection, eq } from "@tanstack/db";
import { queryCollectionOptions } from "@tanstack/query-db-collection";
import { useLiveInfiniteQuery } from "@tanstack/react-db";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  getApiTodos,
  getGetApiTodosQueryKey,
  usePatchApiTodosById,
  usePostApiTodos,
} from "@react-todos/client/react-query";
import type { Todo } from "@react-todos/client";
import {
  subscribeRealtime,
  type RealtimeChangeFrame,
  type RealtimeStatus,
} from "@react-todos/client/realtime";
import { TodoRow } from "../todo-row";

type DeliveryMode = "merge" | "refetch" | "notify";
const TOPIC = "household.demo.todos";
const PAGE_SIZE = 5;

function createTodosCollection(queryClient: ReturnType<typeof useQueryClient>) {
  const collection = createCollection(
    queryCollectionOptions({
      id: "realtime-household-todos",
      queryKey: [...getGetApiTodosQueryKey()],
      queryFn: ({ signal }) => getApiTodos(undefined, { signal }),
      select: (response) => response.data,
      queryClient,
      getKey: (todo) => todo.id,
      staleTime: 30_000,
    }),
  );
  collection.createIndex((todo) => todo.id, { indexType: BTreeIndex });
  return collection;
}

type TodosCollection = ReturnType<typeof createTodosCollection>;

export default function RealtimeLab() {
  const queryClient = useQueryClient();
  const collection = useMemo(() => createTodosCollection(queryClient), [queryClient]);
  const [mode, setMode] = useState<DeliveryMode>("merge");
  const [filter, setFilter] = useState<"all" | "open" | "done">("all");
  const [title, setTitle] = useState("");
  const realtimeOrigin = useMemo(() => new URLSearchParams(window.location.search).get("realtime_origin") ?? undefined, []);
  const stream = useRealtimeCollection(collection, mode, realtimeOrigin);

  useEffect(() => () => { void collection.cleanup(); }, [collection]);

  const live = useLiveInfiniteQuery(
    (q) => {
      const source = q.from({ todo: collection });
      const filtered = filter === "all"
        ? source
        : source.where(({ todo }) => eq(todo.done, filter === "done"));
      return filtered.orderBy(({ todo }) => todo.id, "desc");
    },
    { pageSize: PAGE_SIZE },
  );

  const addTodo = usePostApiTodos({
    mutation: {
      onSuccess: () => setTitle(""),
    },
  });
  const updateTodo = usePatchApiTodosById();

  return (
    <section className="realtime-lab">
      <div className="lab-heading">
        <div>
          <p className="eyebrow">Realtime primitive lab</p>
          <h1>A household is a live collection</h1>
        </div>
        <span className={`connection connection-${stream.status}`}>
          <i aria-hidden="true" /> {statusLabel(stream.status)}
        </span>
      </div>

      <p className="lab-intro">
        This page fetched one authoritative snapshot, attached to <code>{TOPIC}</code>,
        and turned the rows into a TanStack DB collection. Open it in two tabs,
        add or toggle a row, and compare how the same ordered event feels under
        each delivery policy.
      </p>

      <div className="mode-picker" role="group" aria-label="Realtime delivery policy">
        <ModeButton active={mode === "merge"} onClick={() => setMode("merge")} title="Auto merge" detail="Apply the delta immediately; the live page boundary moves." />
        <ModeButton active={mode === "refetch"} onClick={() => setMode("refetch")} title="Auto refetch" detail="Treat every event as invalidation and reload the snapshot." />
        <ModeButton active={mode === "notify"} onClick={() => setMode("notify")} title="Notify first" detail="Hold changes until the person chooses to refresh." />
      </div>

      {stream.pendingCount > 0 ? (
        <button className="updates-banner" type="button" onClick={stream.applyPending}>
          {stream.pendingCount} {stream.pendingCount === 1 ? "change" : "changes"} waiting · refresh this view
        </button>
      ) : null}

      <div className="row live-toolbar">
        <div className="segmented" role="group" aria-label="Filter collection">
          {(["all", "open", "done"] as const).map((value) => (
            <button key={value} type="button" aria-pressed={filter === value} onClick={() => setFilter(value)}>
              {value}
            </button>
          ))}
        </div>
        <span className="muted">{live.data.length} in the live window</span>
      </div>

      <ul className="list live-list">
        {live.data.map((todo) => (
          <TodoRow
            key={todo.id}
            todo={todo}
            onToggle={(item) => updateTodo.mutate({ id: item.id, data: { done: !item.done } })}
          />
        ))}
      </ul>

      <div className="paging-row">
        <button type="button" onClick={() => void live.fetchNextPage()} disabled={!live.hasNextPage || live.isFetchingNextPage}>
          {live.isFetchingNextPage ? "Loading…" : live.hasNextPage ? `Load ${PAGE_SIZE} more` : "End of collection"}
        </button>
        <span className="muted">New matching rows enter at the top; the loaded window stays bounded.</span>
      </div>

      <form className="add" onSubmit={(event) => {
        event.preventDefault();
        if (title.trim()) addTodo.mutate({ data: { title: title.trim() } });
      }}>
        <input value={title} onChange={(event) => setTitle(event.target.value)} placeholder="Simulate a household update…" />
        <button className="primary" type="submit" disabled={addTodo.isPending || !title.trim()}>Broadcast</button>
      </form>

      <div className="lab-footnotes">
        <p><strong>Transport:</strong> local Axum WebSocket now; the included Durable Object speaks the same frames in production.</p>
        <p><strong>Recovery:</strong> every attach and sequence gap refetches. Events improve latency; they never replace the database.</p>
      </div>
    </section>
  );
}

function ModeButton({ active, onClick, title, detail }: { active: boolean; onClick: () => void; title: string; detail: string }) {
  return <button type="button" aria-pressed={active} onClick={onClick}><strong>{title}</strong><span>{detail}</span></button>;
}

function useRealtimeCollection(collection: TodosCollection, mode: DeliveryMode, origin?: string) {
  const [status, setStatus] = useState<RealtimeStatus>("connecting");
  const [pending, setPending] = useState<Array<RealtimeChangeFrame<Todo>>>([]);
  const modeRef = useRef(mode);
  const pendingRef = useRef(pending);
  modeRef.current = mode;
  pendingRef.current = pending;

  const reconcile = useCallback(() => {
    void collection.utils.refetch({ throwOnError: false });
  }, [collection]);
  const apply = useCallback((changes: Array<RealtimeChangeFrame<Todo>>) => {
    let mustRefetch = false;
    collection.utils.writeBatch(() => {
      for (const change of changes) {
        if (change.operation === "upsert" && change.value) {
          collection.utils.writeUpsert(change.value);
        } else if (change.operation === "delete" && change.key) {
          const key = Number(change.key);
          if (collection.has(key)) collection.utils.writeDelete(key);
        } else if (change.operation === "invalidate") {
          mustRefetch = true;
        }
      }
    });
    if (mustRefetch) reconcile();
  }, [collection, reconcile]);

  useEffect(() => {
    const subscription = subscribeRealtime<Todo>({
      topic: TOPIC,
      origin,
      onStatus: setStatus,
      onReady: reconcile,
      onResync: reconcile,
      onChange: (change) => {
        if (modeRef.current === "merge") apply([change]);
        else if (modeRef.current === "refetch") reconcile();
        else setPending((current) => [...current, change]);
      },
    });
    return () => subscription.close();
  }, [apply, origin, reconcile]);

  const applyPending = useCallback(() => {
    // Notify-first deliberately re-fetches rather than replaying possibly old
    // deltas, which is the least surprising behavior for a photo feed.
    if (pendingRef.current.length === 0) return;
    setPending([]);
    reconcile();
  }, [reconcile]);

  return { status, pendingCount: pending.length, applyPending };
}

function statusLabel(status: RealtimeStatus) {
  if (status === "live") return "Live";
  if (status === "reconnecting") return "Reconnecting";
  if (status === "stopped") return "Stopped";
  return "Connecting";
}
