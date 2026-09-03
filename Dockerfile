ARG RUST_VERSION=1.92
ARG PLUGIN_NAME

FROM rust:${RUST_VERSION}-bookworm AS builder

ARG PLUGIN_NAME

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    clang \
    libsqlite3-dev \
    libssl-dev \
    pkg-config \
    sqlite3 \
    protobuf-compiler \
    libprotobuf-dev \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /src

COPY . .

RUN cargo build \
    --release \
    --locked \
    -p "${PLUGIN_NAME}" \
    --bin "${PLUGIN_NAME}"

FROM debian:bookworm-slim AS installer

ARG PLUGIN_NAME
ENV PLUGIN_NAME=${PLUGIN_NAME}

COPY --from=builder /src/target/release/${PLUGIN_NAME} /opt/${PLUGIN_NAME}

ENV TARGET_UID=0 \
    TARGET_GID=0 \
    TARGET_MODE=0755 \
    TARGET_PATH=/plugins/${PLUGIN_NAME}

CMD ["sh", "-c", "install -o \"$TARGET_UID\" -g \"$TARGET_GID\" -m \"$TARGET_MODE\" \"/opt/$PLUGIN_NAME\" \"$TARGET_PATH\""]