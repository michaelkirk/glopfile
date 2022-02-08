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
    const { downloadId, serializedCipherKey } =
      ReceiverClient.parseDownloadURL(downloadURL);

    const cipherKey = await CipherKey.fromSerializedText(serializedCipherKey);
    return new ReceiverClient(apiEndpoint, cipherKey, downloadId);
  }

  static parseDownloadURL(downloadURL: URL): {
    downloadId: string;
    serializedCipherKey: string;
  } {
    let endpoint = new URL(downloadURL);
    endpoint.pathname = "";
    endpoint.hash = "";

    if (!downloadURL.pathname.startsWith("/download/")) {
      // expose this to UI
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

    return {
      downloadId,
      serializedCipherKey,
    };
  }

  async download(
    progressHandler: (completed: number, total: number) => void
  ): Promise<void> {
    const meta = await this.apiClient.fetchMeta(this.downloadId);
    return this.apiClient.downloadContent(meta, progressHandler);
  }
}
