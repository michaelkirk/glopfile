import React from "react";
import "./App.css";

import Downloader from "./Downloader";
import Uploader from "./Uploader";

class AppState {
  apiEndpoint = new URL(process.env.REACT_APP_GLOPFILE_API_ENDPOINT!);
}

class App extends React.Component<{}, AppState> {
  constructor(props: {}) {
    super(props);
    let state = new AppState();
    this.state = state;
  }

  render(): React.ReactNode {
    let pageBody;
    if (window.location.pathname === "/") {
      pageBody = <Uploader apiEndpoint={this.state.apiEndpoint} />;
    } else if (window.location.pathname.startsWith("/download")) {
      pageBody = (
        <Downloader
          location={window.location}
          apiEndpoint={this.state.apiEndpoint}
        />
      );
    } else {
      pageBody = (
        <div>
          <h1>Bad url. Not Found. 404ish</h1>
          <p>That page doesn't exist.</p>
        </div>
      );
    }

    return (
      <div className="App">
        <div className="nav">
          <NavLink path="/" text="send a file" /> |{" "}
          <NavLink path="/download" text="download a file" />
        </div>
        {pageBody}

        <footer className="App-footer">
          <span className="farewell">Have a nice day. ^_^</span>
        </footer>
      </div>
    );
  }
}

class NavLinkProps {
  path: string;
  text: string;
  constructor(path: string, text: string) {
    this.path = path;
    this.text = text;
  }
}

class NavLink extends React.Component<NavLinkProps, {}> {
  render(): React.ReactElement {
    let isCurrentPage: boolean;
    if (this.props.path === "/") {
      isCurrentPage = window.location.pathname === "/";
    } else {
      isCurrentPage = window.location.pathname.startsWith(this.props.path);
    }

    if (isCurrentPage) {
      return <b>{this.props.text}</b>;
    } else {
      return <a href={this.props.path}>{this.props.text}</a>;
    }
  }
}

export default App;
