[./sendfile] is TypeScript library for sending and receiving files.
[./sendfile-ui] is a React-powered user interface on top of the sendfile library.

## Development


### Install dependencies and compile the TypeScript library

    cargo install wasm-pack
    wasm-pack build rust
    (cd sendfile && yarn install && yarn tsc)
    (cd sendfile-ui && yarn install)

### Link to local sendfile

While developing, you probably want to see subsequent changes in sendfile
reflected in sendfile-ui. After completing the steps above, you can replace
the static copy of sendfile with a symlink like this:

    rm -fr sendfile-ui/node_modules/sendfile
    ln -s -r sendfile sendfile-ui/node_modules

### Start the app server

    cd sendfile-ui && yarn start
