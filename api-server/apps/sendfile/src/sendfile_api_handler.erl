-module(sendfile_api_handler).
-behaviour(cowboy_websocket).

-include_lib("kernel/include/logger.hrl").

%% API
-export([websocket_stop/1, websocket_send/2]).

%% cowboy_handler callbacks
-export([init/2, info/3]).

%% cowboy_websocket callbacks
-export([websocket_init/1, websocket_handle/2, websocket_info/2]).

-record(websocket_state,
        {pid :: pid()}).

-record(content_stream_state,
        {id      :: binary(),
         tag     :: reference(),
         session :: pid()}).

-record(state,
        {stream = undefined :: #content_stream_state{} | undefined}).

-record(websocket_stop_cast, {}).

-record(websocket_send_cast,
        {messages :: cowboy_websocket:commands()}).

%%
%% API
%%

-spec websocket_stop(pid()) -> ok.
websocket_stop(Pid) ->
    Pid ! #websocket_stop_cast{},
    ok.

-spec websocket_send(pid(), cowboy_websocket:commands()) -> ok.
websocket_send(Pid, Messages) ->
    Pid ! #websocket_send_cast{messages = Messages},
    ok.


%%
%% cowboy_handler callbacks
%%

init(Req, _InitialState) ->
    RespHeaders =
        #{<<"content-type">>  => <<"application/json">>,
          <<"cache-control">> => <<"no-cache, no-store">>,
          <<"expires">>       => <<"Fri, 1 Jan 1999 12:00:00 AM GMT">>,
          <<"pragma">>        => <<"no-cache">>,
          <<"access-control-allow-origin">> => <<"*">>},
    StreamRespHeaders = RespHeaders#{<<"content-type">> => <<"application/octet-stream">>},
    QueryString = maps:from_list(cowboy_req:parse_qs(Req)),
    #{method := Method, path := Path} = Req,
    case handle_request(Req, QueryString) of
        {ok, RespBody}           -> {ok, cowboy_req:reply(200, RespHeaders, RespBody, Req), #state{}};
        {ok, RespBody, NewReq}   -> {ok, cowboy_req:reply(200, RespHeaders, RespBody, NewReq), #state{}};
        {stream, StreamArg}      -> {cowboy_loop, cowboy_req:stream_reply(200, StreamRespHeaders, Req), #state{stream = StreamArg}};
        {websocket, WsArg}       -> {cowboy_websocket, Req, WsArg, sendfile_websocket:websocket_opts()};
        not_found                -> {ok, cowboy_req:reply(404, RespHeaders, <<>>, Req), #state{}};
        {invalid, InvalidReason} ->
            ?LOG_INFO("invalid ~s ~s request: ~p", [Method, Path, InvalidReason]),
            {ok, cowboy_req:reply(400, #{}, <<>>, Req), #state{}}
    end.

info(Msg, Req, #state{stream = #content_stream_state{}}=State) ->
    {Status, NewReq, NewStreamState} = content_stream_info(Msg, Req, State#state.stream),
    {Status, NewReq, State#state{stream = NewStreamState}}.

%%
%% cowboy_websocket callbacks
%%

websocket_init(Arg) ->
    {ok, Pid} = sendfile_websocket:start_link(self(), Arg),
    {ok, #websocket_state{pid = Pid}}.

websocket_handle(Frame, State) ->
    {ok, Replies} = sendfile_websocket:recv(State#websocket_state.pid, Frame),
    {Replies, State}.

websocket_info(#websocket_stop_cast{}, State) ->
    {stop, State};

websocket_info(#websocket_send_cast{messages = Messages}, State) ->
    {Messages, State};

websocket_info(Message, State) ->
    ?LOG_WARNING("unknown message: ~p", [Message]),
    {ok, State}.

%%
%% request handler functions
%%

-type handle_request_result() ::
        {ok, ResponseBody :: binary()} |
        {ok, ResponseBody :: binary(), NewRequest :: cowboy:req()} |
        {stream, StreamArg :: any()} |
        {websocket, WebsocketInitArg :: any()} |
        not_found |
        {invalid, Err :: any()}.

-spec handle_request(Req :: cowboy:req(), QueryString :: #{binary() => binary()}) -> handle_request_result().
handle_request(#{path := <<"/api/v1/files">>, method := <<"POST">>}=Req, _QueryString) ->
    {ok, BodyKVList, BodyReadReq} = cowboy_req:read_urlencoded_body(Req),
    BodyKVMap = maps:from_list(BodyKVList),
    case BodyKVMap of
        #{<<"encrypted_metadata">> := Metadata} ->
            {ok, _Pid, Id} = sendfile_session:start(Metadata),
            EncodedId = encode_id(Id),
            ResponseMap =
                #{upload_url   => <<"/api/v1/upload/", EncodedId/binary>>,
                  download_url => <<"/api/v1/download/", EncodedId/binary>>},
            {ok, jsone:encode(ResponseMap), BodyReadReq};
        _ ->
            {invalid, metadata_missing}
    end;

handle_request(#{path := <<"/api/v1/download/", EncodedId/binary>>, method := <<"GET">>}, _QueryString) ->
    handle_request_with_id(EncodedId, fun(Id) -> handle_download(Id) end);

handle_request(#{path := <<"/api/v1/upload/", EncodedId/binary>>, method := <<"POST">>}=Req, _QueryString) ->
    handle_request_with_id(EncodedId, fun(Id) -> handle_upload(Id, Req) end);

handle_request(#{path := <<"/api/v1/content/", EncodedId/binary>>, method := <<"GET">>}, _QueryString) ->
    handle_request_with_id(EncodedId, fun(Id) -> handle_content(Id) end);

handle_request(#{path := <<"/api/v1/ws">>}, _QueryString) ->
    {websocket, sendfile_websocket:start_opts()};

handle_request(_Request, _QueryString) ->
    not_found.

-spec handle_request_with_id(Id :: binary(), Fun) -> Res when
      Fun :: fun((EncodedId :: binary()) -> Res).
handle_request_with_id(<<EncodedId/binary>>, Fun) ->
    case decode_id(EncodedId) of
        {ok, Id} ->
            Fun(Id);
        {error, Err} ->
            ?LOG_WARNING("invalid id in download url: ~s: ~p", [EncodedId, Err]),
            {invalid, invalid_id}
    end.

-spec handle_download(Id :: binary()) -> handle_request_result().
handle_download(<<Id/binary>>) ->
    EncodedId = encode_id(Id),
    case sendfile_session:metadata(Id) of
        {ok, Metadata} ->
            ResponseMap =
                #{meta => Metadata,
                  encrypted_content_url => <<"/api/v1/content/", EncodedId/binary>>},
            {ok, jsone:encode(ResponseMap)};
        {error, not_found} ->
            not_found
    end.

-spec handle_upload(Id :: binary(), cowboy:req()) -> handle_request_result().
handle_upload(<<Id/binary>>, Req) ->
    Tag = make_ref(),
    case sendfile_session:start_upload(Id, Tag) of
        {ok, Pid} ->
            UploadedReq = upload(Pid, Req),
            {ok, <<>>, UploadedReq};
        {error, not_found} ->
            not_found
    end.

-spec handle_content(Id :: binary()) -> handle_request_result().
handle_content(<<Id/binary>>) ->
    case content_stream_init(Id) of
        {ok, State} -> {stream, State};
        {error, not_found} -> not_found
    end.

%%
%% upload functions
%%

-spec upload(_, _) -> cowboy_req:req().
upload(Pid, Req) ->
    {Status, Data, BodyReadReq} = cowboy_req:read_body(Req),
    UploadData = case Status of
                     ok -> {data, Data};
                     more -> {more, Data}
                 end,
    ok = sendfile_session:upload_data(Pid, UploadData),
    case Status of
        ok   -> BodyReadReq;
        more -> upload(Pid, BodyReadReq)
    end.

%%
%% content stream functions
%%

-spec content_stream_init(binary()) -> {ok, #content_stream_state{}} | {error, not_found}.
content_stream_init(<<Id/binary>>) ->
    Tag = make_ref(),
    case sendfile_session:start_download(Id, Tag) of
        {ok, Pid} ->
            monitor(process, Pid),
            {ok, #content_stream_state{id = Id, tag = Tag, session = Pid}};
        {error, not_found} ->
            {error, not_found}
    end.

-spec content_stream_info(Msg :: any(), cowboy_req:req(), #content_stream_state{}) -> {ok | stop, cowboy_req:req(), #content_stream_state{}}.
content_stream_info({Tag, {more, Data}}, Req, #content_stream_state{tag = Tag}=State) ->
    NewReq = cowboy_req:stream_body(Data, nofin, Req),
    {ok, NewReq, State};
content_stream_info({Tag, {data, Data}}, Req, #content_stream_state{tag = Tag}=State) ->
    NewReq = cowboy_req:stream_body(Data, fin, Req),
    {stop, NewReq, State};
content_stream_info({'DOWN', _Mon, process, Pid, Info}, _Req, #content_stream_state{session = Pid}) ->
    exit({session_died, Info});
content_stream_info(Msg, Req, State) ->
    ?LOG_WARNING("unknown message: ~p", [Msg]),
    {ok, Req, State}.

%%
%% ID encoding/decoding
%%

-spec encode_id(Id :: binary()) -> EncodedId :: binary().
encode_id(<<Id/binary>>) ->
    << (encode_id_from_base64_ch(Ch)) || <<Ch>> <= base64:encode(Id) >>.

-spec encode_id_from_base64_ch(byte()) -> binary().
encode_id_from_base64_ch($+) -> <<$->>;
encode_id_from_base64_ch($/) -> <<$_>>;
encode_id_from_base64_ch($=) -> <<$~>>;
encode_id_from_base64_ch(Ch) -> <<Ch>>.

-spec decode_id(EncodedId :: binary()) -> {ok, Id :: binary()} | {error, _}.
decode_id(<<EncodedId/binary>>) ->
    try base64:decode(<< (decode_id_to_base64_ch(Ch)) || <<Ch>> <= EncodedId >>) of
        Id -> {ok, Id}
    catch
        _:Err -> {error, Err}
    end.

-spec decode_id_to_base64_ch(byte()) -> binary().
decode_id_to_base64_ch($-) -> <<$+>>;
decode_id_to_base64_ch($_) -> <<$/>>;
decode_id_to_base64_ch($~) -> <<$=>>;
decode_id_to_base64_ch(Ch) -> <<Ch>>.
