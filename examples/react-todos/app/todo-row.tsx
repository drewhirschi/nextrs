import type { Todo } from "@react-todos/client";

export function TodoRow({
  todo,
  onToggle,
}: {
  todo: Todo;
  onToggle: (todo: Todo) => void;
}) {
  return (
    <li className={todo.done ? "done" : ""}>
      <button
        className={`check${todo.done ? " checked" : ""}`}
        aria-label={todo.done ? `Reopen ${todo.title}` : `Complete ${todo.title}`}
        onClick={() => onToggle(todo)}
      >
        {todo.done ? "✓" : ""}
      </button>
      <a className="title" href={`/todos/${todo.id}`}>
        {todo.title}
      </a>
      <span className={`badge ${todo.done ? "badge-done" : "badge-open"}`}>
        {todo.done ? "Done" : "Open"}
      </span>
    </li>
  );
}
