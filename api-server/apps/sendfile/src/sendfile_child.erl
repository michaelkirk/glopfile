-module(sendfile_child).

-callback child_spec() -> supervisor:child_spec().
