import { APIClient } from "./APIClient";
import { CipherKey } from "./cipher";
import { arrayBufferToBase64 } from "./base64";

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
    const downloadURL = this.downloadURLWithoutCipherKey(
      provisionFileResponse.downloadURL
    );

    return new ProvisionedFile(
      file,
      uploadPath,
      downloadURL,
      this.apiClient.cipherKey
    );
  }

  async uploadProvisionedFile(
    provisionedFile: ProvisionedFile
  ): Promise<Response> {
    return this.apiClient.uploadFile(
      provisionedFile.file,
      provisionedFile.uploadURL
    );
  }

  downloadURLWithoutCipherKey(downloadPath: string): URL {
    let url: URL = new URL(this.apiClient.endpoint);
    url.pathname = downloadPath;
    return url;
  }
}

export class ProvisionedFile {
  file: File;
  uploadURL: string;
  downloadURLWithoutCipherKey: URL;
  cipherKey: CipherKey;

  constructor(
    file: File,
    uploadURL: string,
    downloadURLWithoutCipherKey: URL,
    cipherKey: CipherKey
  ) {
    this.file = file;
    this.uploadURL = uploadURL;
    this.downloadURLWithoutCipherKey = downloadURLWithoutCipherKey;
    this.cipherKey = cipherKey;
  }

  async cipherKeyFragment(): Promise<string> {
    const keyBytes = await this.cipherKey.serialized();
    const encoded = arrayBufferToBase64(keyBytes);
    return `cipher_key=${encoded}`;
  }

  async apiDownloadURLWithCipherKey(): Promise<URL> {
    let url: URL = new URL(this.downloadURLWithoutCipherKey);
    url.hash = await this.cipherKeyFragment();
    return url;
  }

  async webDownloadURLWithCipherKey(): Promise<URL> {
    let url = new URL(window.location.toString());
    url.pathname = "/download";
    // TODO: encode?
    url.searchParams.set(
      "downloadURL",
      encodeURI(this.downloadURLWithoutCipherKey.toString())
    );
    url.hash = await this.cipherKeyFragment();
    return url;
  }
}
