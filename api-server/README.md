# sendfile

Sendfile relay

## Development

[Internal API Documentation](https://jessa0.github.io/sendfile/)

## Building with docker

Building with the [`container-build`](https://github.com/container-build/container-build) script requires python3 and
docker, and builds the project within the [`erlang:latest`](https://hub.docker.com/_/erlang/) Docker image.

```
$ scripts/container-build rebar3 compile
```

## Building without docker

Building without docker requires Erlang OTP 24 (or maybe above).

```
$ rebar3 compile
```

## Running with docker

Firstly, ensure that the Erlang OTP version used to build the project closely matches that of the
[`erlang:latest`](https://hub.docker.com/_/erlang/) image, which is the image the included
[`Dockerfile`](Dockerfile) uses.

To run the server in the background:

```
$ container=$(docker run -p 8080:8080 --detach $(docker build -q .))
```

To start a remote Erlang shell on the node:

```
$ docker exec -it $container erl -remsh sendfile@localhost -hidden
```

To re-compile and hot-load changes to a module on the running node (replacing `Mod` with the module):

```
$ docker cp apps $container:/home/erlang/
$ docker exec -u root:root $container chown -R erlang:erlang /home/erlang/apps
$ docker exec -it $container erl -remsh sendfile@localhost -hidden
(sendfile@localhost)1> {ok, _} = c(Mod).
(sendfile@localhost)2> {module, _} = l(Mod).
```

Only hot-load code when you know the new code will be compatible with any running state in the system.

## Running without docker

```
$ rebar3 shell
```
