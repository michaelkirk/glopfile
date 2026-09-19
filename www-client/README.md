[./glopfile] is TypeScript library for sending and receiving files.
[./glopfile-ui] is a React-powered user interface on top of the glopfile library.

## ⚠️ Experimental — do not trust this

An experiment, not something to rely on. It has not been audited, parts of it
are LLM generated, and the protocol and formats can change without notice.

The server does not have access to your file's contents, by design: the file is
encrypted in the browser before upload, and the key lives in the download
link's fragment (`#cipher_key=...`), which browsers never send to the server.
That is the design, not an audited guarantee.

## Development


### Install dependencies and compile the TypeScript library

    cargo install wasm-pack
    wasm-pack build rust
    (cd glopfile && yarn install && yarn tsc)
    (cd glopfile-ui && yarn install)

### Link to local glopfile

While developing, you probably want to see subsequent changes in glopfile
reflected in glopfile-ui. After completing the steps above, you can replace
the static copy of glopfile with a symlink like this:

    rm -fr glopfile-ui/node_modules/glopfile
    ln -s -r glopfile glopfile-ui/node_modules

### Start the app server

    cd glopfile-ui && yarn start
