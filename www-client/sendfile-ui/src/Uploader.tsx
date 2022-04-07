import React from "react";
import { UploaderClient } from "sendfile";

import FilePicker from "./FilePicker";
import FileUploader from "./FileUploader";
import FileUpload from "./FileUpload";

class UploaderProps {
  apiEndpoint: URL;
  constructor(apiEndpoint: URL) {
    this.apiEndpoint = apiEndpoint;
  }
}
class UploaderState {
  uploaderClient: Promise<UploaderClient>;
  fileUpload?: FileUpload;
  constructor(uploaderClient: Promise<UploaderClient>) {
    this.uploaderClient = uploaderClient;
  }
}

class Uploader extends React.Component<UploaderProps, UploaderState> {
  constructor(props: UploaderProps) {
    super(props);
    const uploaderClient = UploaderClient.build(props.apiEndpoint);
    this.state = new UploaderState(uploaderClient);
  }

  componentWillUnmount() {
    this.state.uploaderClient.then(uploaderClient => uploaderClient.free())
  }

  render(): React.ReactNode {
    let pageTitle;
    let pageBody;

    if (this.state.fileUpload == null) {
      pageTitle = "Step 1: Choose File to Send";
      pageBody = <FilePicker onFileAdded={this.addFile.bind(this)} />;
    } else {
      pageTitle = "Step 2: Send Link to Recipient";
      pageBody = <FileUploader fileUpload={this.state.fileUpload} />;
    }
    return (
      <div>
        <h2>{pageTitle}</h2>
        {pageBody}
      </div>
    );
  }

  async addFile(file: File): Promise<void> {
    let uploaderClient = await this.state.uploaderClient!;
    const provisionedFile = await uploaderClient.provisionFile(file);

    try {
      const fileName = file.name;
      const fileSize = file.size;
      const downloadURLWithCipherKey =
        provisionedFile.downloadURLWithCipherKey();

      const fileUpload = new FileUpload(
        fileName,
        fileSize,
        downloadURLWithCipherKey
      );
      this.setState({ fileUpload });

      await uploaderClient.uploadProvisionedFile(
        provisionedFile,
        (completed: number, total: number): void => {
          let fileUpload = this.state.fileUpload!;
          const progressedFileUpload = Object.assign({}, fileUpload);
          progressedFileUpload.progressRatio = completed / total;
          this.setState({ fileUpload: progressedFileUpload });
        }
      );
    } finally {
      provisionedFile.free();
    }

    (() => {
      let fileUpload = this.state.fileUpload!;
      const completedFileUpload = Object.assign({}, fileUpload);
      completedFileUpload.isComplete = true;
      this.setState({ fileUpload: completedFileUpload });
    })();
  }
}

export default Uploader;
