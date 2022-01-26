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
        <h2>via api</h2>
        <p>
          The recipient's client will need the following link to receive your
          file.
        </p>
        <p>
          {this.apiDownloadURLWithCipherKey()}
          <br />[
          <a
            href="#"
            onClick={(_) =>
              this.copyToClipboard(this.apiDownloadURLWithCipherKey())
            }
          >
            Copy
          </a>
          ]
        </p>

        <h2>via web</h2>
        <p>
          Or the recipient can visit the following link to start a browser based
          client.
        </p>
        <p>
          {this.webDownloadURLWithCipherKey()}
          <br />[
          <a
            href="#"
            onClick={(_) =>
              this.copyToClipboard(this.webDownloadURLWithCipherKey())
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

  apiDownloadURLWithCipherKey(): string {
    return this.props.fileUpload.apiDownloadURLWithCipherKey.toString();
  }

  webDownloadURLWithCipherKey(): string {
    return this.props.fileUpload.webDownloadURLWithCipherKey.toString();
  }
}

export default FileUploader;
