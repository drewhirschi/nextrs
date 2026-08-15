// @ts-check
// Compile-time package-resolution coverage for ordinary JavaScript.
import { getTodos } from "@react-todos/client";
import { getGetTodosQueryOptions } from "@react-todos/client/react-query";

void getTodos({ status: "open" });
void getGetTodosQueryOptions({ status: "open" });
// @ts-expect-error generated query parameters stay typed in JavaScript consumers
void getTodos({ status: 123 });
// @ts-expect-error the React Query entry point preserves those parameter types
void getGetTodosQueryOptions({ status: 123 });
