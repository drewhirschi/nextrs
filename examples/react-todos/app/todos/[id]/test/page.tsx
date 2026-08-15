import { getApiTodosById } from "@react-todos/client";
import {
  useGetApiTodosById,
  useUpdateTodo,
} from "@react-todos/client/react-query";

const todoId = 7;

export default function GeneratedClientExample() {
  const query = useGetApiTodosById(todoId, { neighbors: true });
  const updateTodo = useUpdateTodo();
  const todo = query.data?.status === 200 ? query.data.data : undefined;

  async function fetchDirectly() {
    const response = await getApiTodosById(todoId, { neighbors: true });
    if (response.status === 200) {
      console.info("Fetched with the framework-agnostic client", response.data);
    }
  }

  return (
    <section>
      <h1>Generated client example</h1>
      {todo ? (
        <p>
          Todo #{todo.id}: <strong>{todo.title}</strong>
        </p>
      ) : (
        <p>{query.isPending ? "Loading…" : "Todo not found."}</p>
      )}

      <div className="row">
        <button onClick={() => void fetchDirectly()}>Fetch directly</button>
        <button
          disabled={!todo || updateTodo.isPending}
          onClick={() =>
            todo &&
            updateTodo.mutate({
              id: todo.id,
              data: { done: !todo.done },
            })
          }
        >
          {todo?.done ? "Reopen" : "Complete"}
        </button>
      </div>

      <p className="muted">
        The path, query, response, and mutation-variable types above are all
        inferred from the Rust endpoints.
      </p>
    </section>
  );
}
