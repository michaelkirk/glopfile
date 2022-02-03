-module(api_SUITE).
-behaviour(ct_suite).

-include("sendfile_test.hrl").
-import(sendfile_test, [http/3]).

-define(NONEXISTENT_API_PATH, "/api/v1/test_nonexistent_api").
-define(PROVISION_UPLOAD_PATH, "/api/v1/files").
-define(UPLOAD_PATH, "/api/v1/upload").
-define(DOWNLOAD_PATH, "/api/v1/download").
-define(DOWNLOAD_CONTENT_SUBPATH, "/content").

-define(NONEXISTENT_ID, "1234").
-define(INVALID_ID, "test_invalid_id").

-define(METADATA, <<"test_metadata">>).
-define(CONTENT, <<"test_content">>).

all() -> [{group, api}].

groups() -> [{api, [parallel], ct_helper:all(?MODULE)}].

init_per_group(_Name, Config) ->
    sendfile_test:start(Config).

end_per_group(_Name, Config) ->
    sendfile_test:stop(Config).

nonexistent_api(Config) ->
    doc("Requests to non-existent APIs return a 404."),
    #http_response{status = 404} = http(get, Config, #http_request{path = ?NONEXISTENT_API_PATH}),
    ok.

upload_no_metadata(Config) ->
    doc("Upload requests with no metadata return a 400."),
    #http_response{status = 400} = http(post, Config, #http_request{path = ?PROVISION_UPLOAD_PATH, body = #form_body{}}),
    ok.

invalid_method_upload(Config) ->
    doc("Non-POST upload requests return a 405."),
    #http_response{status = 405} = http(get, Config, #http_request{path = ?PROVISION_UPLOAD_PATH}),
    ok.

upload(Config) ->
    doc("Valid upload requests are accepted and return URLs under the paths assumed by other tests."),

    #http_response{
       status = 200,
       body = #json_body{object = #{<<"upload_url">> := <<?UPLOAD_PATH "/", _/binary>>,
                                    <<"download_url">> := <<?DOWNLOAD_PATH "/", _/binary>>}}
      } = http(post, Config, #http_request{path = ?PROVISION_UPLOAD_PATH, body = #form_body{data = #{encrypted_metadata => ?METADATA}}}),
    ok.

nonexistent_download(Config) ->
    doc("Download requests to non-existent IDs return a 404."),
    #http_response{status = 404} = http(get, Config, #http_request{path = ?DOWNLOAD_PATH "/" ?NONEXISTENT_ID}),
    ok.

invalid_download_id(Config) ->
    doc("Download requests to invalid IDs return a 400."),
    #http_response{status = 400} = http(get, Config, #http_request{path = ?DOWNLOAD_PATH "/" ?INVALID_ID}),
    ok.

nonexistent_download_content(Config) ->
    doc("Download content requests to non-existent IDs return a 404."),
    #http_response{status = 404} =
        http(get, Config, #http_request{path = ?DOWNLOAD_PATH "/" ?NONEXISTENT_ID ?DOWNLOAD_CONTENT_SUBPATH}),
    ok.

invalid_download_content_id(Config) ->
    doc("Download content requests to invalid IDs return a 400."),
    #http_response{status = 400} =
        http(get, Config, #http_request{path = ?DOWNLOAD_PATH "/" ?INVALID_ID ? DOWNLOAD_CONTENT_SUBPATH}),
    ok.

invalid_method_download(Config) ->
    doc("Non-GET download requests return a 405."),

    #http_response{
       status = 200,
       body = #json_body{object = #{<<"download_url">> := ProvisionDownloadUrl}}
      } = http(post, Config, #http_request{path = ?PROVISION_UPLOAD_PATH, body = #form_body{data = #{encrypted_metadata => ?METADATA}}}),

    #http_response{status = 405} = http(post, Config, #http_request{path = ProvisionDownloadUrl, body = <<>>}),

    ok.

download(Config) ->
    doc("Valid download requests are accepted and return the correct metadata."),

    #http_response{
       status = 200,
       body = #json_body{object = #{<<"download_url">> := ProvisionDownloadUrl}}
      } = http(post, Config, #http_request{path = ?PROVISION_UPLOAD_PATH, body = #form_body{data = #{encrypted_metadata => ?METADATA}}}),

    #http_response{
       status = 200,
       body = #json_body{object = #{<<"meta">> := ?METADATA}}
      } = http(get, Config, #http_request{path = ProvisionDownloadUrl}),

    ok.

nonexistent_upload(Config) ->
    doc("Upload requests to non-existent IDs return a 404."),
    #http_response{status = 404} = http(post, Config, #http_request{path = ?UPLOAD_PATH "/" ?NONEXISTENT_ID, body = ?CONTENT}),
    ok.

invalid_upload_id(Config) ->
    doc("Upload requests to invalid IDs return a 400."),
    #http_response{status = 400} = http(post, Config, #http_request{path = ?UPLOAD_PATH "/" ?INVALID_ID, body = ?CONTENT}),
    ok.

transfer(Config) ->
   doc("A transfer works between an uploader and downloader and the downloader receives the correct data."),

    #http_response{
       status = 200,
       body = #json_body{object = #{<<"download_url">> := ProvisionDownloadUrl, <<"upload_url">> := UploadContentUrl}}
      } = http(post, Config, #http_request{path = ?PROVISION_UPLOAD_PATH, body = #form_body{data = #{encrypted_metadata => ?METADATA}}}),

    #http_response{
       status = 200,
       body = #json_body{object = #{<<"meta">> := ?METADATA, <<"encrypted_content_url">> := DownloadContentUrl}}
      } = http(get, Config, #http_request{path = ProvisionDownloadUrl}),

    Self = self(),
    spawn_link(fun () -> Self ! http(get, Config, #http_request{path = DownloadContentUrl}) end),

    #http_response{
       status = 200,
       body = <<>>
      } = http(post, Config, #http_request{path = UploadContentUrl, body = ?CONTENT}),

    #http_response{
       status = 200,
       body = ?CONTENT
      } = receive #http_response{}=DownloadContentResp -> DownloadContentResp end,

    ok.
