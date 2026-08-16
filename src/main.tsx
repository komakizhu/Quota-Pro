import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { DesignPlayground } from "./components/DesignPlayground";
import { SettingsPanel } from "./components/SettingsPanel";
import "./styles.css";

const search = new URLSearchParams(window.location.search);
const showDesigner = search.has("designer") || search.has("design");
const showSettings = search.has("settings");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{showSettings ? <SettingsPanel /> : showDesigner ? <DesignPlayground /> : <App />}</React.StrictMode>,
);
