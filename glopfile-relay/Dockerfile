FROM rust:1-slim AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock build.rs ./
COPY proto proto
COPY src src
RUN cargo build --release --locked

FROM debian:stable-slim
RUN useradd --create-home --user-group glopfile
COPY --from=build /build/target/release/glopfile-relay /usr/local/bin/glopfile-relay
USER glopfile
ENV GLOPFILE_LISTEN=0.0.0.0:8080
EXPOSE 8080
CMD ["glopfile-relay"]
