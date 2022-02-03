import { APIClient } from "./APIClient";
import { CipherKey } from "./cipher";

export class ReceiverClient {
  apiClient: APIClient;
  downloadId: string;

  constructor(endpoint: URL, cipherKey: CipherKey, downloadId: string) {
    this.apiClient = new APIClient(endpoint, cipherKey);
    this.downloadId = downloadId;
  }

  static async fromDownloadURL(
    downloadURL: URL,
    apiEndpoint: URL
  ): Promise<ReceiverClient> {
    const { downloadId, cipherKey } = await ReceiverClient.parseDownloadURL(
      downloadURL
    );

    return new ReceiverClient(apiEndpoint, cipherKey, downloadId);
  }

  static async parseDownloadURL(
    downloadURL: URL
  ): Promise<{ downloadId: string; cipherKey: CipherKey }> {
    let endpoint = new URL(downloadURL);
    endpoint.pathname = "";
    endpoint.hash = "";

    if (!downloadURL.pathname.startsWith("/download/")) {
      throw Error(`${downloadURL.pathname} doesn't look like a download URL`);
    }
    const downloadIdMatches = downloadURL.pathname.match("^/download/([^/]+)");
    if (!downloadIdMatches || downloadIdMatches.length !== 2) {
      throw Error(`Unable to parse downloadID from ${downloadURL.pathname}`);
    }
    const downloadId = downloadIdMatches[1];

    if (!downloadURL.hash.startsWith("#cipher_key=")) {
      throw Error(
        `download URL ${downloadURL.hash} is missing "cipher_key" fragment`
      );
    }
    let serializedCipherKey = downloadURL.hash.replace(/^#cipher_key=/, "");

    const cipherKey = await CipherKey.fromSerializedText(serializedCipherKey);

    return {
      downloadId,
      cipherKey,
    };
  }

  async download(): Promise<void> {
    const meta = await this.apiClient.fetchMeta(this.downloadId);
    return this.apiClient.downloadContent(meta);
  }
}
