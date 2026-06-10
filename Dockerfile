# Serverful deployment of the nextrs site: one static-ish binary + public/.
# See site/content/docs/deploy-docker.md for the guide this backs.

FROM rust:1-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --release -p site

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/site /app/site
COPY site/public /app/public
ENV NEXTRS_PUBLIC_DIR=/app/public
EXPOSE 3000
CMD ["/app/site"]
