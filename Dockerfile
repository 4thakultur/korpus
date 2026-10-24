# syntax=docker/dockerfile:1
# Build: docker build -t korpus .
FROM rust:1.96-slim-bookworm AS build
ARG TARGETARCH
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
COPY templates ./templates
COPY static ./static
COPY bundled ./bundled
# Cache the crate registry and build directory between builds: a rebuild
# after a source change then only recompiles Korpus itself, which matters
# on small homelab hardware. The binary is copied out because cache mounts
# are not part of the image.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=korpus-target-${TARGETARCH},target=/src/target,sharing=locked \
    cargo build --release --locked \
 && cp target/release/korpus /usr/local/bin/korpus \
 && strip /usr/local/bin/korpus

FROM debian:bookworm-slim
# ca-certificates: TLS for optional food database providers.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 1000 --home-dir /data korpus \
    && mkdir -p /data && chown korpus:korpus /data
COPY --from=build /usr/local/bin/korpus /usr/local/bin/korpus
USER korpus
ENV KORPUS_BIND=0.0.0.0:9874 \
    KORPUS_DATABASE_URL=sqlite:///data/korpus.db
VOLUME ["/data"]
EXPOSE 9874
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s CMD ["korpus", "healthcheck"]
CMD ["korpus"]
