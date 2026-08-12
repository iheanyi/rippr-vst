import "@fontsource-variable/inter";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { MockRipprBridge, NativeRipprBridge } from "./bridge";
import "./styles.css";

const bridge =
  import.meta.env.DEV && !("invoke" in window)
    ? new MockRipprBridge()
    : new NativeRipprBridge();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App bridge={bridge} />
  </StrictMode>,
);
