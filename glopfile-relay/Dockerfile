# Built from the repository root, since the relay is a workspace member and the
# lockfile and dependency versions live there:
#
#   docker build -f glopfile-relay/Dockerfile .
FROM rust:1-slim AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY glopfile-relay glopfile-relay
COPY rust-client rust-client
RUN cargo build --release --locked -p glopfile-relay

FROM debian:stable-slim
RUN useradd --create-home --user-group glopfile
COPY --from=build /build/target/release/glopfile-relay /usr/local/bin/glopfile-relay
USER glopfile
EXPOSE 8080
ENTRYPOINT ["glopfile-relay"]
