import React from "react";
import "./App.css";

import Downloader from "./Downloader";
import Uploader from "./Uploader";

class AppState {
  endpoint = new URL("http://localhost:8080");
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
      pageBody = <Uploader endpoint={this.state.endpoint} />;
    } else if (window.location.pathname === "/download") {
      pageBody = <Downloader location={window.location} />;
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
    if (window.location.pathname === this.props.path) {
      return <b>{this.props.text}</b>;
    } else {
      return <a href={this.props.path}>{this.props.text}</a>;
    }
  }
}

export default App;
