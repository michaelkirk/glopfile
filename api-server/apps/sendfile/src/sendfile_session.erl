-module(sendfile_session).
-behaviour(gen_server).
-behaviour(sendfile_child).

-include_lib("kernel/include/logger.hrl").
-include("sendfile_websocket_protocol.hrl").

%% API
-export([start/1, start_link/2, start_download/3, finish_download/1, start_upload/3, start_websocket/2, upload_data/2,
         websocket_data/3, metadata/1]).
-ignore_xref([start_link/2]).                   % xref doesn't take simple_one_for_one children into account

%% sendfile_child callbacks
-export([child_spec/0]).

%% gen_server callbacks
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2]).

-define(SUPERVISOR, sendfile_session_sup).
-define(TIMEOUT, 5000).

-record(downloader,
        {pid :: pid(),
         tag :: any(),
         monitor = undefined :: erlang:reference() | undefined,
         position :: non_neg_integer()}).
-type downloader() :: #downloader{}.

-record(downloader_ws,
        {pid :: pid(),
         monitor = undefined :: erlang:reference() | undefined}).
-type downloader_ws() :: #downloader_ws{}.

-record(uploader,
        {pid :: pid(),
         tag :: any(),
         monitor = undefined :: erlang:reference() | undefined,
         position :: non_neg_integer()}).
-type uploader() :: #uploader{}.

-record(uploader_ws,
        {pid :: pid(),
         monitor = undefined :: erlang:reference() | undefined}).
-type uploader_ws() :: #uploader_ws{}.

-type data() :: {more, binary()} | {data, binary()}.

-record(pending_data,
        {data :: data(),
         waiter :: {start_upload | upload_data, {pid(), any()}},
         position :: non_neg_integer(),
         size :: pos_integer()}).
-type pending_data() :: #pending_data{}.

-record(pending_error,
        {error :: upload_error()}).
-type pending_error() :: #pending_error{}.

-record(state,
        {id :: binary(),
         metadata :: binary(),
         downloader = undefined :: downloader() | finished | undefined,
         downloader_ws = undefined :: downloader_ws() | undefined,
         uploader = undefined :: uploader() | undefined,
         uploader_ws = undefined :: uploader_ws() | undefined,
         pending = undefined :: pending_data() | pending_error() | undefined}).
-type state() :: #state{}.

-record(start_download_call, {from :: downloader()}).
-type start_download_call() :: #start_download_call{}.
-type start_download_result() :: {ok, SessionPid :: pid()}.

-record(finish_download_call, {}).
-type finish_download_call() :: #finish_download_call{}.
-type finish_download_result() :: ok.

-record(start_upload_call, {from :: uploader()}).
-type start_upload_call() :: #start_upload_call{}.
-type start_upload_result() :: {ok, SessionPid :: pid()} | upload_error().

-record(start_websocket_call, {from :: uploader_ws() | downloader_ws()}).
-type start_websocket_call() :: #start_websocket_call{}.
-type start_websocket_result() :: ok.

-record(upload_data_call, {data :: data()}).
-type upload_data_call() :: #upload_data_call{}.
-type upload_data_result() :: ok | {error, data_pending} | upload_error().

-type websocket_data() :: {text | binary, iodata()}.
-record(websocket_data_cast, {frame :: websocket_data(), from :: sendfile_websocket:direction()}).
-type websocket_data_result() :: ok | {error, not_connected}.

-record(metadata_call, {}).
-type metadata_call() :: #metadata_call{}.
-type metadata_result() :: {ok, Metadata :: binary()}.

-type call() :: start_download_call() | finish_download_call() | start_upload_call() | start_websocket_call() |
                upload_data_call() | metadata_call().

-type call_result() :: {error, sendfile_session_table:get_error()}.

-type download_error() :: connection_replaced.

-type download_ws_error() :: connection_replaced.

-type upload_error() :: {error, connection_replaced} | {position, non_neg_integer()} | finished.

-type upload_ws_error() :: connection_replaced.

%%
%% API
%%

-spec start(Metadata :: binary()) -> {ok, pid(), Id :: binary()}.
start(Metadata) ->
    Id = sendfile_session_table:new_id(),
    {ok, Pid} = ?SUPERVISOR:start_child([Id, Metadata]),
    {ok, Pid, Id}.

-spec start_link(Id :: binary(), Metadata :: binary()) -> {ok, pid() | {pid(), reference()}} | {error, _} | ignore.
start_link(Id, Metadata) ->
    gen_server:start_link(?MODULE, {Id, Metadata}, []).

-spec start_download(binary(), Tag :: any(), Position :: non_neg_integer()) -> start_download_result() | call_result().
start_download(<<Id/binary>>, Tag, Position) ->
    call(Id, #start_download_call{from = #downloader{pid = self(), tag = Tag, position = Position}}, ?TIMEOUT).

-spec finish_download(binary()) -> finish_download_result() | call_result().
finish_download(<<Id/binary>>) ->
    call(Id, #finish_download_call{}, ?TIMEOUT).

-spec start_upload(binary(), Tag :: any(), Position :: non_neg_integer()) -> start_upload_result() | call_result().
start_upload(<<Id/binary>>, Tag, Position) ->
    call(Id, #start_upload_call{from = #uploader{pid = self(), tag = Tag, position = Position}}, ?TIMEOUT).

-spec start_websocket(pid(), sendfile_websocket:direction()) -> start_websocket_result() | call_result().
start_websocket(Pid, upload) ->
    gen_server:call(Pid, #start_websocket_call{from = #uploader_ws{pid = self()}}, ?TIMEOUT);
start_websocket(Pid, download) ->
    gen_server:call(Pid, #start_websocket_call{from = #downloader_ws{pid = self()}}, ?TIMEOUT).

-spec upload_data(pid(), data()) -> upload_data_result() | call_result().
upload_data(Pid, Data) ->
    gen_server:call(Pid, #upload_data_call{data = Data}, infinity).

-spec websocket_data(pid(), websocket_data(), sendfile_websocket:direction()) -> websocket_data_result() | call_result().
websocket_data(Pid, Frame, From) ->
    gen_server:cast(Pid, #websocket_data_cast{frame = Frame, from = From}).

-spec metadata(binary()) -> metadata_result() | call_result().
metadata(<<Id/binary>>) ->
    call(Id, #metadata_call{}, ?TIMEOUT).

-spec call(binary(), call(), timeout()) -> call_result() | CallResult :: any().
call(<<Id/binary>>, Req, Timeout) ->
    case sendfile_session_table:get(Id) of
        {ok, Pid}    -> gen_server:call(Pid, Req, Timeout);
        {error, Err} -> {error, Err}
    end.

%%
%% sendfile_child callbacks
%%

-spec child_spec() -> supervisor:child_spec().
child_spec() ->
    #{id => ?MODULE,
      start => {?MODULE, start_link, []},
      restart => transient}.

%%
%% gen_server callbacks
%%

init({Id, Metadata}) ->
    case sendfile_session_table:create(Id, self()) of
        ok ->
            ?LOG_DEBUG("session started: ~p", [Id]);
        {error, already_exists} ->
            ?LOG_DEBUG("session restarted: ~p", [Id])
    end,
    {ok, #state{id = Id, metadata = Metadata}}.

handle_call(#metadata_call{}, From, State) ->
    handle_metadata_call(From, State);

handle_call(#start_download_call{from = Downloader}, From, State) ->
    handle_start_download_call(Downloader, From, State);

handle_call(#finish_download_call{}, From, State) ->
    handle_finish_download_call(From, State);

handle_call(#start_upload_call{from = Uploader}, From, State) ->
    handle_start_upload_call(Uploader, From, State);

handle_call(#start_websocket_call{from = FromWs}, From, State) ->
    handle_start_websocket_call(FromWs, From, State);

handle_call(#upload_data_call{data = Data}, From, State) ->
    handle_upload_data_call(Data, From, State);

handle_call(Request, From, State) ->
    ?LOG_WARNING("unknown call from ~p: ~p", [From, Request]),
    {reply, unknown_call, State}.

handle_cast(#websocket_data_cast{frame = Frame, from = FromDirection}, State) ->
    handle_websocket_data_cast(Frame, FromDirection, State);

handle_cast(Message, State) ->
    ?LOG_WARNING("unknown cast: ~p", [Message]),
    {noreply, State}.

handle_info({'DOWN', _Mon, process, Pid, Info}, #state{downloader = #downloader{pid = Pid}}=State) ->
    ?LOG_DEBUG("downloader stopped: ~p", [Info]),
    handle_downloader_disconnected(State, Info);

handle_info({'DOWN', _Mon, process, Pid, Info}, #state{downloader_ws = #downloader_ws{pid = Pid}}=State) ->
    ?LOG_DEBUG("downloader websocket stopped: ~p", [Info]),
    NewState = handle_downloader_ws_disconnected(State),
    {noreply, NewState};

handle_info({'DOWN', _Mon, process, Pid, Info}, #state{uploader = #uploader{pid = Pid}}=State) ->
    ?LOG_DEBUG("uploader stopped: ~p", [Info]),
    handle_uploader_disconnected(State);

handle_info({'DOWN', _Mon, process, Pid, Info}, #state{uploader_ws = #uploader_ws{pid = Pid}}=State) ->
    ?LOG_DEBUG("uploader websocket stopped: ~p", [Info]),
    NewState = handle_uploader_ws_disconnected(State),
    {noreply, NewState};

handle_info(Message, State) ->
    ?LOG_WARNING("unknown message: ~p", [Message]),
    {noreply, State}.

terminate(Reason, State) ->
    ?LOG_DEBUG("session stopping: ~p, reason: ~p", [State#state.id, Reason]),
    sendfile_session_table:delete(State#state.id),
    ok.

%%
%% call handlers
%%

-type handle_call_result(Reply) :: {reply, Reply, state()} | {noreply, state()} | {stop, Reason :: any(), Reply, state()}.
-type handle_cast_result() :: {noreply, state()} | {stop, Reason :: any(), state()}.

-spec handle_metadata_call(From :: {pid(), any()}, state()) -> handle_call_result(metadata_result()).
handle_metadata_call(_From, State) ->
    {reply, {ok, State#state.metadata}, State}.


-spec handle_start_download_call(downloader(), From :: {pid(), any()}, state()) -> handle_call_result(start_download_result()).
%% another downloader is already connected
handle_start_download_call(NewDownloader, From, #state{downloader = #downloader{}=OldDownloader}=State) ->
    terminate_downloader(OldDownloader, connection_replaced),
    %% replace the downloader and recurse
    handle_start_download_call(NewDownloader, From, State#state{downloader = undefined});

%% transfer data is pending (queued)
handle_start_download_call(Downloader, From, #state{pending = #pending_data{}=Pending}=State) ->
    #pending_data{data = PendingData, waiter = PendingWaiter, position = PendingPosition, size = PendingDataSize} = Pending,
    PositionUpdatedDownloader =
        case Downloader#downloader.position of
            PendingPosition ->
                NewPosition = Downloader#downloader.position + PendingDataSize,
                send_data(PendingData, Downloader),
                send_upload_progress(NewPosition, State#state.uploader_ws),
                pending_data_reply(PendingWaiter, ok),
                Downloader#downloader{position = NewPosition};
            DownloaderPosition ->
                pending_data_reply(PendingWaiter, {position, DownloaderPosition}),
                Downloader
        end,
    %% remove the pending data and recurse
    handle_start_download_call(PositionUpdatedDownloader, From, State#state{pending = undefined});

%% no other downloader is connected
handle_start_download_call(Downloader, _From, State) ->
    UploaderUpdatedState =
        case State#state.uploader of
            %% connected uploader position doesn't match new downloader position
            #uploader{position = UploaderPosition}
              when UploaderPosition =/= Downloader#downloader.position ->
                terminate_uploader(State, {position, Downloader#downloader.position});
            _ ->
                State
        end,
    Monitor = monitor(process, Downloader#downloader.pid),
    {reply, {ok, self()}, UploaderUpdatedState#state{downloader = Downloader#downloader{monitor = Monitor}}}.


-spec handle_finish_download_call(downloader(), state()) -> handle_call_result(finish_download_result()).
handle_finish_download_call(_From, State) ->
    case State#state.downloader of
        undefined -> ok;
        finished -> ok;
        OldDownloader -> terminate_downloader(OldDownloader, connection_replaced)
    end,
    NewState = terminate_uploader(State#state{downloader = finished}, finished),
    case NewState#state.uploader of
        undefined ->
            {stop, normal, ok, NewState};
        _ ->
            {reply, ok, NewState}
    end.

-spec handle_start_upload_call(uploader(), From :: {pid(), any()}, state()) -> handle_call_result(start_upload_result()).
%% another uploader is already connected
handle_start_upload_call(NewUploader, From, #state{uploader = #uploader{}}=State) ->
    NewState = terminate_uploader(State, {error, connection_replaced}),
    %% remove the uploader and recurse
    handle_start_upload_call(NewUploader, From, NewState#state{uploader = undefined});

%% new uploader position doesn't match connected downloader position
handle_start_upload_call(Uploader, _From, #state{downloader = #downloader{position = Position}}=State)
  when Uploader#uploader.position =/= Position ->
    {reply, {position, Position}, State};

%% new uploader position doesn't match pending data position
handle_start_upload_call(Uploader, _From, #state{pending = #pending_data{position = Position, size = PendingDataSize}}=State)
  when Uploader#uploader.position =/= Position + PendingDataSize ->
    {reply, {position, Position + PendingDataSize}, State};

%% no other uploader is connected
handle_start_upload_call(Uploader, From, State) ->
    Monitor = monitor(process, Uploader#uploader.pid),
    case State#state.pending of
        #pending_data{}=Pending ->
            send_upload_progress(Uploader#uploader.position, State#state.uploader_ws),
            UpdatedWaiterPending = Pending#pending_data{waiter = {start_upload, From}},
            {noreply, State#state{uploader = Uploader#uploader{monitor = Monitor}, pending = UpdatedWaiterPending}};
        #pending_error{}=Pending ->
            {reply, Pending#pending_error.error, State#state{pending = undefined}};
        _ ->
            send_upload_progress(Uploader#uploader.position, State#state.uploader_ws),
            {reply, {ok, self()}, State#state{uploader = Uploader#uploader{monitor = Monitor}}}
    end.


-spec handle_start_websocket_call(downloader_ws() | uploader_ws(), From :: {pid(), any()}, state()) -> Res when
      Res :: handle_call_result(start_websocket_result()).
handle_start_websocket_call(#downloader_ws{}=DownloaderWs, From, State) ->
    handle_start_download_websocket_call(DownloaderWs, From, State);

handle_start_websocket_call(#uploader_ws{}=UploaderWs, From, State) ->
    handle_start_upload_websocket_call(UploaderWs, From, State).


-spec handle_start_download_websocket_call(downloader_ws(), From :: {pid(), any()}, state()) -> Res when
      Res :: handle_call_result(start_websocket_result()).
%% another downloader websocket is already connected
handle_start_download_websocket_call(DownloaderWs, From, #state{downloader_ws = #downloader_ws{}=OldDownloaderWs}=State) ->
    %% replace the downloader websocket and recurse
    terminate_downloader_ws(OldDownloaderWs, connection_replaced),
    handle_start_download_websocket_call(DownloaderWs, From, State#state{downloader_ws = undefined});

%% no other downloader websocket is connected
handle_start_download_websocket_call(DownloaderWs, _From, State) ->
    Monitor = monitor(process, DownloaderWs#downloader_ws.pid),
    {reply, ok, State#state{downloader_ws = DownloaderWs#downloader_ws{monitor = Monitor}}}.


-spec handle_start_upload_websocket_call(uploader_ws(), From :: {pid(), any()}, state()) -> Res when
      Res :: handle_call_result(start_websocket_result()).
%% another uploader websocket is already connected
handle_start_upload_websocket_call(UploaderWs, From, #state{uploader_ws = #uploader_ws{}=OldUploaderWs}=State) ->
    %% replace the uploader websocket and recurse
    terminate_uploader_ws(OldUploaderWs, connection_replaced),
    handle_start_upload_websocket_call(UploaderWs, From, State#state{uploader_ws = undefined});

%% no other uploader websocket is connected
handle_start_upload_websocket_call(UploaderWs, _From, State) ->
    Monitor = monitor(process, UploaderWs#uploader_ws.pid),
    {reply, ok, State#state{uploader_ws = UploaderWs#uploader_ws{monitor = Monitor}}}.


-spec handle_upload_data_call(data(), From :: {pid(), any()}, state()) -> handle_call_result(upload_data_result()).
%% caller error; uploader pid doesn't match the caller
handle_upload_data_call(_Data, {FromPid, _}, #state{uploader = #uploader{pid = UploaderPid}}=State)
  when FromPid =/= UploaderPid ->
    {reply, {error, connection_replaced}, State};

%% caller error; no uploader is connected
handle_upload_data_call(_Data, _From, #state{uploader = undefined}=State) ->
    {reply, {error, connection_replaced}, State};

%% caller error; data is already pending
handle_upload_data_call(_Data, _From, #state{pending = #pending_data{}}=State) ->
    {reply, {error, data_pending}, State};

%% an upload error is pending
handle_upload_data_call(_Data, _From, #state{pending = #pending_error{}}=State) ->
    {reply, State#state.pending#pending_error.error, State};

%% uploader and downloader are both connected
handle_upload_data_call(Data, _From, #state{downloader = #downloader{}}=State) ->
    %% assert that downloader position matches uploader position
    #state{downloader = #downloader{position = Position}, uploader = #uploader{position = Position}} = State,
    {_, DataBin} = Data,
    NewPosition = Position + iolist_size(DataBin),
    send_data(Data, State#state.downloader),
    send_upload_progress(NewPosition, State#state.uploader_ws),
    NewState = State#state{downloader = State#state.downloader#downloader{position = NewPosition},
                           uploader = State#state.uploader#uploader{position = NewPosition}},
    {reply, ok, NewState};

%% only uploader is connected
handle_upload_data_call({_, DataBin}=Data, From, State) ->
    Position = State#state.uploader#uploader.position,
    DataSize = iolist_size(DataBin),
    {noreply, State#state{pending = #pending_data{data = Data, waiter = {upload_data, From}, position = Position, size = DataSize},
                          uploader = State#state.uploader#uploader{position = Position + DataSize}}}.


-spec handle_websocket_data_cast(websocket_data(), sendfile_websocket:direction(), state()) -> handle_cast_result().
%% frame received from downloader, when an uploader is connected
handle_websocket_data_cast(Frame, download, #state{uploader_ws = #uploader_ws{pid = ToPid}}=State) ->
    sendfile_websocket:send(ToPid, [Frame]),
    {noreply, State};

%% frame received from uploader, when a downloader is connected
handle_websocket_data_cast(Frame, upload, #state{downloader_ws = #downloader_ws{pid = ToPid}}=State) ->
    sendfile_websocket:send(ToPid, [Frame]),
    {noreply, State};

%% frame received, in any other case
handle_websocket_data_cast(_Frame, _Direction, State) ->
    {noreply, State}.


-spec handle_uploader_disconnected(state()) -> handle_cast_result().
%% download is finished
handle_uploader_disconnected(#state{downloader = finished}=State) ->
    {stop, normal, State#state{uploader = undefined}};

%% download is not finished
handle_uploader_disconnected(State) ->
    %% clear any pending error for this specific uploader
    NewPending =
        case State#state.pending of
            #pending_error{} -> undefined;
            Pending -> Pending
        end,
    {noreply, State#state{uploader = undefined, pending = NewPending}}.


-spec handle_uploader_ws_disconnected(state()) -> state().
handle_uploader_ws_disconnected(State) ->
    State#state{uploader_ws = undefined}.


-spec handle_downloader_disconnected(state(), any()) -> handle_cast_result().

handle_downloader_disconnected(State, normal) ->
    NewState = State#state{downloader = finished},
    {stop, normal, NewState};

handle_downloader_disconnected(State, _Info) ->
    NewState = State#state{downloader = undefined},
    {noreply, NewState}.

-spec handle_downloader_ws_disconnected(state()) -> state().
handle_downloader_ws_disconnected(State) ->
    State#state{downloader_ws = undefined}.


-spec pending_data_reply(From :: {start_upload | upload_data, {pid(), any()}}, Reply :: ok | {position, non_neg_integer()}) -> _.
pending_data_reply({start_upload, From}, ok)    -> start_upload_reply(From, {ok, self()});
pending_data_reply({start_upload, From}, Reply) -> start_upload_reply(From, Reply);
pending_data_reply({upload_data, From}, Reply)  -> upload_data_reply(From, Reply).

-spec start_upload_reply(From :: {pid(), any()}, Reply :: start_upload_result()) -> _.
start_upload_reply(From, Reply) ->
    gen_server:reply(From, Reply).

-spec upload_data_reply(From :: {pid(), any()}, Reply :: upload_data_result()) -> _.
upload_data_reply(From, Reply) ->
    gen_server:reply(From, Reply).


-spec terminate_uploader(state(), upload_error()) -> state().
%% no uploader is connected
terminate_uploader(#state{uploader = undefined}=State, _Err) ->
    State;

%% the uploader is waiting on a call
terminate_uploader(#state{uploader = #uploader{}, pending = #pending_data{waiter = {WaiterCall, From}}}=State, Err) ->
    demonitor(State#state.uploader#uploader.monitor),
    case WaiterCall of
        start_upload -> start_upload_reply(From, Err);
        upload_data -> upload_data_reply(From, Err)
    end,
    State#state{uploader = undefined};

%% the uploader is not waiting on a call
terminate_uploader(#state{uploader = #uploader{}}=State, Err) ->
    %% queue the uploader error so it'll be returned from the next call by the uploader
    State#state{pending = #pending_error{error = Err}}.


-spec terminate_uploader_ws(uploader_ws(), upload_ws_error()) -> _.
terminate_uploader_ws(#uploader_ws{pid = Pid, monitor = Monitor}, Err) ->
    sendfile_websocket:session_error(Pid, Err),
    demonitor(Monitor).

-spec terminate_downloader(downloader(), download_error()) -> _.
terminate_downloader(#downloader{pid = Pid, tag = Tag, monitor = Monitor}, Err) ->
    Pid ! {Tag, {error, Err}},
    demonitor(Monitor).

-spec terminate_downloader_ws(downloader_ws(), download_ws_error()) -> _.
terminate_downloader_ws(#downloader_ws{pid = Pid, monitor = Monitor}, Err) ->
    sendfile_websocket:session_error(Pid, Err),
    demonitor(Monitor).

-spec send_data(data(), downloader()) -> _.
send_data(Data, #downloader{pid = Pid, tag = Tag}) ->
    Pid ! {Tag, Data}.

-spec send_upload_progress(non_neg_integer(), uploader_ws() | undefined) -> ok.
send_upload_progress(_Pos, undefined) ->
    ok;
send_upload_progress(Pos, #uploader_ws{pid = WsPid}) ->
    Msg = #sendfile_websocket_protocol_web_socket_message{
             inner = {upload_data_ack, #sendfile_websocket_protocol_upload_data_ack{offset = Pos}}
            },
    MsgData = sendfile_websocket_protocol:encode_msg(Msg),
    sendfile_websocket:send(WsPid, [{binary, MsgData}]).
