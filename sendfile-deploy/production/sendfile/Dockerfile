FROM ghcr.io/glopfile/sendfile:latest

COPY --chown=erlang:erlang config/sys.config /home/erlang/config/
COPY --chown=erlang:erlang erlang.cookie /home/erlang/.erlang.cookie

RUN chmod 400 /home/erlang/.erlang.cookie

CMD [ "-noinput", "-name", "sendfile" ]
