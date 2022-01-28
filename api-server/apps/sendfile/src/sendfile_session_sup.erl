-module(sendfile_session_sup).
-behaviour(supervisor).
-behaviour(sendfile_child).

-include_lib("kernel/include/logger.hrl").

%% API
-export([start_link/0, start_child/1]).
-ignore_xref([start_link/0]).                   % xref doesn't understand the MFA returned by child_spec/0

%% sendfile_sup_child callbacks
-export([child_spec/0]).

%% supervisor callbacks
-export([init/1]).

-define(SERVER, ?MODULE).
-define(CHILD, sendfile_session).

%%
%% API
%%

-spec start_link() -> {ok, pid()} | ignore | {error, any()}.
start_link() ->
    supervisor:start_link({local, ?SERVER}, ?MODULE, {}).

-spec start_child([_]) -> {ok, undefined | pid()} | {ok, undefined | pid(), _} | {error, _}.
start_child(Args) ->
    supervisor:start_child(?SERVER, Args).

%%
%% sendfile_child callbacks
%%

-spec child_spec() -> supervisor:child_spec().
child_spec() ->
    #{id => ?MODULE,
      start => {?MODULE, start_link, []},
      type => supervisor}.

%%
%% supervisor callbacks
%%

init({}=_Args) ->
    ?LOG_INFO(?MODULE_STRING " starting on ~p", [node()]),
    SupervisorFlags =
        #{strategy => simple_one_for_one,
          intensity => 5,
          period => 1},
    {ok, {SupervisorFlags, [?CHILD:child_spec()]}}.
