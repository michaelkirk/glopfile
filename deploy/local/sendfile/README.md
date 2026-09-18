# Local relay

```sh
docker compose up --build
```

Serves <http://localhost:8080>, building the relay from `api-server/`.

Point a client at it with `--api-endpoint http://localhost:8080`, or for the
web client set `REACT_APP_SENDFILE_API_ENDPOINT=http://localhost:8080` in
`www-client/sendfile-ui/.env.local`.

To watch the relay decide things, raise the log level in `docker-compose.yml`:

```yaml
RUST_LOG: sendfile=debug
```
