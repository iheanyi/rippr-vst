import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "./App";
import { MockRipprBridge } from "./bridge";

afterEach(cleanup);

describe("rip request flow", () => {
  it("shows worker progress and makes a prepared sample active", async () => {
    const bridge = new MockRipprBridge();
    render(<App bridge={bridge} />);

    fireEvent.change(screen.getByLabelText("Public media URL"), {
      target: { value: "https://example.test/fixture" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Rip sample" }));

    expect(await screen.findByText("Fixture break")).toBeVisible();
    expect(screen.getByText("Ready to play")).toBeVisible();
    expect(screen.getByRole("button", { name: "Preview sample" })).toBeEnabled();
    await waitFor(() => expect(bridge.requests).toHaveLength(1));
  });

  it("browses cached samples and restores one without reacquiring it", async () => {
    const bridge = new MockRipprBridge();
    render(<App bridge={bridge} />);

    fireEvent.click(screen.getByRole("button", { name: "Library" }));

    expect(await screen.findByText("Cached break")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Load Cached break" }));
    expect(await screen.findByText("Ready to play")).toBeVisible();
    expect(bridge.requests).toHaveLength(0);
  });
});
