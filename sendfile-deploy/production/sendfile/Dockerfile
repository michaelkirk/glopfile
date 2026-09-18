FROM ghcr.io/privacyresearchgroup/sendfile:latest

USER root

RUN for retry in {{0..10}}; do apt-get update && break; done \
    && apt-get install --no-install-recommends -y -o APT::Acquire::Retries=10 \
           rsyslog \
           rsyslog-gnutls \
    && rm -rf /var/lib/apt/lists/*

USER erlang

RUN mkdir /home/erlang/rsyslog-spool

COPY --chown=erlang:erlang config/sys.config /home/erlang/config/
COPY --chown=erlang:erlang erlang.cookie /home/erlang/.erlang.cookie
COPY --chown=erlang:erlang rsyslog.conf /home/erlang/rsyslog.conf
COPY --chown=erlang:erlang bin/start.sh /home/erlang/bin/

RUN chmod 400 /home/erlang/.erlang.cookie

COPY --chown=erlang:erlang hosts.erlang /home/erlang/.hosts.erlang
RUN chmod 400 /home/erlang/.hosts.erlang

ENTRYPOINT [ "/home/erlang/bin/start.sh", "/usr/local/bin/erl", "-config", "config/sys.config", "-s", "sendfile_app" ]

CMD [ "-noinput", "-name", "sendfile" ]
