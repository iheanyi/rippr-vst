import "@fontsource-variable/inter";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { MockRipprBridge, NativeRipprBridge } from "./bridge";
import "./styles.css";

const bridge =
  "invoke" in window ? new NativeRipprBridge() : new MockRipprBridge();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App bridge={bridge} />
  </StrictMode>,
);
