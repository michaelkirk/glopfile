import { APIClient } from "./APIClient";
import { CipherKey } from "./cipher";

export class ReceiverClient {
  apiClient: APIClient;
  downloadPath: string;

  constructor(endpoint: URL, cipherKey: CipherKey, downloadPath: string) {
    this.apiClient = new APIClient(endpoint, cipherKey);
    this.downloadPath = downloadPath;
  }

  static async fromDownloadURL(downloadURL: URL): Promise<ReceiverClient> {
    const { endpoint, downloadPath, cipherKey } =
      await ReceiverClient.parseDownloadURL(downloadURL);

    return new ReceiverClient(endpoint, cipherKey, downloadPath);
  }

  static async parseDownloadURL(
    downloadURL: URL
  ): Promise<{ endpoint: URL; downloadPath: string; cipherKey: CipherKey }> {
    let endpoint = new URL(downloadURL);
    endpoint.pathname = "";
    endpoint.hash = "";

    const downloadPath = downloadURL.pathname;

    if (!downloadURL.hash.startsWith("#cipher_key=")) {
      throw Error(
        `download URL ${downloadURL.hash} is missing "cipher_key" fragment`
      );
    }
    let serializedCipherKey = downloadURL.hash.replace(/^#cipher_key=/, "");

    const cipherKey = await CipherKey.fromSerializedText(serializedCipherKey);

    return {
      endpoint,
      downloadPath,
      cipherKey,
    };
  }

  async download(): Promise<void> {
    const meta = await this.apiClient.fetchMeta(this.downloadPath);
    return this.apiClient.downloadContent(meta);
  }
}
