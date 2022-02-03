import React from "react";
import { ReceiverClient } from "sendfile";

class DownloaderState {
  receiverClient?: Promise<ReceiverClient>;
}

class DownloaderProps {
  location: Location;
  apiEndpoint: URL;

  constructor(location: Location, apiEndpoint: URL) {
    this.location = location;
    this.apiEndpoint = apiEndpoint;
  }
}

class Downloader extends React.Component<DownloaderProps, DownloaderState> {
  constructor(props: DownloaderProps) {
    super(props);
    let state = new DownloaderState();

    const downloadURL = new URL(props.location.toString());
    state.receiverClient = ReceiverClient.fromDownloadURL(
      downloadURL,
      props.apiEndpoint
    );
    this.state = state;
  }

  render(): React.ReactElement {
    return (
      <div>
        <h1>Download</h1>
        <div>
          <button onClick={this.onClickDownloadHandler.bind(this)}>
            Start Download
          </button>
        </div>
      </div>
    );
  }

  async onClickDownloadHandler(): Promise<void> {
    const receiverClient = await this.state.receiverClient!;
    await receiverClient.download();
  }
}

export default Downloader;
