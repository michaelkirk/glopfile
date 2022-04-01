-module(sendfile_session_table).
-behaviour(gen_cluster_server).
-behaviour(sendfile_child).

-include_lib("kernel/include/logger.hrl").

%% API
-export([start_link/0, new_id/0, create/2, get/1, delete/1]).
-export_type([get_error/0]).
-ignore_xref([start_link/0]).                   % xref doesn't understand the MFA returned by child_spec/0

%% sendfile_child callbacks
-export([child_spec/0]).

%% gen_server callbacks
-export([init/2, dispatch_call/4, dispatch_cast/3]).

-define(SERVER, sendfile_session_table).
-define(TABLE, sendfile_session).
-record(?TABLE, {id, pid}).

-define(FRAGMENT_COUNT, 2).

-define(ID_LENGTH, 8).

-record(create_call,
        {id :: binary(),
         pid :: pid()}).

-record(get_call, {id :: binary()}).

-record(delete_call, {id :: binary()}).

-type get_error() :: not_found.

%%
%% API
%%

-spec start_link() -> ignore | {error, _} | {ok, pid() | {pid(), reference()}}.
start_link() ->
    SessionNodes = case sendfile_app:session_table_nodes() of
                            {ok, OkNodes} -> OkNodes;
                            undefined     -> [node()]
                        end,
    Node = node(),
    case SessionNodes of
        [Node | Peers] ->
            ?LOG_INFO(?MODULE_STRING " starting ~p as primary of ~p", [Node, Peers]),
            gen_cluster_server:start_link(?SERVER, ?MODULE, {Peers}, #{role => primary});
        _ -> case lists:member(node(), SessionNodes) of
            true ->
                 Peers = lists:delete(node(), SessionNodes),
                 ?LOG_INFO(?MODULE_STRING " starting ~p as fallback of session nodes: ~p", [Node, Peers]),
                 gen_cluster_server:start_link(?SERVER, ?MODULE, {Peers}, #{role => fallback});
            false -> ?LOG_INFO(?MODULE_STRING " ~p is not a session node. connecting to session nodes: ~p", [node(), SessionNodes]),
                 net_adm:ping_list(SessionNodes),
                 ignore
        end
    end.

-spec new_id() -> binary().
new_id() ->
    crypto:strong_rand_bytes(?ID_LENGTH).

-spec create(binary(), pid()) -> ok | {error, already_exists}.
create(<<Id/binary>>, Pid) when is_pid(Pid) ->
    gen_server:call({via, gen_cluster_client, {?SERVER, 1}}, #create_call{id = Id, pid = Pid}).

-spec get(binary()) -> {error, get_error()} | {ok, pid()}.
get(<<Id/binary>>) ->
    gen_server:call({via, gen_cluster_client, {?SERVER, 1}}, #get_call{id = Id}).

-spec delete(binary()) -> _.
delete(<<Id/binary>>) ->
    gen_server:call({via, gen_cluster_client, {?SERVER, 1}}, #delete_call{id = Id}).

%%
%% sendfile_child callbacks
%%

child_spec() ->
    #{id => ?MODULE,
      start => {?MODULE, start_link, []}}.

%%
%% gen_server callbacks
%%

init({Peers}, _Group) ->
    _ = mnesia:start(),
    {ok, _} = mnesia:change_config(extra_db_nodes, Peers),
    case gen_cluster_mnesia:create_table(?TABLE, ?FRAGMENT_COUNT, record_info(fields, ?TABLE), ram_copies) of
        {atomic, ok} -> ?LOG_INFO("created mnesia table ~p", [?TABLE]);
        {aborted, {already_exists, _}} ->
            case mnesia:add_table_copy(?TABLE, node(), ram_copies) of
                {atomic, ok} -> ?LOG_INFO("added copy of mnesia table ~p", [?TABLE]);
                {aborted, {already_exists, _, _}} -> ok
            end,
            Frags = mnesia:activity(transaction, fun mnesia:table_info/2, [?TABLE, frag_names], mnesia_frag),
            [ case mnesia:add_table_copy(Frag, node(), ram_copies) of
                  {atomic, ok} -> ?LOG_INFO("adding copy of mnesia table ~p fragment ~p", [?TABLE, Frag]);
                  {aborted, {already_exists, _, _}} -> ok
              end || Frag <- Frags ]
    end,
    {ok, nostate}.

dispatch_call(#create_call{id = Id, pid = Pid}, _From, _Group, State) ->
    Reply = gen_cluster_mnesia:write_new(?TABLE, #sendfile_session{id = Id, pid = Pid}, transaction),
    {reply, Reply, State};

dispatch_call(#get_call{id = Id}, _From, _Group, State) ->
    Reply = case gen_cluster_mnesia:read(?TABLE, Id, transaction) of
                [#sendfile_session{pid = Pid} | _] -> {ok, Pid};
                []                                 -> {error, not_found};
                {aborted, Err}                     -> {error, {internal, Err}};
                Err                                -> {error, {unknown, Err}}
            end,
    {reply, Reply, State};

dispatch_call(#delete_call{id = Id}, _From, _Group, State) ->
    gen_cluster_mnesia:delete(?TABLE, Id, transaction),
    {reply, ok, State};

dispatch_call(Request, From, _Group, State) ->
    ?LOG_WARNING("unknown call from ~p: ~p", [From, Request]),
    {reply, unknown_call, State}.

dispatch_cast(Message, _Group, State) ->
    ?LOG_WARNING("unknown cast: ~p", [Message]),
    {noreply, State}.
