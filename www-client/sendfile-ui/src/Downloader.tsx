import React from "react";
import { ReceiverClient } from "sendfile";

class DownloaderState {
  receiverClient?: Promise<ReceiverClient>;
  errorText?: string;
  progress?: Progress;
}

class Progress {
  completed: number;
  total: number;

  constructor(completed: number = 0, total: number = 0) {
    this.completed = completed;
    this.total = total;
  }

  ratio(): number {
    if (this.completed === 0) {
      return 0;
    } else {
      return this.completed / this.total;
    }
  }
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

    if (props.location.pathname === "/download") {
      state.errorText =
        "To download a file, ask the sender for the download link.";
    } else {
      const downloadURL = new URL(props.location.toString());
      try {
        ReceiverClient.parseDownloadURL(downloadURL);
        state.receiverClient = ReceiverClient.fromDownloadURL(
          downloadURL,
          props.apiEndpoint
        );
      } catch {
        state.errorText =
          "The download link was invalid. Ask the sender to re-send you the download link.";
        console.error("unable to parse download url");
      }
    }

    this.state = state;
  }

  render(): React.ReactElement {
    if (this.state.errorText) {
      return <p>{this.state.errorText}</p>;
    }
    return (
      <div>
        <h1>Download</h1>
        <div>
          {this.state.progress && (
            <table>
              <tbody>
                <tr>
                  <th>downloaded</th>
                  <td>{this.downloadProgress(this.state.progress)}</td>
                </tr>
              </tbody>
            </table>
          )}
          <button onClick={this.onClickDownloadHandler.bind(this)}>
            Start Download
          </button>
        </div>
      </div>
    );
  }

  downloadProgress(progress: Progress): string {
    const percent = 100.0 * progress.ratio();
    return `${percent.toFixed(1)}%`;
  }

  async onClickDownloadHandler(): Promise<void> {
    this.setState({ progress: new Progress() });
    const receiverClient = await this.state.receiverClient!;
    await receiverClient.download((completed: number, total: number): void => {
      this.setState({ progress: new Progress(completed, total) });
    });
  }
}

export default Downloader;
