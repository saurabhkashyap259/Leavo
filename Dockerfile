# syntax=docker/dockerfile:1
# Leavo: one image with the web app embedded in the Rust binary (T214, research R1).
# Build:  docker build -t leavo .
# Run:    see docker-compose.yml and contracts/configuration.md

# --- Web app (Node LTS) -----------------------------------------------------------------------
FROM node:lts-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY web/ ./
COPY locales/ /src/locales/
RUN npm run build

# --- Rust (latest stable) with cached dependencies -------------------------------------------
FROM rust:slim-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p leavo-api --bins
COPY . .
COPY --from=web /src/web/dist web/dist
RUN cargo build --release -p leavo-api --bins \
 && mkdir -p /out/data/uploads \
 && cp target/release/leavo target/release/leavo-demo-seed /out/

# --- Runtime: distroless, non-root -----------------------------------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /out/leavo /out/leavo-demo-seed /usr/local/bin/
COPY --from=builder --chown=65532:65532 /out/data /data
ENV LEAVO_BIND=0.0.0.0:8080 \
    STORAGE=local \
    STORAGE_LOCAL_PATH=/data/uploads
VOLUME ["/data/uploads"]
EXPOSE 8080
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/leavo"]
CMD ["serve"]
