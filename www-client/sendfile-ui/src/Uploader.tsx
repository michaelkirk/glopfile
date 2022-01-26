import React from "react";
import { SenderClient } from "sendfile";

import FilePicker from "./FilePicker";
import FileUploader from "./FileUploader";
import FileUpload from "./FileUpload";

class UploaderProps {
  endpoint: URL;
  constructor(endpoint: URL) {
    this.endpoint = endpoint;
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
    const senderClient = SenderClient.build(props.endpoint);
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
    const apiDownloadURLWithCipherKey =
      await provisionedFile.apiDownloadURLWithCipherKey();
    const webDownloadURLWithCipherKey =
      await provisionedFile.webDownloadURLWithCipherKey();
    // TODO clean up / hide these details from App
    // It has to be async because we're exporting cipherkey.
    // It's a weird dance to have the two links (one for "api" and one for
    // "web", but that's because the "web" link serves the web client
    // download app, while the api link serves the actual data used by the
    // client (be it a rust client or web client).
    //
    // Maybe an easier situation would be to have the api server serve the
    // web-client when requesting the download link with the accepts headers
    // that look like a browser vs. the actual download data when requesting
    // from one of the clients, which presumably set something like "accepts json"
    const fileUpload = new FileUpload(
      fileName,
      fileSize,
      apiDownloadURLWithCipherKey,
      webDownloadURLWithCipherKey
    );
    this.setState({ fileUpload });

    await senderClient.uploadProvisionedFile(provisionedFile);
  }
}

export default Uploader;
