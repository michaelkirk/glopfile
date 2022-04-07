import { UploadableFile, UploaderClient as RustUploaderClient, ProvisionedFile, UploadEventHandler } from "sendfile-www-rust";
const rustPromise: Promise<typeof import('sendfile-www-rust')> = import("sendfile-www-rust").then(m => {
  return m.default;
});

export { ProvisionedFile };

export class UploaderClient {
  inner?: RustUploaderClient;

  constructor(endpoint: URL, rust: typeof import('sendfile-www-rust')) {
    let downloadEndpoint = new URL(window.location.toString());
    downloadEndpoint.hash = '';
    this.inner = new rust.UploaderClient(endpoint, downloadEndpoint);
  }

  static async build(endpoint: URL): Promise<UploaderClient> {
    return new UploaderClient(endpoint, await rustPromise);
  }

  async provisionFile(file: File): Promise<ProvisionedFile> {
    return await this.inner!.provisionFile(new UploadFile(file), file.name);
  }

  async uploadProvisionedFile(
    provisionedFile: ProvisionedFile,
    progressHandler: (complete: number, total: number) => void
  ): Promise<void> {
    let state = new UploadState(progressHandler);
    await this.inner!.uploadFile(provisionedFile, state);
  }

  free() {
    this.inner?.free();
    this.inner = undefined;
  }
}

class UploadFile implements UploadableFile {
  file: File;

  constructor(file: File) {
    this.file = file;
  }

  len(): BigInt {
    return BigInt(this.file.size);
  }

  async readAt(offset: BigInt, len: BigInt): Promise<Uint8Array> {
    const data = await this.file.slice(Number(offset), Number(offset) + Number(len)).arrayBuffer();
    return new Uint8Array(data);
  }
}


class UploadState implements UploadEventHandler {
  progressHandler: (completed: number, total: number) => void;

  constructor(progressHandler: (completed: number, total: number) => void) {
    this.progressHandler = progressHandler;
  }

  uploadProgress(offset: BigInt, len: BigInt) {
    this.progressHandler(Number(offset), Number(len));
  }
}
