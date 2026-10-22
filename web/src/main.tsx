import "@/index.css";
import "@/i18n";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { createTransport } from "@/api/transport";
import { App } from "@/App";

const root = document.getElementById("root");
if (!root) throw new Error("#root element is missing from index.html");

createRoot(root).render(
  <StrictMode>
    <App transport={createTransport()} />
  </StrictMode>,
);
