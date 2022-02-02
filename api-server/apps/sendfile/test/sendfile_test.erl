-module(sendfile_test).

-include("sendfile_test.hrl").

-export([start/1, stop/1, http/3]).

-define(HTTPC_PROFILE, sendfile_test).
-define(HTTPC_OPTS, [{max_pipeline_length, 0}, {max_keep_alive_length, 0}, {max_sessions, 1000}]).

start(Config) ->
    application:set_env(sendfile_app:application(), port, 0),
    sendfile_app:start(),
    application:ensure_all_started(inets),
    application:ensure_all_started(ct_helper),
    {ok, _Pid} = inets:start(httpc, [{profile, ?HTTPC_PROFILE}]),
    ok = httpc:set_options(?HTTPC_OPTS, ?HTTPC_PROFILE),
    [{port, sendfile_server:listen_port()} | Config].

stop(Config) ->
    inets:stop(httpc, ?HTTPC_PROFILE),
    sendfile_app:stop(),
    Config.

-spec http(httpc:method(), ct_suite:ct_config(), http_request()) -> http_response().
http(Method, Config, Request) ->
    http_response(httpc:request(Method, http_request(Config, Request), [], [], ?HTTPC_PROFILE)).

-spec http_request(ct_suite:ct_config(), http_request()) -> http_response().
%% no body or content_type is set
http_request(Config, #http_request{content_type = undefined, body = undefined}=Request) ->
    PortBin = integer_to_binary(proplists:get_value(port, Config)),
    PathBin = iolist_to_binary(Request#http_request.path),
    UrlBin = <<"http://localhost:", PortBin/binary, PathBin/binary>>,
    HeadersList = maps:to_list(Request#http_request.headers),
    Headers = lists:keymap(fun iolist_to_string/1, 1, HeadersList),
    {binary_to_list(UrlBin), Headers};

%% undefined content_type, and body is json
http_request(Config, #http_request{content_type = undefined, body = #json_body{}}=Request) ->
    %% set content_type for json and recurse
    http_request(Config, Request#http_request{content_type = <<"application/json">>});

%% undefined content_type, and body is form data
http_request(Config, #http_request{content_type = undefined, body = #form_body{}}=Request) ->
    %% set content_type for form data and recurse
    http_request(Config, Request#http_request{content_type = <<"application/x-www-form-urlencoded">>});

%% undefined content_type, and body is none of the above
http_request(Config, #http_request{content_type = undefined}=Request) ->
    %% set content_type to the default and recurse
    http_request(Config, Request#http_request{content_type = <<"application/octet-stream">>});

%% undefined body
http_request(Config, #http_request{body = undefined}=Request) ->
    %% set the body to an empty binary and recurse
    http_request(Config, Request#http_request{body = <<>>});

%% body is json
http_request(Config, #http_request{body = #json_body{object = BodyMap}}=Request) ->
    %% set the body to the encoded binary and recurse
    http_request(Config, Request#http_request{body = jsone:encode(BodyMap)});

%% body is form data
http_request(Config, #http_request{body = #form_body{data = FormDataMap}}=Request) ->
    %% set the body to the encoded binary and recurse
    UriQuery = lists:foldl(fun (KeyPos, FormDataList) ->
                                   lists:keymap(fun maybe_atom_to_binary/1, KeyPos, FormDataList)
                           end, maps:to_list(FormDataMap), [1, 2]),
    http_request(Config, Request#http_request{body = uri_string:compose_query(UriQuery)});

%% body is none of the above
http_request(Config, #http_request{}=Request) ->
    #http_request{path = InPath, headers = InHeaders, content_type = ContentType, body = Body} = Request,
    {Url, Headers} = http_request(Config, #http_request{path = InPath, headers = InHeaders}),
    {Url, Headers, iolist_to_string(ContentType), Body}.

http_response({ok, {{_HttpVersion, StatusCode, _StatusText}, HeadersList, BodyIoData}}) ->
    HeadersMap = lists:foldl(fun ({Key, Value}, Acc) -> Acc#{(iolist_to_binary(string:lowercase(Key))) => iolist_to_binary(Value)} end, #{}, HeadersList),
    BodyLen = iolist_size(BodyIoData),
    Body = case HeadersMap of
               #{<<"content-type">> := <<"application/json">>} when BodyLen > 0 ->
                   #json_body{object = jsone:decode(iolist_to_binary(BodyIoData))};
               _ ->
                   iolist_to_binary(BodyIoData)
           end,
    #http_response{status = StatusCode, headers = HeadersMap, body = Body}.

iolist_to_string(IoList) ->
    binary_to_list(iolist_to_binary(IoList)).

maybe_atom_to_binary(Atom) when is_atom(Atom) ->
    atom_to_binary(Atom);
maybe_atom_to_binary(Other) ->
    Other.
