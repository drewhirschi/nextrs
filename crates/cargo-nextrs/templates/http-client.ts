import { HttpError } from "./http-error";

export type ErrorType<T> = HttpError<T>;

export async function httpClient<T>(url: string, options?: RequestInit): Promise<T> {
  const response = await fetch(url, options);
  const text = [204, 205, 304].includes(response.status) ? "" : await response.text();
  let data: unknown = text || undefined;
  if (text && response.headers.get("content-type")?.includes("json")) {
    try {
      data = JSON.parse(text) as unknown;
    } catch (error) {
      if (response.ok) throw error;
    }
  }
  if (!response.ok) throw new HttpError(response.status, data, response.headers);
  return { data, status: response.status, headers: response.headers } as T;
}
