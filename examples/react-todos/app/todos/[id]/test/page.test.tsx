// Compile-time contract tests for the generated client used by page.tsx.
import {
  postApiTodos,
  getApiTodosById,
  getApiTodos,
  patchApiTodosById,
} from "@react-todos/client";
import type { ApiError } from "@react-todos/client";
import {
  getGetApiTodosByIdQueryOptions,
  useGetApiTodosById,
  usePatchApiTodosById,
} from "@react-todos/client/react-query";

type Assert<T extends true> = T;
type Equal<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2
    ? (<T>() => T extends B ? 1 : 2) extends <T>() => T extends A ? 1 : 2
      ? true
      : false
    : false;
type IsAny<T> = 0 extends 1 & T ? true : false;
type NotAny<T> = IsAny<T> extends false ? true : false;
type QueryFunctionData<T> = T extends (...args: never[]) => infer Result
  ? Awaited<Result>
  : never;

type DetailResponse = Awaited<ReturnType<typeof getApiTodosById>>;
type DetailSuccess = Extract<DetailResponse, { status: 200 }>;
// Non-200s carry the framework's typed error body (`ApiError`), inferred from
// the handler's `Result<Json<TodoDetail>, ApiError>` — no responses(...) block.
type DetailError = Exclude<DetailResponse, { status: 200 }>;
type DetailPath = Parameters<typeof getApiTodosById>[0];
type DetailQuery = NonNullable<Parameters<typeof getApiTodosById>[1]>;
type AddBody = Parameters<typeof postApiTodos>[0];
type UpdateBody = Parameters<typeof patchApiTodosById>[1];

type _ResponseInference = Assert<Equal<DetailSuccess["data"]["id"], number>>;
type _ErrorInference = Assert<Equal<DetailError["data"], ApiError>>;
type _PathInference = Assert<Equal<DetailPath, number>>;
type _QueryInference = Assert<
  Equal<DetailQuery["neighbors"], boolean | null | undefined>
>;
type _AddBodyInference = Assert<Equal<AddBody["title"], string>>;
type _UpdateBodyInference = Assert<Equal<UpdateBody["done"], boolean>>;

const detailOptions = getGetApiTodosByIdQueryOptions(7, {
  neighbors: true,
});
type DetailQueryData = Awaited<
  QueryFunctionData<NonNullable<typeof detailOptions.queryFn>>
>;
type UpdateVariables = Parameters<
  ReturnType<typeof usePatchApiTodosById>["mutate"]
>[0];

type _QueryOptionsInference = Assert<
  Equal<Extract<DetailQueryData, { status: 200 }>["data"]["title"], string>
>;
type _MutationPathInference = Assert<Equal<UpdateVariables["id"], number>>;
type _MutationBodyInference = Assert<
  Equal<UpdateVariables["data"]["done"], boolean>
>;

type _FetchIsNotAny = Assert<NotAny<DetailResponse>>;
type _QueryIsNotAny = Assert<NotAny<DetailQueryData>>;
type _MutationIsNotAny = Assert<NotAny<UpdateVariables>>;

function inferredQueryHookData() {
  const query = useGetApiTodosById(7, { neighbors: true });
  type HookQueryData = Exclude<typeof query.data, undefined>;
  type _QueryHookInference = Assert<
    Equal<Extract<HookQueryData, { status: 200 }>["data"]["done"], boolean>
  >;
  type _HookIsNotAny = Assert<NotAny<HookQueryData>>;
  return query;
}

function rejectedInputsStayRejected(
  mutation: ReturnType<typeof usePatchApiTodosById>,
) {
  // @ts-expect-error path parameters are generated as numbers
  void getApiTodosById("7");
  // @ts-expect-error query parameters are generated as booleans
  void getApiTodosById(7, { neighbors: "yes" });
  // @ts-expect-error list query parameters are generated as strings
  void getApiTodos({ status: 1 });
  // @ts-expect-error request bodies require a title
  void postApiTodos({});
  // @ts-expect-error request-body fields keep their generated type
  void patchApiTodosById(7, { done: "yes" });
  // @ts-expect-error React Query options keep the same typed path parameter
  void getGetApiTodosByIdQueryOptions("7");
  // @ts-expect-error mutation variables infer both path and body types
  mutation.mutate({ id: "7", data: { done: true } });
  // @ts-expect-error mutation request bodies stay typed at the hook boundary
  mutation.mutate({ id: 7, data: { done: "yes" } });
}

void rejectedInputsStayRejected;
void inferredQueryHookData;
