export class HttpError<T = unknown> extends Error {
  constructor(
    public readonly status: number,
    public readonly data: T,
    public readonly headers: Headers,
  ) {
    super(`HTTP ${status}`);
    this.name = "HttpError";
  }
}
