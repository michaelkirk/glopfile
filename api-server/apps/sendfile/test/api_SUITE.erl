-module(api_SUITE).
-behaviour(ct_suite).

-include("sendfile_test.hrl").
-import(sendfile_test, [http_post/2]).

all() -> [{group, api}].

groups() -> [{api, [parallel], ct_helper:all(?MODULE)}].

init_per_group(_Name, Config) ->
    sendfile_test:start(Config).

end_per_group(_Name, Config) ->
    sendfile_test:stop(Config).

upload_no_metadata(Config) ->
    doc("Upload requests with no metadata are denied"),
    #http_response{status = 400} = http_post(Config, #http_request_with_body{path = "/api/v1/files"}),
    ok.
