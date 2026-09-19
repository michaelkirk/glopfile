// jest cannot instantiate the client's wasm, so the rust module stands in for
// it: enough surface for the app to construct and tear down its clients.
class UploaderClient {
  free() {}
}

class DownloaderClient {
  free() {}
}

module.exports = { UploaderClient, DownloaderClient };
