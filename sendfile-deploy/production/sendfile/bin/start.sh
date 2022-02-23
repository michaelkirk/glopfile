#!/bin/bash

set -e

start-stop-daemon \
    --start --pidfile /home/erlang/rsyslog.pid --startas /usr/sbin/rsyslogd -- \
    -f /home/erlang/rsyslog.conf -i /home/erlang/rsyslog.pid

exec "$@" | logger --stderr --socket /home/erlang/rsyslog-sock
