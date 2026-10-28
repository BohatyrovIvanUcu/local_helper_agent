import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { DiskAnalyzerApp } from "./components/DiskAnalyzer/DiskAnalyzerApp";
import { ShareApp } from "./components/Share/ShareApp";
import "./index.css";

const urlParams = new URLSearchParams(window.location.search);
const view = urlParams.get("view");

function RootRouter() {
  if (view === "disk_analyzer") {
    return <DiskAnalyzerApp />;
  }
  if (view === "share") {
    return <ShareApp />;
  }
  return <App />;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <RootRouter />
  </React.StrictMode>
);


