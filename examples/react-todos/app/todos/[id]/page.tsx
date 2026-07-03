// Todo detail — a dynamic route ([id]). The `params` prop comes from the
// framework: TanStack Router's live match under soft navigation, the server's
// __nx_params__ tag on a hard load. First paint is seeded by the sibling
// prefetch.rs (soft navigations too, via the app shell's /__nx/prefetch).
import {
  useGetApiTodosByIdFromUrl,
  useUpdateTodo,
  useParams,
  getGetApiTodosByIdQueryKey,
  getGetTodosQueryKey,
} from "@react-todos/client";
import { useQueryClient } from "@tanstack/react-query";

// A "deep" component that needs the route param but doesn't get the `params`
// prop threaded down — useParams() reads the app-shell router's live match.
function Permalink() {
  const { id } = useParams<{ id: string }>();
  return <code className="muted">/todos/{id}</code>;
}

export default function TodoDetail({ params }: { params: { id: string } }) {
  const id = Number(params.id);
  const queryClient = useQueryClient();
  // A path+query route: the id (identity — which todo) is an explicit
  // argument, and the query options (?neighbors=) bind to the page URL, so
  // "showing neighbors" is shareable/back-forwardable view state.
  const {
    data,
    isFetching,
    params: urlParams,
    setParams,
  } = useGetApiTodosByIdFromUrl(id);
  // The handler is fallible (Result<Json<TodoDetail>, 404>) — the generated
  // response type is a status union, so narrow on it.
  const todo = data?.status === 200 ? data.data : undefined;

  // A todo's state shows on TWO surfaces: this detail entry and the list
  // (whose key is a different URL, so it's not a prefix of this one). With
  // soft navigation the list stays cached across pages — invalidate both, or
  // going back shows a stale badge.
  const updateTodo = useUpdateTodo({
    mutation: {
      onSuccess: () => {
        queryClient.invalidateQueries({ queryKey: getGetApiTodosByIdQueryKey(id) });
        queryClient.invalidateQueries({ queryKey: getGetTodosQueryKey() });
      },
    },
  });

  return (
    <section>
      <h1>
        Todo #{params.id} <Permalink />
      </h1>
      {todo ? (
        <div className="detail">
          <span className={`badge ${todo.done ? "badge-done" : "badge-open"}`}>
            {todo.done ? "Done" : "Open"}
          </span>
          <strong className={todo.done ? "struck" : ""}>{todo.title}</strong>
          <button
            onClick={() => updateTodo.mutate({ id, data: { done: !todo.done } })}
            disabled={updateTodo.isPending}
          >
            {todo.done ? "Reopen" : "Mark done"}
          </button>
        </div>
      ) : (
        <p className="muted">{isFetching ? "Loading…" : "No such todo."}</p>
      )}

      <p className="row">
        <label className="muted">
          <input
            type="checkbox"
            checked={urlParams.neighbors ?? false}
            onChange={(e) => setParams({ neighbors: e.target.checked || undefined })}
          />{" "}
          Show neighbors (URL state: <code>?neighbors=true</code>)
        </label>
        {todo?.prev != null && <a href={`/todos/${todo.prev}?neighbors=true`}>← prev</a>}
        {todo?.next != null && <a href={`/todos/${todo.next}?neighbors=true`}>next →</a>}
      </p>

      <p className="muted note">
        The <code>id</code> arrives as a <code>params</code> prop — live from
        the router on soft navigation, streamed by the server on a hard load.
        The todo itself was seeded by <code>prefetch.rs</code> via the typed
        companion for the fallible <code>GET /api/todos/&#123;id&#125;</code>.
      </p>
      <p>
        <a href="/">← back to the list</a>
      </p>
    </section>
  );
}
