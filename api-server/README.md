# sendfile

Sendfile relay.

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

The server listens on `0.0.0.0:8080` by default; set `SENDFILE_LISTEN` to change the
address and `RUST_LOG` to change the log level.

## Running with docker

```
$ container=$(docker run -p 8080:8080 --detach $(docker build -q .))
```

## Tests

```
$ cargo test
```

## API

All endpoints live under `/api/v1` and answer `OPTIONS` with permissive CORS headers.

| Endpoint | Description |
| --- | --- |
| `POST /files` | Provisions a session from the `encrypted_metadata` form field; returns `upload_url` and `download_id`. |
| `GET /download/{id}` | Returns the session's `meta` and its `encrypted_content_url`. |
| `GET /download/{id}/content` | Streams the content. `Range: bytes=<position>-` resumes; `Range: bytes=-0` marks the download finished. |
| `GET /content/{id}` | Alias for `/download/{id}/content`. |
| `POST /upload/{id}` | Sends content. `Content-Range: bytes <position>-<last>/<size>` resumes. A `409` carries the `position` the downloader expects. |
| `GET /download/{id}/ws`, `GET /upload/{id}/ws` | Relays websocket frames between the two peers, and sends the uploader progress acks. |
