FROM sendfile:latest

# Clobber default config
COPY --chown=erlang:erlang config/sys.config /home/erlang/config/

COPY --chown=erlang:erlang erlang.cookie /home/erlang/.erlang.cookie
RUN chmod 400 /home/erlang/.erlang.cookie

COPY --chown=erlang:erlang hosts.erlang /home/erlang/.hosts.erlang
RUN chmod 400 /home/erlang/.hosts.erlang

EXPOSE 8080/tcp

ENTRYPOINT ["/usr/local/bin/erl", "-config", "config/sys.config", "-s", "sendfile_app" ]

CMD [ "-noinput", "-name", "sendfile" ]
