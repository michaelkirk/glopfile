-module(sendfile_api_handler).
-behaviour(cowboy_websocket).

-include_lib("kernel/include/logger.hrl").

%% API
-export([websocket_stop/1, websocket_send/2]).

%% cowboy_handler callbacks
-export([init/2]).

%% cowboy_loop callbacks
-export([info/3]).

%% ignore "unused exports" here since we can't declare -behaviour(cowboy_loop) above. it technically conflicts with
%% cowboy_websocket, but is fine since both are derived from cowboy_handler, and it seems there's no way to ignore the
%% warning.
-ignore_xref([info/3]).

%% cowboy_websocket callbacks
-export([websocket_init/1, websocket_handle/2, websocket_info/2]).

-record(websocket_state,
        {pid :: pid()}).

-record(content_stream_state,
        {id      :: binary(),
         tag     :: reference(),
         session :: pid()}).
-type content_stream_state() :: #content_stream_state{}.

-record(state,
        {stream = undefined :: content_stream_state() | undefined}).

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
        {ok, RespBody}               -> {ok, cowboy_req:reply(200, RespHeaders, RespBody, Req), #state{}};
        {ok, RespBody, NewReq}       -> {ok, cowboy_req:reply(200, RespHeaders, RespBody, NewReq), #state{}};
        {stream, StreamArg}          -> {cowboy_loop, cowboy_req:stream_reply(200, StreamRespHeaders, Req), #state{stream = StreamArg}};
        {websocket, WsArg}           -> {cowboy_websocket, Req, WsArg, sendfile_websocket:websocket_opts()};
        not_found                    -> {ok, cowboy_req:reply(404, RespHeaders, <<>>, Req), #state{}};
        invalid_method               -> {ok, cowboy_req:reply(405, RespHeaders, <<>>, Req), #state{}};
        {conflict, RespBody}         -> {ok, cowboy_req:reply(409, RespHeaders, RespBody, Req), #state{}};
        {conflict, RespBody, NewReq} -> {ok, cowboy_req:reply(409, RespHeaders, RespBody, NewReq), #state{}};
        {invalid, InvalidReason} ->
            ?LOG_INFO("invalid ~s ~s request: ~p", [Method, Path, InvalidReason]),
            {ok, cowboy_req:reply(400, RespHeaders, <<>>, Req), #state{}}
    end.

%%
%% cowboy_loop callbacks
%%

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
        {websocket, WebsocketInitArg :: sendfile_websocket:start_opts()} |
        not_found |
        invalid_method |
        {conflict, ResponseBody :: binary()} |
        {conflict, ResponseBody :: binary(), NewRequest :: cowboy:req()} |
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

handle_request(#{path := <<"/api/v1/download/", SubPath/binary>>}=Req, _QueryString) ->
    handle_request_with_id(SubPath, fun(Id, Path) -> handle_download(Id, Path, Req) end);

handle_request(#{path := <<"/api/v1/upload/", SubPath/binary>>}=Req, _QueryString) ->
    handle_request_with_id(SubPath, fun(Id, Path) -> handle_upload(Id, Path, Req) end);

handle_request(#{path := <<"/api/v1/content/", SubPath/binary>>}=Req, _QueryString) ->
    handle_request_with_id(SubPath, fun(Id, Path) -> handle_content(Id, Path, Req) end);

handle_request(_Request, _QueryString) ->
    not_found.

-spec handle_request_with_id(Path :: binary(), Fun) -> Res when
      Fun :: fun((DecodedId :: binary(), SubPath :: binary()) -> Res).
handle_request_with_id(<<Path/binary>>, Fun) ->
    case decode_id(Path) of
        {ok, Id, SubPath} ->
            Fun(Id, SubPath);
        {error, Err} ->
            ?LOG_WARNING("invalid id in download url: ~s: ~p", [Path, Err]),
            {invalid, invalid_id}
    end.

-spec handle_download(Id :: binary(), Path :: binary(), Req :: cowboy_req:req()) -> handle_request_result().
%% GET to top-level endpoint
handle_download(<<Id/binary>>, <<>>, #{method := <<"GET">>}) ->
    EncodedId = encode_id(Id),
    case sendfile_session:metadata(Id) of
        {ok, Metadata} ->
            ResponseMap =
                #{meta => Metadata,
                  encrypted_content_url => <<"/api/v1/download/", EncodedId/binary, "/content">>},
            {ok, jsone:encode(ResponseMap)};
        {error, not_found} ->
            not_found
    end;

%% non-GET to top-level endpoint
handle_download(<<_Id/binary>>, <<>>, _Req) ->
    invalid_method;

%% GET to content endpoint
handle_download(<<Id/binary>>, <<"content">>, #{method := <<"GET">>}=Req) ->
    case content_stream_init(Id, Req) of
        {ok, State} -> {stream, State};
        {error, not_found} -> not_found
    end;

%% non-GET to content endpoint
handle_download(<<_Id/binary>>, <<"content">>, _Req) ->
    invalid_method;

%% websocket endpoint
handle_download(<<Id/binary>>, <<"ws">>, _Req) ->
    case sendfile_session_table:get(Id) of
        {ok, SessionPid} ->
            {websocket, sendfile_websocket:start_opts(Id, SessionPid, download)};
        {error, not_found} ->
            not_found
    end;

%% non-existent endpoint
handle_download(_Id, _Path, _Req) ->
    not_found.

-spec handle_upload(Id :: binary(), Path :: binary(), cowboy:req()) -> handle_request_result().
%% POST to top-level endpoint
handle_upload(<<Id/binary>>, <<>>, #{method := <<"POST">>}=Req) ->
    Tag = make_ref(),
    case cowboy_req:parse_header(<<"range">>, Req, {bytes, [{0, infinity}]}) of
        {bytes, [{ReqPosition, infinity}]} ->
            case sendfile_session:start_upload(Id, Tag, ReqPosition) of
                {ok, Pid} ->
                    upload(Pid, Req);
                {position, NewPosition} ->
                    {conflict, jsone:encode(upload_conflict_response(NewPosition))};
                {error, not_found} ->
                    not_found
            end;
        _ ->
            {invalid, invalid_range}
    end;

%% non-POST to top-level endpoint
handle_upload(<<_Id/binary>>, <<>>, _Req) ->
    invalid_method;

%% websocket endpoint
handle_upload(<<Id/binary>>, <<"ws">>, _Req) ->
    case sendfile_session_table:get(Id) of
        {ok, SessionPid} ->
            {websocket, sendfile_websocket:start_opts(Id, SessionPid, upload)};
        {error, not_found} ->
            not_found
    end;

%% non-existent endpoint
handle_upload(_Id, _Path, _Req) ->
    not_found.

-spec handle_content(Id :: binary(), Path :: binary(), cowboy_req:req()) -> handle_request_result().
%% GET to top-level endpoint
handle_content(<<Id/binary>>, <<>>, #{method := <<"GET">>}=Req) ->
    case content_stream_init(Id, Req) of
        {ok, State} -> {stream, State};
        {error, not_found} -> not_found
    end;

%% non-GET to top-level endpoint
handle_content(<<_Id/binary>>, <<>>, _Req) ->
    invalid_method;

%% non-existent endpoint
handle_content(_Id, _Path, _Req) ->
    not_found.

%%
%% upload functions
%%

-spec upload(_, _) -> handle_request_result().
upload(Pid, Req) ->
    {Status, Data, BodyReadReq} = cowboy_req:read_body(Req),
    UploadData = case Status of
                     ok -> {data, Data};
                     more -> {more, Data}
                 end,
    case sendfile_session:upload_data(Pid, UploadData) of
        ok -> case Status of
                  ok   -> {ok, <<>>, BodyReadReq};
                  more -> upload(Pid, BodyReadReq)
              end;
        {position, NewPosition} ->
            {conflict, jsone:encode(upload_conflict_response(NewPosition)), BodyReadReq};
        {error, connection_replaced} ->
            {ok, <<>>, BodyReadReq}
    end.

-spec upload_conflict_response(Position :: non_neg_integer()) -> jsone:json_object().
upload_conflict_response(Position) ->
    #{position => Position}.

%%
%% content stream functions
%%

-spec content_stream_init(binary(), cowboy_req:req()) -> {ok, content_stream_state()} | {error, not_found}.
content_stream_init(<<Id/binary>>, Req) ->
    Tag = make_ref(),
    case cowboy_req:parse_header(<<"range">>, Req, {bytes, [{0, infinity}]}) of
        {bytes, [{ReqPosition, infinity}]} ->
            case sendfile_session:start_download(Id, Tag, ReqPosition) of
                {ok, Pid} ->
                    monitor(process, Pid),
                    {ok, #content_stream_state{id = Id, tag = Tag, session = Pid}};
                {error, not_found} ->
                    {error, not_found}
            end;
        _ ->
            {invalid, invalid_range}
    end.

-spec content_stream_info(Msg :: any(), cowboy_req:req(), content_stream_state()) -> {ok | stop, cowboy_req:req(), content_stream_state()}.
content_stream_info({Tag, {more, Data}}, Req, #content_stream_state{tag = Tag}=State) ->
    ok = cowboy_req:stream_body(Data, nofin, Req),
    {ok, Req, State};
content_stream_info({Tag, {data, Data}}, Req, #content_stream_state{tag = Tag}=State) ->
    #content_stream_state{id = Id, session = Pid} = State,
    ?LOG_DEBUG("download ~p from ~p finished", [Id, Pid]),
    ok = cowboy_req:stream_body(Data, fin, Req),
    {stop, Req, State};
content_stream_info({Tag, {error, connection_replaced}}, Req, #content_stream_state{tag = Tag}=State) ->
    #content_stream_state{id = Id, session = Pid} = State,
    ?LOG_DEBUG("download ~p from ~p replaced by new connection", [Id, Pid]),
    %% TODO cowboy always ends the chunked encoding gracefully, but we would ideally close ungracefully so clients don't
    %% erroneously think the download is finished.
    {stop, Req, State};
content_stream_info({'DOWN', _Mon, process, Pid, Info}, _Req, #content_stream_state{session = Pid}=State) ->
    #content_stream_state{id = Id} = State,
    ?LOG_WARNING("download ~p from ~p session died", [Id, Pid]),
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

-spec decode_id(Path :: binary()) -> {ok, Id :: binary(), SubPath :: binary()} | {error, _}.
decode_id(<<Path/binary>>) ->
    case binary:split(Path, <<"/">>) of
        [EncodedId, RestPath] -> ok;
        [EncodedId] -> RestPath = <<>>;
        [] -> {EncodedId, RestPath} = {<<>>, <<>>}
    end,
    try base64:decode(<< (decode_id_to_base64_ch(Ch)) || <<Ch>> <= EncodedId >>) of
        <<>> -> {error, empty_id};
        Id   -> {ok, Id, RestPath}
    catch
        _:Err -> {error, Err}
    end.

-spec decode_id_to_base64_ch(byte()) -> binary().
decode_id_to_base64_ch($-) -> <<$+>>;
decode_id_to_base64_ch($_) -> <<$/>>;
decode_id_to_base64_ch($~) -> <<$=>>;
decode_id_to_base64_ch(Ch) -> <<Ch>>.
