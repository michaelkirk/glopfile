import { DownloadableFile, DownloadMeta, DownloaderClient as RustDownloaderClient, DownloadEventHandler } from "glopfile-www-rust";
const rustPromise: Promise<typeof import('glopfile-www-rust')> = import("glopfile-www-rust").then(m => {
  return m.default;
});

export { DownloadMeta };

export class DownloaderClient {
  inner?: RustDownloaderClient;

  constructor(downloadURL: URL, apiEndpoint: URL, rust: typeof import('glopfile-www-rust')) {
    this.inner = new rust.DownloaderClient(downloadURL.toString(), apiEndpoint);
  }

  static async fromDownloadURL(
    downloadURL: URL,
    apiEndpoint: URL
  ): Promise<DownloaderClient> {
    return new DownloaderClient(downloadURL, apiEndpoint, await rustPromise);
  }

  async fetchMeta(): Promise<DownloadMeta> {
    return await this.inner!.fetchMeta();
  }

  async downloadContent(
    meta: DownloadMeta,
    timeout: number | undefined,
    progressHandler: (completed: number, total: number) => void,
  ): Promise<DownloadFile> {
    let file = new DownloadFile();
    let state = new DownloadState(progressHandler);
    await this.inner!.download(meta, file, timeout, state);
    return file;
  }

  free() {
    this.inner?.free();
    this.inner = undefined;
  }
}

export class DownloadFile implements DownloadableFile {
  data: Array<Uint8Array> = [];

  async write(newData: Uint8Array): Promise<void> {
    this.data.push(newData);
  }
}

class DownloadState implements DownloadEventHandler {
  progressHandler: (completed: number, total: number) => void;

  constructor(progressHandler: (completed: number, total: number) => void) {
    this.progressHandler = progressHandler;
  }

  downloadProgress(offset: BigInt, len: BigInt) {
    this.progressHandler(Number(offset), Number(len));
  }
}
