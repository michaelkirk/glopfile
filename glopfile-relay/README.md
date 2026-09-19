# glopfile

Glopfile relay.

A session pairs one uploader with one downloader and streams content between them;
nothing is stored on the server. Sessions live in memory and end when the transfer
finishes.

## Building

```
$ cargo build --release
```

## Running

```
$ cargo run
```

The server listens on `0.0.0.0:8080` by default; pass `--address` and `--port`
to change that, and set `RUST_LOG` to change the log level. `--help` lists
everything.

## Running with docker

The relay is a workspace member, so the image builds from the repository root:

```
$ docker build -f glopfile-relay/Dockerfile -t glopfile-relay ..
$ docker run -p 8080:8080 --detach glopfile-relay
```

## Tests

```
$ cargo test
```

## Behind a reverse proxy

The relay streams an upload straight through to the downloader, which most
proxies are configured wrong for by default. Whatever fronts it must:

* **Not buffer requests or responses.** With buffering on — nginx's default —
  the proxy holds the whole upload before contacting the relay, and the
  downloader receives nothing until the upload has finished. Measured over a
  512K transfer: 0 bytes delivered mid-flight with buffering on, 262K with it
  off. The transfer still completes, so this fails silently.
* **Allow long-lived requests.** An uploader holds its request open until a
  downloader appears, which is unbounded.
* **Proxy websocket upgrades**, for `/api/v1/{upload,download}/{id}/ws`.

caddy needs `flush_interval -1` for the first of those and handles the rest on
its own.

## Why only one

Sessions live in this process's memory, so a transfer only works if the
uploader and the downloader reach the *same* one. Running two behind a load
balancer silently breaks transfers.

The Erlang implementation this replaced could run several nodes, because its
session table was a clustered mnesia table. The rust rewrite dropped the
cluster deliberately.

Scaling out again needs either the node's identity embedded in the generated
session id, so the proxy can route every `/{id}` path to the node that owns it,
or a shared session table. `POST /api/v1/files` carries no id, so plain sticky
routing is not enough on its own.

## API

All endpoints live under `/api/v1` and answer `OPTIONS` with permissive CORS headers.

| Endpoint | Description |
| --- | --- |
| `POST /files` | Provisions a session from the `encrypted_metadata` form field; returns `upload_url` and `download_id`. |
| `GET /download/{id}` | Returns the session's `meta` and its `encrypted_content_url`. |
| `GET /download/{id}/content` | Streams the content. `Range: bytes=<position>-` resumes; `Range: bytes=-0` marks the download finished. |
| `GET /content/{id}` | Alias for `/download/{id}/content`. |
| `POST /upload/{id}/start` | Asks, via the `position` form field, whether an upload may start there. `200 {"status":"ok"}` or `409 {"position":N}`. |
| `POST /upload/{id}` | Sends content. `Content-Range: bytes <position>-<last>/<size>` resumes; `bytes */<size>` waits at EOF for the downloader. Answers `{"status":"ok"}` or `{"status":"error","reason":...}`, or `409 {"position":N}`. |
| `GET /health_check` | Liveness check; returns `{"status":"ok"}`. |
| `GET /download/{id}/ws`, `GET /upload/{id}/ws` | Relays websocket frames between the two peers, and sends the uploader progress acks. |
