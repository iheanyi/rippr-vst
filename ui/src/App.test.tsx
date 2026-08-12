import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { App } from "./App";
import { MockRipprBridge } from "./bridge";

afterEach(cleanup);

describe("rip request flow", () => {
  it("shows worker progress and makes a prepared sample active", async () => {
    const bridge = new MockRipprBridge();
    render(<App bridge={bridge} />);

    fireEvent.change(screen.getByLabelText("Media URL"), {
      target: { value: "https://example.test/fixture" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Rip sample" }));

    expect(await screen.findByText("Fixture break")).toBeVisible();
    expect(screen.getByText("Ready")).toBeVisible();
    expect(screen.getByRole("button", { name: "Preview sample" })).toBeEnabled();
    await waitFor(() => expect(bridge.requests).toHaveLength(1));
    expect(bridge.requests[0]).toEqual({ sourceUrl: "https://example.test/fixture" });
    expect(screen.queryByLabelText("Trim in")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Trim out")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Waveform for Fixture break")).toBeVisible();
    expect(screen.getByLabelText("Prepared WAV format")).toHaveTextContent(
      "32-bit float WAV",
    );
    expect(screen.getByLabelText("Prepared WAV format")).toHaveTextContent(
      "48 kHz · Stereo",
    );
  });

  it("lets the user choose the friendly WAV destination", async () => {
    const bridge = new MockRipprBridge();
    render(<App bridge={bridge} />);

    expect(await screen.findByText("/Users/fixture/Music/Rippr Samples")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Change" }));
    expect(await screen.findByText("/Users/fixture/Music/New Samples")).toBeVisible();
  });

  it("keeps a fast folder handoff result instead of overwriting it with a pending state", async () => {
    const bridge = new MockRipprBridge();
    render(<App bridge={bridge} />);

    fireEvent.change(screen.getByLabelText("Media URL"), {
      target: { value: "https://example.test/fixture" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Rip sample" }));
    await screen.findByText("WAV ready for your DAW.");

    fireEvent.click(screen.getByRole("button", { name: "Change" }));

    expect(
      await screen.findByText("The active WAV is ready in the new folder."),
    ).toBeVisible();
    expect(screen.queryByText(/Preparing the active WAV/)).not.toBeInTheDocument();
  });

  it("browses cached samples and restores one without reacquiring it", async () => {
    const bridge = new MockRipprBridge();
    render(<App bridge={bridge} />);

    fireEvent.click(screen.getByRole("button", { name: "Library" }));

    expect(await screen.findByText("Cached break")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Load Cached break" }));
    expect(await screen.findByText("Ready")).toBeVisible();
    expect(bridge.requests).toHaveLength(0);
  });

  it("starts the native WAV handoff from the current pointer gesture", async () => {
    const bridge = new MockRipprBridge();
    render(<App bridge={bridge} />);

    fireEvent.change(screen.getByLabelText("Media URL"), {
      target: { value: "https://example.test/fixture" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Rip sample" }));
    await screen.findByText("Ready");

    fireEvent.pointerDown(
      screen.getByRole("button", { name: "Drag WAV into your DAW" }),
      { button: 0 },
    );

    await waitFor(() => expect(bridge.dragStarts).toBe(1));
  });

  it("toggles preview playback off as well as on", async () => {
    const bridge = new MockRipprBridge();
    render(<App bridge={bridge} />);

    fireEvent.change(screen.getByLabelText("Media URL"), {
      target: { value: "https://example.test/fixture" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Rip sample" }));
    await screen.findByText("Ready");

    fireEvent.click(screen.getByRole("button", { name: "Preview sample" }));
    expect(await screen.findByRole("button", { name: "Stop preview" })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Stop preview" }));
    expect(await screen.findByRole("button", { name: "Preview sample" })).toBeVisible();
    expect(bridge.previewStarts).toBe(1);
    expect(bridge.previewStops).toBe(1);
  });
});
