// @ts-check
// Compile-time fixture: ordinary JavaScript resolves the linked client package.
import { getTodos } from "@react-todos/client";

void getTodos({ status: "open" });
// @ts-expect-error generated query parameters stay typed in JavaScript consumers
void getTodos({ status: 123 });
