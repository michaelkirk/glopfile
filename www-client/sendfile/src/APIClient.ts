import { CipherKey, ContentCipher } from "./cipher";
import Base64 from "./Base64";

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
    const encoded: string = Base64.bodyEncode(encryptedFileMeta);
    // 🐍 case
    let bodyContent = `encrypted_metadata=${encodeURIComponent(encoded)}`;

    let response = await fetch(this.url("/api/v1/files"), {
      method: "POST",
      body: bodyContent,
    });

    const json = await response.json();
    // 🐍 case
    return new ProvisionFileResponse(json.download_id, json.upload_url);
  }

  async uploadFile(
    file: File,
    uploadPath: string,
    progressHandler: (complete: number, total: number) => void
  ): Promise<void> {
    // TODO stream
    const plaintext = await file.arrayBuffer();
    const cipher = new ContentCipher(this.cipherKey);
    const body = await cipher.encrypt(plaintext);

    let promise = new Promise<void>((resolve, _reject) => {
      let xhr = new XMLHttpRequest();
      xhr.upload.addEventListener(
        "progress",
        (e) => {
          progressHandler(e.loaded, e.total);
        },
        false
      );

      xhr.onreadystatechange = (_) => {
        if (xhr.readyState == 4) {
          resolve();
        }
      };

      xhr.open("post", this.url(uploadPath), true);
      xhr.send(body);
    });

    return promise;
  }

  async fetchMeta(downloadId: string): Promise<DownloadMeta> {
    // TODO: do we need to verify success in js?
    const downloadPath = `/api/v1/download/${downloadId}`;
    const response = await fetch(this.url(downloadPath));
    const json = await response.json();

    const encryptedContentURL = json.encrypted_content_url;
    let encryptedFileMeta = Base64.bodyDecode(json.meta);

    const serialized: ArrayBuffer = await this.cipher().decrypt(
      encryptedFileMeta
    );
    let fileMeta = FileMeta.fromSerialized(serialized);
    const result = new DownloadMeta(encryptedContentURL, fileMeta);
    return result;
  }

  async downloadContent(
    downloadMeta: DownloadMeta,
    progressHandler: (completed: number, total: number) => void
  ): Promise<void> {
    const contentURL = this.url(downloadMeta.encryptedContentURL);

    let downloadContent = new Promise<ArrayBuffer>((resolve, _reject) => {
      let xhr = new XMLHttpRequest();
      xhr.responseType = "arraybuffer";

      xhr.addEventListener(
        "progress",
        (e) => {
          // There's currently no content-length from the server so we have
          // to synthesize `total`.
          // Ultimately we probably want to include decryption progress and whatever else in the progress
          // handler, but for now we can expect the vast majority to be spent in network i/o
          const total = downloadMeta.fileMeta.fileSize;
          progressHandler(e.loaded, total);
        },
        false
      );

      xhr.onreadystatechange = (_) => {
        if (xhr.readyState == 4) {
          resolve(xhr.response);
        }
      };

      xhr.open("get", contentURL, true);
      xhr.send();
    });

    await downloadContent;

    // TODO: stream
    const encryptedBytes: ArrayBuffer = await downloadContent;
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
  downloadId: string;
  uploadURL: string;

  constructor(downloadId: string, uploadURL: string) {
    if (!downloadId) {
      throw new Error("Missing downloadId");
    }
    if (!uploadURL) {
      throw new Error("Missing uploadURL");
    }

    this.downloadId = downloadId;
    this.uploadURL = uploadURL;
  }
}
