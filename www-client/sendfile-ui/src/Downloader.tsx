import React from "react";
import { DownloaderClient, DownloadMeta, HTTPErrorResponseError, errorName } from "sendfile";

class DownloaderState {
  downloaderClient?: Promise<DownloaderClient>;
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
        state.downloaderClient = DownloaderClient.fromDownloadURL(
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
    if (this.state.downloaderClient) {
      this.state.downloaderClient.catch((err) => {
        if (err.name === errorName("InvalidCipherKeyError")) {
          this.setState({
            errorText:
              "Invalid cipher key. Did you get the entire link?"
          });
        } else {
          this.setState({ errorText: err.message });
        }
      });
      this.state.downloaderClient.then(async (downloaderClient) => {
        try {
          let downloadMeta = await downloaderClient.fetchMeta();
          this.setState({ downloadMeta });
        } catch (err) {
          let anyErr = err;
          if (!(err instanceof Error)) {
            this.setState({ errorText: String(err) });
          } else if (err.name === errorName("HTTPErrorResponseError")
            && (anyErr as HTTPErrorResponseError).status === 404) {
            this.setState({
              statusText:
                "This link is expired or invalid. Ask the sender to re-upload and send you a new link.",
            });
          } else if (err.name === errorName("DecryptError")) {
            this.setState({
              errorText:
                "Unable to decrypt file metadata. Please check the link or ask your friend to send the file again."
            });
          } else {
            this.setState({ errorText: err.message });
          }
        }
      });
    }
  }

  componentWillUnmount(): void {
    this.state.downloaderClient?.then(downloaderClient => downloaderClient.free());
    this.state.downloadMeta?.free();
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
    return formatBytes(Number(this.state.downloadMeta.fileMeta.fileSize));
  }

  async downloadContent(downloadMeta: DownloadMeta): Promise<void> {
    this.setState({ progress: new Progress() });
    const downloaderClient = await this.state.downloaderClient!;
    let file = await downloaderClient.downloadContent(
      downloadMeta,
      (completed: number, total: number): void => {
        this.setState({ progress: new Progress(completed, total) });
      }
    );

    var blobUrl = URL.createObjectURL(new Blob(file.data));
    var link = document.createElement("a"); // Or maybe get it from the current document
    link.href = blobUrl;
    link.download = downloadMeta.fileMeta.fileName;
    link.click();
  }
}

export default Downloader;
