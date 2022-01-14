-module(sendfile_session).
-behaviour(gen_server).

-include_lib("kernel/include/logger.hrl").

%% API
-export([start/1, simple_child_spec/0, start_link/2, start_download/2, start_upload/2, upload_data/2, metadata/1]).

%% gen_server callbacks
-export([init/1, handle_call/3, handle_cast/2, handle_info/2, terminate/2]).

-define(SUPERVISOR, sendfile_session_sup).
-define(TIMEOUT, 5000).

-record(downloader,
        {pid :: pid(),
         tag :: any()}).

-record(uploader,
        {pid :: pid(),
         tag :: any()}).

-type data() :: {more, binary()} | {data, binary()}.

-record(pending_data,
        {data :: data(),
         from :: {pid(), any()}}).

-record(state,
        {id :: binary(),
         metadata :: binary(),
         downloader = undefined :: #downloader{} | undefined,
         uploader = undefined :: #uploader{} | undefined,
         pending = undefined :: #pending_data{} | undefined}).

-record(start_download_call, {from :: #downloader{}}).
-type start_download_result() :: {ok, SessionPid :: pid()}.

-record(start_upload_call, {from :: #uploader{}}).
-type start_upload_result() :: {ok, SessionPid :: pid()}.

-record(upload_data_call, {data :: data()}).
-type upload_data_result() :: ok | {error, data_pending}.

-record(metadata_call, {}).
-type metadata_result() :: {ok, Metadata :: binary()}.

-type call() :: #start_download_call{} | #start_upload_call{} | #upload_data_call{} | #metadata_call{}.

-type call_result() :: {error, sendfile_session_table:get_error()}.

%%
%% API
%%

-spec start(Metadata :: binary()) -> {ok, pid(), Id :: binary()}.
start(Metadata) ->
    Id = sendfile_session_table:new_id(),
    {ok, Pid} = ?SUPERVISOR:start_child([Id, Metadata]),
    {ok, Pid, Id}.

-spec simple_child_spec() -> supervisor:child_spec().
simple_child_spec() ->
    #{id => ?MODULE,
      start => {?MODULE, start_link, []},
      restart => transient}.

-spec start_link(Id :: binary(), Metadata :: binary()) -> {ok, pid() | {pid(), reference()}} | {error, _} | ignore.
start_link(Id, Metadata) ->
    gen_server:start_link(?MODULE, {Id, Metadata}, []).

-spec start_download(binary(), Tag :: any()) -> start_download_result() | call_result().
start_download(<<Id/binary>>, Tag) ->
    call(Id, #start_download_call{from = #downloader{pid = self(), tag = Tag}}, ?TIMEOUT).

-spec start_upload(binary(), Tag :: any()) -> start_upload_result() | call_result().
start_upload(<<Id/binary>>, Tag) ->
    call(Id, #start_upload_call{from = #uploader{pid = self(), tag = Tag}}, ?TIMEOUT).

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
    {stop, normal, State};

handle_info({'DOWN', _Mon, process, Pid, Info}, #state{uploader = #uploader{pid = Pid}}=State) ->
    ?LOG_DEBUG("uploader stopped: ~p", [Info]),
    {stop, normal, State};

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

-type handle_call_result(Reply) :: {reply, Reply, #state{}} | {noreply, #state{}} | {stop, Reason :: any(), Reply, #state{}}.

-spec handle_metadata_call(From :: {pid(), any()}, #state{}) -> handle_call_result(metadata_result()).
handle_metadata_call(_From, State) ->
    {reply, {ok, State#state.metadata}, State}.

-spec handle_start_download_call(#downloader{}, From :: {pid(), any()}, #state{}) -> handle_call_result(start_download_result()).
handle_start_download_call(Downloader, From, #state{downloader = undefined, pending = #pending_data{}=Pending}=State) ->
    #pending_data{data = PendingData, from = PendingFrom} = Pending,
    send_data(PendingData, Downloader),
    gen_server:reply(PendingFrom, ok),
    handle_start_download_call(Downloader, From, State#state{pending = undefined});

handle_start_download_call(Downloader, _From, #state{downloader = undefined, pending = undefined}=State) ->
    monitor(process, Downloader#downloader.pid),
    {reply, {ok, self()}, State#state{downloader = Downloader}};

handle_start_download_call(_Downloader, _From, #state{downloader = #downloader{}}=State) ->
    {reply, {error, resume_not_supported}, State}.

-spec handle_start_upload_call(#uploader{}, From :: {pid(), any()}, #state{}) -> handle_call_result(start_upload_result()).
handle_start_upload_call(Uploader, _From, State) ->
    monitor(process, Uploader#uploader.pid),
    {reply, {ok, self()}, State#state{uploader = Uploader}}.

-spec handle_upload_data_call(data(), From :: {pid(), any()}, #state{}) -> handle_call_result(upload_data_result()).
handle_upload_data_call(Data, _From, #state{downloader = #downloader{}, pending = undefined}=State) ->
    send_data(Data, State#state.downloader),
    {reply, ok, State};

handle_upload_data_call(Data, From, #state{downloader = undefined, pending = undefined}=State) ->
    {noreply, State#state{pending = #pending_data{data = Data, from = From}}};

handle_upload_data_call(_Data, _From, #state{downloader = undefined, pending = #pending_data{}}=State) ->
    {reply, {error, data_pending}, State}.

-spec send_data(data(), #downloader{}) -> _.
send_data(Data, #downloader{pid = Pid, tag = Tag}) ->
    Pid ! {Tag, Data}.
