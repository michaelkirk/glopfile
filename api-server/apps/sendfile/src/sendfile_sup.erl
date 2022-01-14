-module(sendfile_sup).
-behaviour(supervisor).

-include_lib("kernel/include/logger.hrl").

%% API
-export([start_link/0]).

%% supervisor callbacks
-export([init/1]).

-define(SERVER, ?MODULE).

%%
%% API
%%

-spec start_link() -> {ok, pid()} | ignore | {error, any()}.
start_link() ->
    supervisor:start_link({local, ?SERVER}, ?MODULE, {}).

%%
%% supervisor callbacks
%%

init({}=_Args) ->
    ?LOG_INFO(?MODULE_STRING " starting on ~p", [node()]),
    Mods = [sendfile_session_table, sendfile_session_sup],
    ChildSpecs = [Mod:child_spec() || Mod <- Mods],
    SupervisorFlags =
        #{strategy => one_for_one,
          intensity => 5,
          period => 1},
    {ok, {SupervisorFlags, ChildSpecs}}.
