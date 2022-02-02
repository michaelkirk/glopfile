-import(ct_helper, [config/2, doc/1]).
-compile([export_all, nowarn_export_all]).

-record(http_request,
        {path :: iodata(),
         headers = #{} :: #{iodata() | atom() => iodata() | atom()}}).

-record(http_request_with_body,
        {path :: iodata(),
         headers = #{} :: #{iodata() | atom() => iodata() | atom()},
         content_type = <<"application/octet-stream">> :: iodata(),
         body = <<>> :: iodata()}).

-record(http_response,
        {status :: 200..600,
         headers :: #{binary() => binary()},
         body :: binary()}).
