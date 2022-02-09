-module(sendfile_websocket).
-behaviour(gen_server).

-include_lib("kernel/include/logger.hrl").

%% API
-export([start_link/2, start_opts/3, websocket_opts/0, recv/2, send/2, session_error/2]).
-export_type([start_opts/0, direction/0]).

%% gen_server callbacks
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2]).

-define(FRAME_SIZE_MAX, 102400).

-record(session,
       {pid :: pid(),
        monitor :: reference()}).
-type session() :: #session{}.

-record(state,
        {websocket_pid :: pid() | undefined,
         direction :: direction(),
         session :: session()}).

-type direction() :: upload | download.
-record(start_opts,
        {id :: binary(),
         session :: pid(),
         direction :: direction()}).
-opaque start_opts() :: #start_opts{}.

-record(recv_call, {message :: cow_ws:frame()}).

-record(send_cast, {frames :: [cow_ws:frame()]}).

-record(session_error_cast, {error :: any()}).


%%
%% API
%%


-spec start_link(WebsocketPid :: pid(), Arg :: start_opts()) -> {ok, pid()} | {error, any()} | ignore.
start_link(WebsocketPid, Arg) ->
    gen_server:start_link(?MODULE, {WebsocketPid, Arg}, []).

-spec start_opts(Id :: binary(), SessionPid :: pid(), direction()) -> start_opts().
start_opts(Id, SessionPid, Direction) ->
    #start_opts{id = Id, session = SessionPid, direction = Direction}.

-spec websocket_opts() -> cowboy_websocket:opts().
websocket_opts() ->
    #{max_frame_size => ?FRAME_SIZE_MAX}.

-spec recv(pid(), Message :: cow_ws:frame()) -> {ok, cowboy_websocket:commands()}.
recv(Pid, Message) ->
    gen_server:call(Pid, #recv_call{message = Message}).

-spec send(pid(), Message :: [cow_ws:frame()]) -> ok.
send(Pid, Frames) ->
    gen_server:cast(Pid, #send_cast{frames = Frames}).

-spec session_error(pid(), Error :: any()) -> ok.
session_error(Pid, Error) ->
    gen_server:cast(Pid, #session_error_cast{error = Error}).

%%
%% gen_server callbacks
%%

init({WebsocketPid, StartOpts}) ->
    #start_opts{id = Id, session = SessionPid, direction = Direction} = StartOpts,
    ?LOG_DEBUG("websocket ~p connected for ~p with id ~p", [WebsocketPid, Direction, Id]),

    ok = sendfile_session:start_websocket(SessionPid, Direction),
    process_flag(trap_exit, true),
    SessionMonitor = monitor(process, SessionPid),
    Session = #session{pid = SessionPid, monitor = SessionMonitor},
    {ok, #state{websocket_pid = WebsocketPid, direction = Direction, session = Session}}.

handle_call(#recv_call{message = Message}, From, State) ->
    handle_recv(Message, From, State);

handle_call(Request, From, State) ->
    ?LOG_WARNING("unknown call from ~p: ~p", [From, Request]),
    {reply, unknown_call, State}.

handle_cast(#send_cast{frames = Frames}, State) ->
    handle_send(Frames, State);

handle_cast(#session_error_cast{error = Error}, State) ->
    {stop, {session_error, Error}, State};

handle_cast(Message, State) ->
    ?LOG_WARNING("unknown cast: ~p", [Message]),
    {noreply, State}.

handle_info({'EXIT', WebsocketPid, _Reason}, #state{websocket_pid = WebsocketPid} = State) ->
    NewState = State#state{websocket_pid = undefined},
    {stop, normal, NewState};

handle_info({'EXIT', SessionPid, _Reason}, #state{session = #session{pid = SessionPid}} = State) ->
    {stop, session_died, State};

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
handle_recv({text, Data}, From, State) ->
    handle_recv_data({text, Data}, From, State);
handle_recv({binary, Data}, From, State) ->
    handle_recv_data({binary, Data}, From, State);
handle_recv(Message, _From, State) ->
    ?LOG_WARNING("unknown websocket message: ~p", [Message]),
    {reply, {ok, []}, State}.

handle_recv_data(Frame, _From, State) ->
    sendfile_session:websocket_data(State#state.session#session.pid, Frame, State#state.direction),
    {reply, {ok, []}, State}.

handle_send(Frames, State) ->
    sendfile_api_handler:websocket_send(State#state.websocket_pid, Frames),
    {noreply, State}.
