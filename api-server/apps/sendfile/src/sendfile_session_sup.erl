-module(sendfile_session_sup).
-behaviour(supervisor).

-include_lib("kernel/include/logger.hrl").

%% API
-export([child_spec/0, start_link/0, start_child/1]).

%% supervisor callbacks
-export([init/1]).

-define(SERVER, ?MODULE).
-define(CHILD, sendfile_session).

%%
%% API
%%

-spec child_spec() -> supervisor:child_spec().
child_spec() ->
    #{id => ?MODULE,
      start => {?MODULE, start_link, []},
      type => supervisor}.

-spec start_link() -> {ok, pid()} | ignore | {error, any()}.
start_link() ->
    supervisor:start_link({local, ?SERVER}, ?MODULE, {}).

-spec start_child([_]) -> {ok, undefined | pid()} | {ok, undefined | pid(), _} | {error, _}.
start_child(Args) ->
    supervisor:start_child(?SERVER, Args).

%%
%% supervisor callbacks
%%

init({}=_Args) ->
    ?LOG_INFO(?MODULE_STRING " starting on ~p", [node()]),
    SupervisorFlags =
        #{strategy => simple_one_for_one,
          intensity => 5,
          period => 1},
    {ok, {SupervisorFlags, [?CHILD:simple_child_spec()]}}.
