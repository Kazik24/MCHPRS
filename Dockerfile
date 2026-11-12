# syntax=docker/dockerfile:1

# Keep the compiler stable so a new Rust release does not invalidate the cache.
ARG RUST_VERSION=1.98.1
FROM rust:${RUST_VERSION}-slim-bookworm AS builder
ARG RUST_VERSION
ARG TARGETARCH

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        git pkg-config libssl-dev clang lld \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
ENV CARGO_HOME=/var/cache/cargo \
    CARGO_TARGET_DIR=/app/target

# The adjacent .dockerignore includes only build inputs, including optional
# pinned mc_data assets. Runtime data/docs stay excluded.
COPY . ./

# Use the server's default 512x512-block plots.

# BuildKit stores both caches on the builder selected by the Docker context.
# Lock the mounts to prevent concurrent builds from changing cached artifacts.
# Cargo checks source mtimes. Preserve them for unchanged content, and refresh
# changed files even when COPY brings in timestamps older than cached artifacts.
# Copy the executable out in this RUN: cache mounts are absent in later layers.
RUN --mount=type=cache,id=mchprs-cargo,target=/var/cache/cargo,sharing=locked \
    --mount=type=cache,id=mchprs-target-${RUST_VERSION}-${TARGETARCH},target=/app/target,sharing=locked \
    find . -path ./target -prune -o -type f -exec sh -c ' \
        for source do \
            cached="$CARGO_TARGET_DIR/.mchprs-sources/$source"; \
            if [ -f "$cached" ] && cmp -s "$source" "$cached"; then \
                touch -r "$cached" "$source"; \
            else \
                touch "$source"; \
                mkdir -p "$(dirname "$cached")"; \
                cp -p "$source" "$cached"; \
            fi; \
        done \
    ' sh {} + \
    && cargo build --release --locked --bin mroww \
    && install -D /app/target/release/mroww /out/mroww

FROM debian:bookworm-slim AS runtime
# C23 #embed needs Clang 19; preserve the paths used by Compose overrides.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 clang-19 lld-19 \
    && ln -s /usr/bin/clang-19 /usr/bin/clang \
    && ln -s /usr/bin/ld.lld-19 /usr/bin/ld.lld \
    && printf '\001\002\003' > /tmp/llvm-embed.bin \
    && printf '%s\n' \
        'static const unsigned char data[] = {' \
        '#embed "/tmp/llvm-embed.bin"' \
        '};' \
        'int main(void) { return sizeof(data) != 3 || data[0] != 1 || data[1] != 2 || data[2] != 3; }' \
        > /tmp/llvm-embed.c \
    && clang -std=c23 -O3 -ffreestanding -fno-builtin -Werror \
        --ld-path=/usr/bin/ld.lld /tmp/llvm-embed.c -o /tmp/llvm-embed \
    && /tmp/llvm-embed \
    && clang -std=c23 -O3 -ffreestanding -fno-builtin -Werror -fPIC -shared \
        --ld-path=/usr/bin/ld.lld -Wl,-z,defs -Wl,-z,relro -Wl,-z,now \
        /tmp/llvm-embed.c -o /tmp/llvm-embed.so \
    && rm -f /tmp/llvm-embed.bin /tmp/llvm-embed.c /tmp/llvm-embed /tmp/llvm-embed.so \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /out/mroww /usr/local/bin/mroww
COPY Config.docker.toml /etc/mchprs/Config.toml
ENV MCHPRS_CONFIG=/etc/mchprs/Config.toml
ENV MCHPRS_LLVM_WORK_DIR=/data/work/llvm \
    MCHPRS_LLVM_CLANG=/usr/bin/clang \
    MCHPRS_LLVM_LINKER=/usr/bin/ld.lld
WORKDIR /data
VOLUME ["/data"]
EXPOSE 25565

CMD ["/usr/local/bin/mroww"]
