# Serverful deployment of the nextrs site: one static-ish binary + public/.
# See content/docs/deploy-docker.md for the guide this backs.
#
# Build context is the REPO ROOT, not site/ — `site` is a workspace member and
# its `../crates/nextrs` path deps live outside this dir. Build from the root:
#   docker build -f site/Dockerfile -t nextrs-site .

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
