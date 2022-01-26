import React from "react";
import { ReceiverClient } from "sendfile";

class DownloaderState {
  downloadURL?: URL;
  receiverClient?: Promise<ReceiverClient>;
}

class DownloaderProps {
  location: Location;

  constructor(location: Location) {
    this.location = location;
  }
}

class Downloader extends React.Component<DownloaderProps, DownloaderState> {
  constructor(props: DownloaderProps) {
    super(props);
    let state = new DownloaderState();

    const downloadURL = Downloader.downloadURLFromLocation(props.location);
    if (downloadURL != null) {
      state.downloadURL = downloadURL;
      state.receiverClient = ReceiverClient.fromDownloadURL(downloadURL);
    }
    this.state = state;
  }

  render(): React.ReactElement {
    let downloadURLField;
    if (this.state.downloadURL == null) {
      downloadURLField = (
        <input
          id="download-url"
          type="text"
          onChange={this.downloadURLDidChange.bind(this)}
        />
      );
    } else {
      downloadURLField = <span>{this.state.downloadURL.toString()}</span>;
    }

    return (
      <div>
        <h1>Download</h1>
        <label htmlFor="download-url">API Download URL: </label>
        <p>{downloadURLField}</p>
        <button
          disabled={this.state.downloadURL == null}
          onClick={this.onClickDownloadHandler.bind(this)}
        >
          Start Download
        </button>
      </div>
    );
  }

  downloadURLDidChange(_e: any): void {
    const field: HTMLInputElement = document.getElementById(
      "download-url"
    )! as HTMLInputElement;
    const text: string = field.value;

    let downloadURL;
    try {
      downloadURL = Downloader.parseDownloadURL(text);
    } catch (e) {
      console.log("incomplete or invalid download URL", e);
    }

    console.assert(this.state.downloadURL == null);
    console.assert(this.state.receiverClient == null);
    if (downloadURL != null) {
      this.setState({
        downloadURL,
        receiverClient: ReceiverClient.fromDownloadURL(downloadURL),
      });
    }
  }

  // Throws Error if invalid
  static parseDownloadURL(downloadURLString: string): URL {
    const downloadURL = new URL(downloadURLString);
    if (downloadURL == null) {
      throw Error("`downloadURL` param not parseable as URL");
    }

    // We have some split brain with the www server being separate from the api-server...
    if (!downloadURL.pathname.startsWith("/api/v1/download")) {
      throw Error(
        "`downloadURL` param must have path that starts with /api/v1/download"
      );
    }

    return downloadURL;
  }

  static downloadURLFromLocation(location: Location): URL | null {
    let url = new URL(location.toString());
    const downloadURLString = url.searchParams.get("downloadURL");
    if (downloadURLString == null) {
      return null;
    }

    let downloadURL = Downloader.parseDownloadURL(downloadURLString);

    // copy cipher_key fragment from window
    downloadURL.hash = window.location.hash;
    return downloadURL;
  }

  async onClickDownloadHandler(): Promise<void> {
    const receiverClient = await this.state.receiverClient!;
    await receiverClient.download();
  }
}

export default Downloader;
