import React from "react";
import { SenderClient } from "sendfile";

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
  senderClient: Promise<SenderClient>;
  fileUpload?: FileUpload;
  constructor(senderClient: Promise<SenderClient>) {
    this.senderClient = senderClient;
  }
}

class Uploader extends React.Component<UploaderProps, UploaderState> {
  constructor(props: UploaderProps) {
    super(props);
    const senderClient = SenderClient.build(props.apiEndpoint);
    this.state = new UploaderState(senderClient);
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
    let senderClient = await this.state.senderClient!;
    const provisionedFile = await senderClient.provisionFile(file);

    const fileName = file.name;
    const fileSize = file.size;
    const downloadURLWithCipherKey =
      await provisionedFile.downloadURLWithCipherKey();

    const fileUpload = new FileUpload(
      fileName,
      fileSize,
      downloadURLWithCipherKey
    );
    this.setState({ fileUpload });

    await senderClient.uploadProvisionedFile(provisionedFile);
  }
}

export default Uploader;
