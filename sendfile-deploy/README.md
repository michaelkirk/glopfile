# sendfile deployment resources

This repo contains deployment resources for the [Sendfile relay](https://github.com/glopfile/sendfile/). The resources are divided by their environment, i.e. production, staging, and testing.

## Docker images

The docker images to build are based on those in the application source repo, except:

* `sys.config` is replaced with one specific for the deployment
* The image's `CMD` configures Erlang for communication with peers over dist with a real, long hostname
