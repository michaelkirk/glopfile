import React from "react";
import { ReceiverClient, DownloadMeta } from "sendfile";

class DownloaderState {
  receiverClient?: Promise<ReceiverClient>;
  downloadMeta?: DownloadMeta;
  errorText?: string;
  statusText?: string;
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
      state.statusText =
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

  componentDidMount(): void {
    if (this.state.receiverClient) {
      this.state.receiverClient.catch((err) => {
        this.setState({ errorText: err.message });
      });
      this.state.receiverClient.then((receiverClient) => {
        receiverClient
          .fetchMeta()
          .then((downloadMeta) => {
            this.setState({ downloadMeta });
          })
          .catch((err) => {
            this.setState({ errorText: err.message });
          });
      });
    }
  }

  render(): React.ReactElement {
    if (this.state.errorText) {
      return <p>🤮 {this.state.errorText}</p>;
    }

    if (this.state.statusText) {
      return <p>🕵️ {this.state.statusText}</p>;
    }

    let maybeDownloadButton;
    const hasStartedDownloadingContent = !!this.state.progress;
    if (this.state.downloadMeta && !hasStartedDownloadingContent) {
      let downloadMeta: DownloadMeta = this.state.downloadMeta;
      maybeDownloadButton = (
        <button onClick={() => this.downloadContent(downloadMeta)}>
          Start Download
        </button>
      );
    }

    return (
      <div>
        <h1>Download</h1>
        {this.state.downloadMeta ? (
          <div>
            <table>
              <tbody>
                <tr>
                  <th>file</th>
                  <td>{this.formattedFileName()}</td>
                </tr>
                <tr>
                  <th>size</th>
                  <td>{this.formattedFileSize()}</td>
                </tr>
                <tr>
                  <th>downloaded</th>
                  <td>{this.formattedDownloadProgress()}</td>
                </tr>
              </tbody>
            </table>
            {maybeDownloadButton}
          </div>
        ) : (
          <p>Fetching download information...</p>
        )}
      </div>
    );
  }

  formattedDownloadProgress(): string {
    if (this.state.progress) {
      // a little bit of overhead from encryption leads to slightly more than
      // 1.0 ratio.
      const ratio = Math.min(1.0, this.state.progress.ratio());
      const percent = 100.0 * ratio;
      return `${percent.toFixed(1)}%`;
    } else {
      return "";
    }
  }

  formattedFileName(): string {
    if (!this.state.downloadMeta) {
      return "unknown size";
    }
    return this.state.downloadMeta.fileMeta.fileName;
  }

  formattedFileSize(): string {
    // modified from https://stackoverflow.com/a/18650828/353178
    const formatBytes = (bytes: number, decimals = 2) => {
      if (bytes === 0) return "0 Bytes";

      //const k = 1024;
      // 1000 matches what's shown in, e.g. finder
      const k = 1000;
      const dm = decimals < 0 ? 0 : decimals;
      const sizes = ["Bytes", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];

      const i = Math.floor(Math.log(bytes) / Math.log(k));

      return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + " " + sizes[i];
    };

    if (!this.state.downloadMeta) {
      return "unknown size";
    }
    return formatBytes(this.state.downloadMeta.fileMeta.fileSize);
  }

  async downloadContent(downloadMeta: DownloadMeta): Promise<void> {
    this.setState({ progress: new Progress() });
    const receiverClient = await this.state.receiverClient!;
    await receiverClient.downloadContent(
      downloadMeta,
      (completed: number, total: number): void => {
        this.setState({ progress: new Progress(completed, total) });
      }
    );
  }
}

export default Downloader;
