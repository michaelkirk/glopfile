import React from "react";
import FileUpload from "./FileUpload";

class FileUploaderProps {
  fileUpload: FileUpload;
  constructor(fileUpload: FileUpload) {
    this.fileUpload = fileUpload;
  }
}

class FileUploader extends React.Component<FileUploaderProps, {}> {
  render() {
    return (
      <div className="FileUploader">
        <p>Uploading file: {this.fileName()}</p>
        <p>Send this download link to the recipient.</p>
        <p>
          {this.downloadURLWithCipherKey()}
          <br />[
          <a
            href="#copy"
            onClick={(_) =>
              this.copyToClipboard(this.downloadURLWithCipherKey())
            }
          >
            Copy
          </a>
          ]
        </p>
      </div>
    );
  }

  copyToClipboard(text: string): void {
    navigator.clipboard.writeText(text);
  }

  fileName(): string {
    return this.props.fileUpload.fileName;
  }

  downloadURLWithCipherKey(): string {
    return this.props.fileUpload.downloadURLWithCipherKey.toString();
  }
}

export default FileUploader;
