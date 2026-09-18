# Local relay

```sh
docker compose up --build
```

Serves <http://127.0.0.1:8080>, built from `glopfile-relay/`. Set `GLOPFILE_PORT` if
8080 is taken.

Point a client at it with `--api-endpoint http://127.0.0.1:8080`, or for the web
client set `REACT_APP_GLOPFILE_API_ENDPOINT` in
`www-client/glopfile-ui/.env.local`.

There is no nginx here, matching production, where the reverse proxy lives
outside this repo. If you are debugging proxy behaviour rather than the relay,
read the reverse proxy requirements in [../README.md](../README.md) — the
defaults of most proxies are wrong for this service.
