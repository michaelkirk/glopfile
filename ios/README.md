# Sendfile iOS

Send a file with your iOS device!

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
