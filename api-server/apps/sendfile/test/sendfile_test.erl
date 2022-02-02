-module(sendfile_test).

-include("sendfile_test.hrl").

-export([start/1, stop/1, http_get/2, http_post/2]).

start(Config) ->
    application:ensure_all_started(gun),
    application:ensure_all_started(ct_helper),
    application:set_env(sendfile_app:application(), port, 0),
    sendfile_app:start(),
    [{port, sendfile_server:listen_port()} | Config].

stop(Config) ->
    sendfile_app:stop(),
    Config.

http_get(Config, Request) ->
    http_response(httpc:request(http_request(Config, Request))).

http_post(Config, Request) ->
    http_response(httpc:request(post, http_request(Config, Request), [], [])).

http_request(Config, #http_request{}=Request) ->
    PortBin = integer_to_binary(proplists:get_value(port, Config)),
    PathBin = iolist_to_binary(Request#http_request.path),
    UrlBin = <<"http://localhost:", PortBin/binary, PathBin/binary>>,
    HeadersList = maps:to_list(Request#http_request.headers),
    Headers = lists:keymap(fun iolist_to_string/1, 1, HeadersList),
    {binary_to_list(UrlBin), Headers};

http_request(Config, #http_request_with_body{}=Request) ->
    #http_request_with_body{path = InPath, headers = InHeaders, content_type = ContentType, body = Body} = Request,
    {Url, Headers} = http_request(Config, #http_request{path = InPath, headers = InHeaders}),
    {Url, Headers, iolist_to_string(ContentType), Body}.

http_response({ok, {{_HttpVersion, StatusCode, _StatusText}, HeadersList, Body}}) ->
    HeadersMap = lists:foldl(fun ({Key, Value}, Acc) -> Acc#{(iolist_to_binary(Key)) => iolist_to_binary(Value)} end, #{}, HeadersList),
    #http_response{status = StatusCode, headers = HeadersMap, body = iolist_to_binary(Body)}.

iolist_to_string(IoList) ->
    binary_to_list(iolist_to_binary(IoList)).
