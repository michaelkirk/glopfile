[./sendfile] is typescript library for sending and receiving files.
[./senfile-ui] is a react powered user interface on top of the sendfile library.

## Development


### Install dependencies

    cd sendfile-ui
    yarn install


### Link to local sendfile

While developing, you probably want to see subsequent changes in sendfile
reflected in sendfile-ui. So after `yarn install`, replace the static copy of
sendfile with a symlink, like this:

    rm -fr node_modules/sendfile
    ln -s ../../sendfile node_modules/sendfile

### start the app server

    yarn start
