# Sendfile iOS

Send a file with your iOS device!

## ⚠️ Experimental — do not trust this

An experiment, not something to rely on. It has not been audited, parts of it
are LLM generated, and the protocol and formats can change without notice.

The server does not have access to your file's contents, by design: the file is
encrypted on the device before upload, and the key is only ever carried in the
download link's fragment. That is the design, not an audited guarantee.

## Building

### Prerequisites:
- Rust 1.56.0 or newer
- cmake (in a system-wide PATH directory)

Make sure Rust version 1.56.0 or newer is installed with the desired toolchain targets. The Rust target names for the
supported build targets are:

* iPhone on x86_64: `x86_64-apple-ios`
* iPhone on 64-bit ARM: `aarch64-apple-ios`
* iPhone Simulator on 64-bit ARM (e.g. M1 processor): `aarch64-apple-ios-sim`

One can update Rust and install toolchain targets with [`rustup`](https://rustup.rs/):

```
$ rustup update
$ rustup target install x86_64-apple-ios aarch64-apple-ios aarch64-apple-ios-sim
```

Ensure that cmake is installed (via e.g. MacPorts or Homebrew) and is accessible in a system-wide `$PATH` directory:

```
$ sudo ln -s $(which cmake) /usr/local/bin/
```

Open the `Sendfile.xcworkspace` in Xcode.

```
$ open Sendfile.xcworkspace
```

In the Project Settings Editor, select the Sendfile target, navigate to the Signing & Capabilities tab, and select your own Team for signing. Xcode must be logged in to an Apple Developer account.

Then build and run the app.

## License

No public license yet.
