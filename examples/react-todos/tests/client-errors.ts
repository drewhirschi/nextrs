import { HttpError, useGetApiTodosById } from '@react-todos/client/react-query';

export function generatedErrorTypes() {
  const query = useGetApiTodosById(1);
  if (query.error) {
    const error: HttpError<{ error: string }> = query.error;
    const message: string = error.data.error;
    // @ts-expect-error Undocumented fields must not degrade to any.
    error.data.missing;
    return message;
  }
  if (query.data) {
    const status: 200 = query.data.status;
    return status;
  }
}
