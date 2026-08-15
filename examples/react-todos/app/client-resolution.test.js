// @ts-check
// Compile-time package-resolution coverage for ordinary JavaScript.
import { getApiTodos } from "@react-todos/client";
import { getGetApiTodosQueryOptions } from "@react-todos/client/react-query";

void getApiTodos({ status: "open" });
void getGetApiTodosQueryOptions({ status: "open" });
// @ts-expect-error generated query parameters stay typed in JavaScript consumers
void getApiTodos({ status: 123 });
// @ts-expect-error the React Query entry point preserves those parameter types
void getGetApiTodosQueryOptions({ status: 123 });
