// Todo detail — a dynamic route ([id]). The `params` prop comes from the
// framework: TanStack Router's live match under soft navigation, the server's
// __nx_params__ tag on a hard load. First paint is seeded by the sibling
// prefetch.rs, so there's no fetch on load here either.
import { useGetApiTodosById, useParams } from "@react-todos/client";

// A "deep" component that needs the route param but doesn't get the `params`
// prop threaded down — useParams() reads the app-shell router's live match.
function Permalink() {
  const { id } = useParams<{ id: string }>();
  return <code className="muted">/todos/{id}</code>;
}

export default function TodoDetail({ params }: { params: { id: string } }) {
  const id = Number(params.id);
  const { data, isFetching } = useGetApiTodosById(id);
  const todo = data?.data;

  return (
    <section>
      <h1>
        Todo #{params.id} <Permalink />
      </h1>
      {todo ? (
        <p>
          <strong>{todo.title}</strong>{" "}
          <span className="muted">{todo.done ? "done" : "open"}</span>
        </p>
      ) : (
        <p className="muted">{isFetching ? "Loading…" : "No such todo."}</p>
      )}
      <p className="muted note">
        The <code>id</code> arrives as a <code>params</code> prop — live from
        the router on soft navigation, streamed by the server on a hard load.
        The todo itself was seeded by <code>prefetch.rs</code> via the typed
        Path-param companion for <code>GET /api/todos/&#123;id&#125;</code>.
      </p>
      <p>
        <a href="/">← back to the list</a>
      </p>
    </section>
  );
}
