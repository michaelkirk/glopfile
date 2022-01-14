-module(sendfile_websocket).
-behaviour(gen_server).

-include_lib("kernel/include/logger.hrl").

%% API
-export([start_link/2, start_opts/0, websocket_opts/0, recv/2]).
-export_type([start_opts/0]).

%% gen_server callbacks
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2]).

-define(FRAME_SIZE_MAX, 102400).

-record(state,
        {websocket_pid :: pid() | undefined}).

-opaque start_opts() :: {}.

-record(recv_call, {message :: cow_ws:frame()}).

%%
%% API
%%


-spec start_link(WebsocketPid :: pid(), Arg :: start_opts()) -> {ok, pid()} | {error, any()} | ignore.
start_link(WebsocketPid, Arg) ->
    gen_server:start_link(?MODULE, {WebsocketPid, Arg}, []).

-spec start_opts() -> start_opts().
start_opts() ->
    {}.

-spec websocket_opts() -> cowboy_websocket:opts().
websocket_opts() ->
    #{max_frame_size => ?FRAME_SIZE_MAX}.

-spec recv(pid(), Message :: cow_ws:frame()) -> {ok, cowboy_websocket:commands()}.
recv(Pid, Message) ->
    gen_server:call(Pid, #recv_call{message = Message}).

%%
%% gen_server callbacks
%%

init({WebsocketPid, {}=_Arg}) ->
    ?LOG_DEBUG("websocket ~p connected", [WebsocketPid]),
    process_flag(trap_exit, true),
    {ok, #state{websocket_pid = WebsocketPid}}.

handle_call(#recv_call{message = Message}, _From, State) ->
    handle_recv(Message, _From, State);

handle_call(Request, From, State) ->
    ?LOG_WARNING("unknown call from ~p: ~p", [From, Request]),
    {reply, unknown_call, State}.

handle_cast(Message, State) ->
    ?LOG_WARNING("unknown cast: ~p", [Message]),
    {noreply, State}.

handle_info({'EXIT', WebsocketPid, _Reason}, #state{websocket_pid = WebsocketPid} = State) ->
    NewState = State#state{websocket_pid = undefined},
    {stop, normal, NewState};

handle_info(Message, State) ->
    ?LOG_WARNING("unknown message: ~p", [Message]),
    {noreply, State}.

terminate(Reason, State) ->
    ?LOG_DEBUG("websocket ~p disconnected: ~p", [State#state.websocket_pid, Reason]),
    case State#state.websocket_pid of
        undefined -> ok;
        WebsocketPid -> sendfile_api_handler:websocket_stop(WebsocketPid)
    end,
    ok.

%%
%% private functions
%%

handle_recv(ping, _From, State) ->
    {reply, {ok, [pong]}, State};
handle_recv({ping, _}, _From, State) ->
    {reply, {ok, [pong]}, State};
handle_recv(pong, _From, State) ->
    {reply, {ok, []}, State};
handle_recv({pong, _}, _From, State) ->
    {reply, {ok, []}, State};
handle_recv(close, _From, State) ->
    ?LOG_DEBUG("websocket ~s closed", []),
    {stop, normal, State};
handle_recv({close, Code, Reason}, _From, State) ->
    ?LOG_DEBUG("websocket ~s closed for reason ~b: ~s", [Code, Reason]),
    {stop, normal, State};
handle_recv({_, <<>>}, _From, State) ->
    {reply, {ok, [{binary, <<>>}]}, State};
handle_recv(Message, _From, State) ->
    ?LOG_WARNING("unknown websocket message: ~p", [Message]),
    {reply, {ok, []}, State}.
