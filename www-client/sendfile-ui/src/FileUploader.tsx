import React from "react";
import FileUpload from "./FileUpload";
import toast, { Toaster } from "react-hot-toast";
import { QRCodeSVG } from "qrcode.react";

class FileUploaderProps {
  fileUpload: FileUpload;
  constructor(fileUpload: FileUpload) {
    this.fileUpload = fileUpload;
  }
}

class FileUploader extends React.Component<FileUploaderProps, {}> {
  render() {
    let body;

    if (this.props.fileUpload.isComplete) {
      body = (
        <div>
          <p>
            <b>🎉 Done!</b> You can close this window.
          </p>
          <p>
            <a href="/">send another file</a>
          </p>
        </div>
      );
    } else {
      body = (
        <div>
          <p>Send this download link to the recipient to continue uploading.</p>
          <p>
            {this.downloadURLWithCipherKey()}
            <br />[
            <a
              href="#copy"
              onClick={(_) => {
                this.copyToClipboard(this.downloadURLWithCipherKey());
                toast.success("Link copied!");
              }}
            >
              Copy
            </a>
            ]
            <Toaster containerStyle={{ position: "relative" }} />
          </p>
          <p className="FileUploader-QR">
            <QRCodeSVG value={this.downloadURLWithCipherKey()} includeMargin />
          </p>
        </div>
      );
    }

    return (
      <div className="FileUploader">
        <table>
          <tbody>
            <tr>
              <th>file</th>
              <td>{this.fileName()}</td>
            </tr>
            <tr>
              <th>uploaded</th>
              <td>{this.uploadProgress()}</td>
            </tr>
          </tbody>
        </table>
        {body}
      </div>
    );
  }

  copyToClipboard(text: string): void {
    navigator.clipboard.writeText(text);
  }

  fileName(): string {
    return this.props.fileUpload.fileName;
  }

  uploadProgress(): string {
    const fileUpload = this.props.fileUpload;
    const percent = 100.0 * fileUpload.progressRatio;
    return `${percent.toFixed(1)}%`;
  }

  downloadURLWithCipherKey(): string {
    return this.props.fileUpload.downloadURLWithCipherKey.toString();
  }
}

export default FileUploader;
