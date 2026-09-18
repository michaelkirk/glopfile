FROM rust:1-slim AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock build.rs ./
COPY proto proto
COPY src src
RUN cargo build --release --locked

FROM debian:stable-slim
RUN useradd --create-home --user-group sendfile
COPY --from=build /build/target/release/sendfile /usr/local/bin/sendfile
USER sendfile
ENV SENDFILE_LISTEN=0.0.0.0:8080
EXPOSE 8080
CMD ["sendfile"]
