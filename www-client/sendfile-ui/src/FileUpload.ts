class FileUpload {
  fileName: string;
  fileSize: number;
  isComplete: boolean;
  progressRatio = 0;
  downloadURLWithCipherKey: URL;

  constructor(
    fileName: string,
    fileSize: number,
    downloadURLWithCipherKey: URL
  ) {
    this.fileName = fileName;
    this.fileSize = fileSize;
    this.downloadURLWithCipherKey = downloadURLWithCipherKey;
    this.isComplete = false;
  }
}

export default FileUpload;
