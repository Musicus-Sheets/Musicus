# syntax=docker/dockerfile:1.7
#
# Musicus in one image: the server program, with the web client built into it
# (feature embed-web), the same single file as the plain program.
#
#   docker build -t musicus .
#   docker compose up -d                  # see compose.yaml
#
# For a Raspberry Pi (arm64) build on the Pi itself, or cross-build with
#   docker buildx build --platform linux/arm64 -t musicus .
#
# The release pipeline (.github/workflows/release.yml) builds the target
# `release` instead: the same runtime image around the program it built,
# tested and offers for download, taken from the build context `program`
# (<arch>/musicus-server per platform), so no Rust is compiled here:
#   docker buildx build --target release --build-context program=<dir> \
#     --platform linux/amd64,linux/arm64 .

ARG RUST_IMAGE=rust:1.97-bookworm
ARG RUNTIME_IMAGE=gcr.io/distroless/cc-debian12:nonroot

# --- Dioxus CLI: its own stage, so the slow install is cached --------------
FROM ${RUST_IMAGE} AS dx
ARG DX_VERSION=0.7.10
RUN cargo install --locked dioxus-cli --version ${DX_VERSION} --root /opt/dx

# --- Build the web client and the server ----------------------------------
FROM ${RUST_IMAGE} AS build
RUN rustup target add wasm32-unknown-unknown
COPY --from=dx /opt/dx/bin/dx /usr/local/bin/dx
WORKDIR /src
COPY . .
# pdf.js is pinned and checksum-verified by the script.
RUN ./scripts/fetch-pdfjs.sh
# `dx` downloads wasm-bindgen, wasm-opt and esbuild on first use. The client
# folder is emptied first, so no file of an earlier build (cache mount) is
# built into the program.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    rm -rf target/dx/musicus-web/release \
    && dx bundle --platform web --package musicus-web --release \
    && cargo build --release --locked -p musicus-server --features embed-web \
    && mkdir -p /out \
    && cp target/release/musicus-server /out/

# --- An empty data directory (no shell needed: WORKDIR creates it) ---------
FROM scratch AS datadir
WORKDIR /out/data

# --- Runtime: no shell, no package manager, not root -----------------------
FROM ${RUNTIME_IMAGE} AS runtime
# An empty data directory owned by the runtime user, so a fresh volume
# inherits that ownership.
COPY --from=datadir --chown=65532:65532 /out/data /data
ENV MUSICUS_DATA_DIR=/data \
    MUSICUS_LISTEN=[::]:8080
USER 65532:65532
VOLUME ["/data"]
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD ["/usr/local/bin/musicus-server", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/musicus-server"]
CMD ["serve"]

# --- Release image: the program built by the release pipeline -------------
FROM runtime AS release
ARG TARGETARCH
COPY --from=program --chmod=0755 ${TARGETARCH}/musicus-server /usr/local/bin/musicus-server

# --- Default: the program built above from the sources ---------------------
FROM runtime
COPY --from=build /out/musicus-server /usr/local/bin/musicus-server
