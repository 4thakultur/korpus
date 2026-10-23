# syntax=docker/dockerfile:1
# Build: docker build -t korpus .
FROM rust:1.96-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY templates ./templates
COPY static ./static
RUN cargo build --release --locked && strip target/release/korpus

FROM debian:bookworm-slim
# ca-certificates: TLS for optional food database providers.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data korpus \
    && mkdir -p /data && chown korpus:korpus /data
COPY --from=build /src/target/release/korpus /usr/local/bin/korpus
USER korpus
ENV KORPUS_BIND=0.0.0.0:9874 \
    KORPUS_DATABASE_URL=sqlite:///data/korpus.db
VOLUME ["/data"]
EXPOSE 9874
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s CMD ["korpus", "healthcheck"]
CMD ["korpus"]
