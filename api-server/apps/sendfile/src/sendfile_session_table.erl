-module(sendfile_session_table).
-behaviour(gen_server).

-include_lib("kernel/include/logger.hrl").

%% API
-export([child_spec/0, start_link/0, new_id/0, create/2, get/1, delete/1]).
-export_type([get_error/0]).

%% gen_server callbacks
-export([init/1, handle_call/3, handle_cast/2, terminate/2]).

-define(SERVER, sendfile_session_table).
-define(TABLE, sendfile_session).
-record(?TABLE, {id, pid}).

-define(ID_LENGTH, 8).

-type get_error() :: not_found.

%%
%% API
%%

-spec child_spec() -> supervisor:child_spec().
child_spec() ->
    #{id => ?MODULE,
      start => {?MODULE, start_link, []}}.

-spec start_link() -> ignore | {error, _} | {ok, pid() | {pid(), reference()}}.
start_link() ->
    gen_server:start_link({local, ?SERVER}, ?MODULE, {}, []).

-spec new_id() -> binary().
new_id() ->
    crypto:strong_rand_bytes(?ID_LENGTH).

-spec create(binary(), pid()) -> ok | {error, already_exists}.
create(<<Id/binary>>, Pid) when is_pid(Pid) ->
    %% TODO monitor pid to delete when process dies
    case ets:insert_new(?TABLE, #sendfile_session{id = Id, pid = Pid}) of
        true -> ok;
        false -> {error, already_exists}
    end.

-spec get(binary()) -> {error, get_error()} | {ok, pid()}.
get(<<Id/binary>>) ->
    case ets:lookup(?TABLE, Id) of
        [#sendfile_session{pid = Pid}] -> {ok, Pid};
        [] -> {error, not_found}
    end.

-spec delete(binary()) -> _.
delete(<<Id/binary>>) ->
    ets:delete(?TABLE, Id).

%%
%% gen_server callbacks
%%

init({}) ->
    ?LOG_INFO(?MODULE_STRING " starting on ~p", [node()]),
    ets:new(?TABLE, [set, public, named_table, {keypos, #?TABLE.id}]),
    {ok, nostate}.

handle_call(Request, From, State) ->
    ?LOG_WARNING("unknown call from ~p: ~p", [From, Request]),
    {reply, unknown_call, State}.

handle_cast(Message, State) ->
    ?LOG_WARNING("unknown cast: ~p", [Message]),
    {noreply, State}.

terminate(Reason, _State) ->
    ?LOG_INFO(?MODULE_STRING " stopping: ~p", [Reason]),
    ok.
