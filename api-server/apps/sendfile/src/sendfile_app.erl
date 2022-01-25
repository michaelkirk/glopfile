-module(sendfile_app).
-behaviour(application).

%% API
-export([start/0, stop/0, restart/0, application/0]).
-export([listen_ip/0, listen_port/0, session_table_nodes/0]).

%% application callbacks
-export([start/2, stop/1]).

-define(APP, sendfile).

%%
%% API
%%

-spec start() -> {ok, Started :: [App :: atom()]} | {error, any()}.
start() ->
    application:ensure_all_started(?APP).

-spec stop() -> ok | {error, any()}.
stop() ->
    application:stop(?APP).

-spec restart() -> {ok, Started :: [App :: atom()]} | {error, any()}.
restart() ->
    stop(),
    start().

-spec application() -> atom().
application() ->
    ?APP.

-spec listen_ip() -> {ok, any()} | undefined.
listen_ip() ->
    application:get_env(?APP, listen_ip).

-spec listen_port() -> any().
listen_port() ->
    application:get_env(?APP, port, 8080).

-spec session_table_nodes() -> {ok, any()} | undefined.
session_table_nodes() ->
    application:get_env(?APP, session_table_nodes).

%%
%% application callbacks
%%

start(_StartType, _StartArgs) ->
    sendfile_server:start(),
    sendfile_sup:start_link().

stop(_State) ->
    ok.
