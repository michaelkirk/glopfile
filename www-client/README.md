[./glopfile] is TypeScript library for sending and receiving files.
[./glopfile-ui] is a React-powered user interface on top of the glopfile library.

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
