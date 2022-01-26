import { CipherKey, ContentCipher } from "./cipher";
import { arrayBufferToBase64, base64ToArrayBuffer } from "./base64";

export class APIClient {
  cipherKey: CipherKey;
  endpoint: URL;

  constructor(endpoint: URL, cipherKey: CipherKey) {
    this.endpoint = endpoint;
    this.cipherKey = cipherKey;
  }

  async provisionFile(
    fileName: string,
    fileSize: number
  ): Promise<ProvisionFileResponse> {
    const fileMeta = new FileMeta(fileName, fileSize);

    let cipher = new ContentCipher(this.cipherKey);
    const encryptedFileMeta: ArrayBuffer = await cipher.encrypt(
      fileMeta.serialized()
    );
    const encoded: string = arrayBufferToBase64(encryptedFileMeta);
    // 🐍 case
    let bodyContent = `encrypted_metadata=${encodeURIComponent(encoded)}`;

    let response = await fetch(this.url("/api/v1/files"), {
      method: "POST",
      body: bodyContent,
    });

    const json = await response.json();
    // 🐍 case
    return new ProvisionFileResponse(json.download_url, json.upload_url);
  }

  async uploadFile(file: File, uploadPath: string): Promise<Response> {
    // TODO stream
    const plaintext = await file.arrayBuffer();
    const cipher = new ContentCipher(this.cipherKey);
    const body = await cipher.encrypt(plaintext);

    return fetch(this.url(uploadPath), {
      method: "POST",
      body,
    });
  }

  async fetchMeta(downloadPath: string): Promise<DownloadMeta> {
    // TODO: do we need to verify success in js?
    const response = await fetch(this.url(downloadPath));
    const json = await response.json();

    const encryptedContentURL = json.encrypted_content_url;
    const base64Meta = json.meta;
    let encryptedFileMeta = base64ToArrayBuffer(base64Meta);

    const serialized: ArrayBuffer = await this.cipher().decrypt(
      encryptedFileMeta
    );
    let fileMeta = FileMeta.fromSerialized(serialized);
    const result = new DownloadMeta(encryptedContentURL, fileMeta);
    return result;
  }

  async downloadContent(downloadMeta: DownloadMeta): Promise<void> {
    const contentURL = this.url(downloadMeta.encryptedContentURL);
    const response = await fetch(contentURL);
    // TODO: stream
    const encryptedBytes = await (await response.blob()).arrayBuffer();
    const decryptedContent = await this.cipher().decrypt(encryptedBytes);

    var blobUrl = URL.createObjectURL(new Blob([decryptedContent]));
    var link = document.createElement("a"); // Or maybe get it from the current document
    link.href = blobUrl;
    link.download = downloadMeta.fileMeta.fileName;
    link.click();
  }

  cipher(): ContentCipher {
    return new ContentCipher(this.cipherKey);
  }

  url(pathname: string): string {
    let url: URL = new URL(this.endpoint);
    url.pathname = pathname;

    // surprisingly `fetch` errors with a URL:
    //
    // > TS2345: Argument of type 'URL' is not assignable to parameter of type 'RequestInfo'.
    // > Type 'URL' is not assignable to type 'string'.
    //
    // So we return a string instead of a URL, since mostly this is being used as args to `fetch`
    return url.toString();
  }
}

class DownloadMeta {
  encryptedContentURL: string;
  fileMeta: FileMeta;

  constructor(encryptedContentURL: string, fileMeta: FileMeta) {
    this.encryptedContentURL = encryptedContentURL;
    this.fileMeta = fileMeta;
  }
}

class FileMeta {
  fileName: string;
  fileSize: number;

  constructor(fileName: string, fileSize: number) {
    const isEmpty = (str: string) => !str || str.length === 0;

    if (isEmpty(fileName)) {
      throw new Error("FileMeta is missing fileName");
    }

    if (fileSize === null || fileSize === 0) {
      throw new Error("FileMeta has missing or zero fileSize");
    }

    this.fileName = fileName;
    this.fileSize = fileSize;
  }

  static fromSerialized(buffer: ArrayBuffer): FileMeta {
    const text = new TextDecoder().decode(buffer);
    const json = JSON.parse(text);
    return new FileMeta(json.file_name, json.file_size);
  }

  serialized(): ArrayBuffer {
    // 🐍 case
    const record = {
      file_name: this.fileName,
      file_size: this.fileSize,
    };
    const bytes: Uint8Array = new TextEncoder().encode(JSON.stringify(record));
    return bytes;
  }
}

export class ProvisionFileResponse {
  downloadURL: string;
  uploadURL: string;

  constructor(downloadURL: string, uploadURL: string) {
    if (!downloadURL) {
      throw new Error("Missing downloadURL");
    }
    if (!uploadURL) {
      throw new Error("Missing uploadURL");
    }

    this.downloadURL = downloadURL;
    this.uploadURL = uploadURL;
  }
}
