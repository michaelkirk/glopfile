import { APIClient } from "./APIClient";
import { CipherKey } from "./cipher";
import Base64 from "./Base64";

export class SenderClient {
  apiClient: APIClient;

  constructor(endpoint: URL, cipherKey: CipherKey) {
    this.apiClient = new APIClient(endpoint, cipherKey);
  }

  static async build(endpoint: URL): Promise<SenderClient> {
    return new SenderClient(endpoint, await CipherKey.random());
  }

  async provisionFile(file: File): Promise<ProvisionedFile> {
    const provisionFileResponse = await this.apiClient.provisionFile(
      file.name,
      file.size
    );
    const uploadPath = provisionFileResponse.uploadURL;
    const downloadId = provisionFileResponse.downloadId;

    return new ProvisionedFile(
      file,
      uploadPath,
      downloadId,
      this.apiClient.cipherKey
    );
  }

  async uploadProvisionedFile(
    provisionedFile: ProvisionedFile,
    progressHandler: (complete: number, total: number) => void
  ): Promise<void> {
    return this.apiClient.uploadFile(
      provisionedFile.file,
      provisionedFile.uploadURL,
      progressHandler
    );
  }
}

export class ProvisionedFile {
  file: File;
  uploadURL: string;
  downloadId: string;
  cipherKey: CipherKey;

  constructor(
    file: File,
    uploadURL: string,
    downloadId: string,
    cipherKey: CipherKey
  ) {
    this.file = file;
    this.uploadURL = uploadURL;
    this.downloadId = downloadId;
    this.cipherKey = cipherKey;
  }

  async cipherKeyFragment(): Promise<string> {
    const keyBytes = await this.cipherKey.serialized();
    const encoded = Base64.urlSafeEncode(keyBytes);
    return `cipher_key=${encoded}`;
  }

  async downloadURLWithCipherKey(): Promise<URL> {
    let url = new URL(window.location.toString());
    url.pathname = `/download/${this.downloadId}`;
    url.hash = await this.cipherKeyFragment();
    return url;
  }
}
