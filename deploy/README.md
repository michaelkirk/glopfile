# Deployment

The relay runs as a single Docker container bound to a **loopback port**. The
reverse proxy that terminates TLS and faces the internet lives outside this
repo; nothing here publishes on a public interface.

## hemlock

hemlock is deployed by ansible, not by the script below: the `glopfile` role in
`hemlock-ansible` owns the compose file, the unit and the caddy vhost, and
serves the relay at `relay.s.endoftheworl.de`. The web client is the static site
at `s.endoftheworl.de`. Re-running that playbook deploys a new image.

What follows is the generic path, for a host ansible does not manage.

## Install or update a host

On the host, as root:

```sh
deploy/bin/deploy --image ghcr.io/<owner>/glopfile:latest --port 8080
```

That pulls the image, installs `/opt/glopfile/docker-compose.yml` and
`/etc/systemd/system/glopfile-relay.service`, seeds `/etc/glopfile/relay.env` on
first run, restarts the unit, and waits for `/api/v1/health_check` to answer
before reporting success. It is idempotent — re-run it to deploy a new image.

Afterwards:

```sh
systemctl status glopfile-relay
journalctl -u glopfile-relay -f
```

Configuration is `/etc/glopfile/relay.env`; see
[`production/relay.env.example`](production/relay.env.example). The deploy script
preserves values already set there.

## Reverse proxy requirements

The proxy is out of band, but the relay will not work behind a default
configuration. Whatever fronts it must:

* **Disable request and response buffering.** The relay streams an upload
  straight through to the downloader. With buffering on — the default in nginx —
  the proxy holds the entire upload before contacting the relay, and the
  downloader receives nothing until the upload has finished. Measured over a
  512K transfer: 0 bytes delivered mid-flight with buffering on, 262K with it
  off. Transfers still complete, so this fails silently.
* **Allow long-lived requests.** An uploader holds its request open until a
  downloader appears, which is unbounded.
* **Proxy websocket upgrades**, for `/api/v1/{upload,download}/{id}/ws`.

For nginx:

```nginx
location / {
  proxy_pass http://127.0.0.1:8080;

  proxy_request_buffering off;
  proxy_buffering off;

  proxy_read_timeout 10d;
  proxy_send_timeout 10d;

  proxy_http_version 1.1;
  proxy_set_header Upgrade $http_upgrade;
  proxy_set_header Connection $connection_upgrade;
  proxy_set_header Host $host;
}
```

with `map $http_upgrade $connection_upgrade { default Upgrade; '' close; }` in
the `http` block.

## Why only one relay

The relay holds its sessions in memory, in the process, so a transfer only works
if the uploader and the downloader reach the *same* process. Running two behind
a load balancer silently breaks transfers.

The Erlang implementation this replaced could run several nodes, because its
session table was a clustered mnesia table — that is what the `erlang.cookie`,
`hosts.erlang` and `session_table_nodes` files were for. The Rust rewrite
dropped the cluster deliberately.

Scaling out again needs either the node's identity embedded in the generated
session id, so the proxy can route every `/{id}` path to the node that owns it,
or a shared session table. `POST /api/v1/files` carries no id, so plain sticky
routing is not enough on its own.

## Local

See [`local/README.md`](local/README.md).
