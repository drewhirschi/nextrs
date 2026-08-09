import { getApiTodosById, updateTodo, type TodoDetail } from "@react-todos/client";
import { getGetApiTodosByIdQueryOptions, useGetApiTodosById, useUpdateTodo } from "@react-todos/client/react-query";

type Equal<A, B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;
type Expect<T extends true> = T;
type IsAny<T> = 0 extends (1 & T) ? true : false;

const response = getApiTodosById(1, { neighbors: true });
type Response = Awaited<typeof response>;
type _ResponseData = Expect<Equal<Extract<Response, { status: 200 }>["data"], TodoDetail>>;
type _ErrorData = Expect<Equal<Extract<Response, { status: 404 }>["data"], void>>;
type _ResponseIsNotAny = Expect<Equal<IsAny<Response>, false>>;

getApiTodosById(1, { neighbors: false });
// @ts-expect-error path parameters are numbers
getApiTodosById("1");
// @ts-expect-error query parameters are typed
getApiTodosById(1, { neighbors: "yes" });

updateTodo(1, { done: true });
// @ts-expect-error request bodies require done: boolean
updateTodo(1, { done: "yes" });

const options = getGetApiTodosByIdQueryOptions(1, { neighbors: true });
void options;

export default function GeneratedClientTypeFixture() {
  const query = useGetApiTodosById(1, { neighbors: true });
  type QueryData = NonNullable<typeof query.data>;
  type _QueryData = Expect<Equal<QueryData, Response>>;
  type _QueryDataIsNotAny = Expect<Equal<IsAny<QueryData>, false>>;
  type _QueryErrorIsNotAny = Expect<Equal<IsAny<typeof query.error>, false>>;
  const mutation = useUpdateTodo({ mutation: { onSuccess(data, variables) {
    type _MutationData = Expect<Equal<typeof data, Awaited<ReturnType<typeof updateTodo>>>>;
    type _MutationVariables = Expect<Equal<typeof variables, { id: number; data: { done: boolean } }>>;
    type _VariablesAreNotAny = Expect<Equal<IsAny<typeof variables>, false>>;
    void [data, variables];
  } } });
  type _MutationErrorIsNotAny = Expect<Equal<IsAny<typeof mutation.error>, false>>;

  mutation.mutate({ id: 1, data: { done: true } });
  // @ts-expect-error mutation variables include a numeric path parameter
  mutation.mutate({ id: "1", data: { done: true } });
  // @ts-expect-error mutation variables include the typed request body
  mutation.mutate({ id: 1, data: { done: "yes" } });
  return null;
}
