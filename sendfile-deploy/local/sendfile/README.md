# Start a local cluster with docker

1. Compile the erlang app

```sh
cd sendfile # (the api server)

# do we need to clean the build like this?
rm -fr _build

scripts/container-build rebar3 compile

# Incorporate the compiled app into a runtime container
docker build . -t sendfile
```

2. build cluster-deployable nodes based on the docker container image from step 1

```sh
cd sendfile-deploy/local/sendfile

# NOTE: Requires Docker compose 2.0
#
# If you're on Mac/Windows and installed Docker Desktop, a recent enough
# version of docker-compose should already be installed for you.  On linux,
# however, you'll probably have to follow:
# https://docs.docker.com/compose/cli-command/#install-on-linux
docker compose build
docker compose up
```

## Testing network resiliency

The docker compose command adds the containers to an internal network. You can
simulate an outage by temporarily removing a node from the network.

```sh
docker network disconnect glopfile-local relay-1
```

Then re-attach it when you're done
```sh
docker network connect glopfile-local relay-1
```

