class FileUpload {
  fileName: string;
  fileSize: number;
  uploadByteCount = 0;
  apiDownloadURLWithCipherKey: URL;
  webDownloadURLWithCipherKey: URL;

  constructor(
    fileName: string,
    fileSize: number,
    apiDownloadURLWithCipherKey: URL,
    webDownloadURLWithCipherKey: URL
  ) {
    this.fileName = fileName;
    this.fileSize = fileSize;
    this.apiDownloadURLWithCipherKey = apiDownloadURLWithCipherKey;
    this.webDownloadURLWithCipherKey = webDownloadURLWithCipherKey;
  }
}

export default FileUpload;
