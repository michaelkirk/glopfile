-import(ct_helper, [config/2, doc/1]).
-compile([export_all, nowarn_export_all]).

-record(json_body, {object = #{} :: jsone:json_object()}).
-type json_body() :: #json_body{}.

-record(form_body, {data = #{} :: #{iodata() | atom() => iodata() | atom()}}).
-type form_body() :: #form_body{}.

-type body() :: iodata() | json_body() | form_body().

-record(http_request,
        {path :: iodata(),
         headers = #{} :: #{iodata() | atom() => iodata() | atom()},
         content_type = undefined :: iodata() | undefined,
         body = undefined :: body() | undefined}).
-type http_request() :: #http_request{}.

-record(http_response,
        {status :: 100..600,
         headers :: #{binary() => binary()},
         body :: binary() | json_body()}).
-type http_response() :: #http_response{}.
