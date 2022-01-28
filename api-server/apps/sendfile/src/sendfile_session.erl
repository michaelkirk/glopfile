-module(sendfile_session).
-behaviour(gen_server).
-behaviour(sendfile_child).

-include_lib("kernel/include/logger.hrl").

%% API
-export([start/1, start_link/2, start_download/3, start_upload/3, upload_data/2, metadata/1]).
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

-record(uploader,
        {pid :: pid(),
         tag :: any(),
         monitor = undefined :: erlang:reference() | undefined,
         position :: non_neg_integer()}).
-type uploader() :: #uploader{}.

-type data() :: {more, binary()} | {data, binary()}.

-record(pending_data,
        {data :: data(),
         waiter :: {start_upload | upload_data, {pid(), any()}},
         position :: non_neg_integer(),
         size :: pos_integer()}).
-type pending_data() :: #pending_data{}.

-record(state,
        {id :: binary(),
         metadata :: binary(),
         downloader = undefined :: downloader() | undefined,
         uploader = undefined :: uploader() | undefined,
         pending = undefined :: pending_data() | undefined}).
-type state() :: #state{}.

-record(start_download_call, {from :: downloader()}).
-type start_download_call() :: #start_download_call{}.
-type start_download_result() :: {ok, SessionPid :: pid()}.

-record(start_upload_call, {from :: uploader()}).
-type start_upload_call() :: #start_upload_call{}.
-type start_upload_result() :: {ok, SessionPid :: pid()} | {position, non_neg_integer()}.

-record(upload_data_call, {data :: data()}).
-type upload_data_call() :: #upload_data_call{}.
-type upload_data_result() :: ok | {error, data_pending | connection_replaced} | {position, non_neg_integer()}.

-record(metadata_call, {}).
-type metadata_call() :: #metadata_call{}.
-type metadata_result() :: {ok, Metadata :: binary()}.

-type call() :: start_download_call() | start_upload_call() | upload_data_call() | metadata_call().

-type call_result() :: {error, sendfile_session_table:get_error()}.

-type download_error() :: connection_replaced.

-type upload_error() :: connection_replaced | wrong_position.

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

-spec start_upload(binary(), Tag :: any(), Position :: non_neg_integer()) -> start_upload_result() | call_result().
start_upload(<<Id/binary>>, Tag, Position) ->
    call(Id, #start_upload_call{from = #uploader{pid = self(), tag = Tag, position = Position}}, ?TIMEOUT).

-spec upload_data(pid(), data()) -> upload_data_result() | call_result().
upload_data(Pid, Data) ->
    gen_server:call(Pid, #upload_data_call{data = Data}, infinity).

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

handle_call(#start_upload_call{from = Uploader}, From, State) ->
    handle_start_upload_call(Uploader, From, State);

handle_call(#upload_data_call{data = Data}, From, State) ->
    handle_upload_data_call(Data, From, State);

handle_call(Request, From, State) ->
    ?LOG_WARNING("unknown call from ~p: ~p", [From, Request]),
    {reply, unknown_call, State}.

handle_cast(Message, State) ->
    ?LOG_WARNING("unknown cast: ~p", [Message]),
    {noreply, State}.

handle_info({'DOWN', _Mon, process, Pid, Info}, #state{downloader = #downloader{pid = Pid}}=State) ->
    ?LOG_DEBUG("downloader stopped: ~p", [Info]),
    NewState = handle_downloader_disconnected(State),
    {noreply, NewState};

handle_info({'DOWN', _Mon, process, Pid, Info}, #state{uploader = #uploader{pid = Pid}}=State) ->
    ?LOG_DEBUG("uploader stopped: ~p", [Info]),
    NewState = handle_uploader_disconnected(State),
    {noreply, NewState};

handle_info(Message, State) ->
    ?LOG_WARNING("unknown message: ~p", [Message]),
    {noreply, State}.

terminate(_Reason, State) ->
    ?LOG_DEBUG("session stopping: ~p", [State#state.id]),
    sendfile_session_table:delete(State#state.id),
    ok.

%%
%% call handlers
%%

-type handle_call_result(Reply) :: {reply, Reply, state()} | {noreply, state()} | {stop, Reason :: any(), Reply, state()}.

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
                send_data(PendingData, Downloader),
                pending_data_reply(PendingWaiter, ok),
                Downloader#downloader{position = Downloader#downloader.position + PendingDataSize};
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
                terminate_uploader(State#state.uploader, wrong_position),
                State#state{uploader = undefined};
            _ ->
                State
        end,
    Monitor = monitor(process, Downloader#downloader.pid),
    {reply, {ok, self()}, UploaderUpdatedState#state{downloader = Downloader#downloader{monitor = Monitor}}}.

-spec handle_start_upload_call(uploader(), From :: {pid(), any()}, state()) -> handle_call_result(start_upload_result()).
%% another uploader is already connected
handle_start_upload_call(NewUploader, From, #state{uploader = #uploader{}=OldUploader}=State) ->
    terminate_uploader(OldUploader, connection_replaced),
    %% remove the uploader and recurse
    handle_start_upload_call(NewUploader, From, State#state{uploader = undefined});

%% new uploader position doesn't match connected downloader position
handle_start_upload_call(Uploader, _From, #state{downloader = #downloader{position = Position}}=State)
  when Uploader#uploader.position =/= Position ->
    {reply, {position, Position}, State};

%% new uploader position doesn't match pending data position
handle_start_upload_call(Uploader, _From, #state{pending = #pending_data{position = Position, size = PendingDataSize}}=State)
  when Uploader#uploader.position =/= Position + PendingDataSize ->
    {reply, {position, Position}, State};

%% no other uploader is connected
handle_start_upload_call(Uploader, From, State) ->
    Monitor = monitor(process, Uploader#uploader.pid),
    case State#state.pending of
        #pending_data{}=Pending ->
            UpdatedWaiterPending = Pending#pending_data{waiter = {start_upload, From}},
            {noreply, State#state{uploader = Uploader#uploader{monitor = Monitor}, pending = UpdatedWaiterPending}};
        _ ->
            {reply, {ok, self()}, State#state{uploader = Uploader#uploader{monitor = Monitor}}}
    end.

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

%% uploader and downloader are both connected
handle_upload_data_call(Data, _From, #state{downloader = #downloader{}}=State) ->
    %% assert that downloader position matches uploader position
    #state{downloader = #downloader{position = Position}, uploader = #uploader{position = Position}} = State,
    {_, DataBin} = Data,
    NewPosition = Position + iolist_size(DataBin),
    send_data(Data, State#state.downloader),
    NewState = State#state{downloader = State#state.downloader#downloader{position = NewPosition},
                           uploader = State#state.uploader#uploader{position = NewPosition}},
    {reply, ok, NewState};

%% only uploader is connected
handle_upload_data_call({_, DataBin}=Data, From, State) ->
    Position = State#state.uploader#uploader.position,
    DataSize = iolist_size(DataBin),
    {noreply, State#state{pending = #pending_data{data = Data, waiter = {upload_data, From}, position = Position, size = DataSize},
                          uploader = State#state.uploader#uploader{position = Position + DataSize}}}.

-spec handle_uploader_disconnected(state()) -> state().
handle_uploader_disconnected(State) ->
    State#state{uploader = undefined}.

-spec handle_downloader_disconnected(state()) -> state().
handle_downloader_disconnected(State) ->
    State#state{downloader = undefined}.

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

-spec terminate_uploader(uploader(), upload_error()) -> _.
terminate_uploader(#uploader{pid = Pid, tag = Tag, monitor = Monitor}, Err) ->
    Pid ! {Tag, {error, Err}},
    demonitor(Monitor).

-spec terminate_downloader(downloader(), download_error()) -> _.
terminate_downloader(#downloader{pid = Pid, tag = Tag, monitor = Monitor}, Err) ->
    Pid ! {Tag, {error, Err}},
    demonitor(Monitor).

-spec send_data(data(), downloader()) -> _.
send_data(Data, #downloader{pid = Pid, tag = Tag}) ->
    Pid ! {Tag, Data}.
