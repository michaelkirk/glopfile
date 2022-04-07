export {
  HTTPErrorResponseError, IOError, HTTPClientError, RTCDataChannelError, WebSocketClientError,
  InvalidPeerMessageError, InvalidInputError, InvalidCipherKeyError, DecryptError, InvalidServerResponseError,
  TimeoutError,
} from "sendfile-www-rust";

import * as rust from "sendfile-www-rust";

interface AllErrorNames extends
rust.HTTPErrorResponseErrorName,
rust.IOErrorName,
rust.HTTPClientErrorName,
rust.RTCDataChannelErrorName,
rust.WebSocketClientErrorName,
rust.InvalidPeerMessageErrorName,
rust.InvalidInputErrorName,
rust.InvalidCipherKeyErrorName,
rust.DecryptErrorName,
rust.InvalidServerResponseErrorName,
rust.TimeoutErrorName {
}

export function errorName(name: keyof AllErrorNames): string {
  return name;
}
