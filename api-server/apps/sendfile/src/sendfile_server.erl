-module(sendfile_server).

-include_lib("kernel/include/logger.hrl").

%% API
-export([start/0, stop/0]).
-ignore_xref([start/0, stop/0]). % unused, but useful in the shell

%%
%% API
%%

-spec start() -> {ok, pid()} | {error, any()}.
start() ->
    ?LOG_INFO(?MODULE_STRING " starting on ~p", [node()]),

    ListenIpOpts = case sendfile_app:listen_ip() of
                       undefined      -> [];
                       {ok, ListenIp} -> [{listen_ip, ListenIp}]
                   end,
    Port = sendfile_app:listen_port(),
    SocketOpts = ListenIpOpts ++
        [{port, Port}],
    TransportOpts = #{connection_type => supervisor,
                      socket_opts => SocketOpts},

    ApiRoute = {"/api/[...]", sendfile_api_handler, {}},
    Routes = [ApiRoute],
    Dispatch = cowboy_router:compile([{'_', Routes}]),

    ProtocolOpts = #{env => #{dispatch => Dispatch},
                     idle_timeout => infinity,
                     inactivity_timeout => infinity},

    cowboy:start_clear(?MODULE, TransportOpts, ProtocolOpts).

-spec stop() -> ok | {error, any()}.
stop() ->
    ?LOG_INFO(?MODULE_STRING " stopping on ~p", [node()]),
    ranch:stop_listener(?MODULE).
