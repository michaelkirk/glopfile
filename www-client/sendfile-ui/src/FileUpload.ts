class FileUpload {
  fileName: string;
  fileSize: number;
  uploadByteCount = 0;
  downloadURLWithCipherKey: URL;

  constructor(
    fileName: string,
    fileSize: number,
    downloadURLWithCipherKey: URL
  ) {
    this.fileName = fileName;
    this.fileSize = fileSize;
    this.downloadURLWithCipherKey = downloadURLWithCipherKey;
  }
}

export default FileUpload;
