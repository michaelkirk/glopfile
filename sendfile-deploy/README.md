# sendfile deployment resources

Deployment resources for the [Sendfile relay](../api-server), divided by
environment.

Both environments are the same shape: nginx in front of a single relay
container. The relay is configured entirely through the environment
(`SENDFILE_LISTEN`, `RUST_LOG`), so there is no deployment-specific image to
build — production runs the same image CI builds from `api-server/Dockerfile`.

## Why only one relay

The relay holds its sessions in memory, in the process. A transfer only works
if the uploader and the downloader reach the *same* process, so running two
relays behind a load balancer silently breaks transfers: the two halves land on
different nodes and never find each other.

The Erlang implementation this replaced could run several nodes, because its
session table was a clustered mnesia table — that is what the `erlang.cookie`,
`hosts.erlang` and `session_table_nodes` files were for. The Rust rewrite
dropped the cluster deliberately, and those files are gone with it.

Scaling out again needs one of two things, neither of which exists today:

* the node's identity embedded in the generated session id, so nginx can route
  every `/{id}` path to the node that owns it, or
* a shared session table the relays agree on.

`POST /api/v1/files` carries no id, so plain sticky routing or a consistent
hash over the URL is not enough on its own.

## Local

```sh
cd local/sendfile
docker compose up --build
```

This builds the relay straight from `api-server/` and serves it on
<http://localhost:8080>. It is worth going through nginx locally rather than
running the binary directly, because the websocket upgrade and the streaming
proxy settings are where deployment problems actually show up.

## Production

```sh
cd production/sendfile
SENDFILE_IMAGE=ghcr.io/<owner>/sendfile:latest docker compose up -d
```

Container stdout goes to Papertrail through Docker's syslog logging driver,
which replaced the in-container rsyslog the Erlang image ran.
