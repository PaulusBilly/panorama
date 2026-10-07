import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { LoaderAnimationDrafts } from "@/app/loader/LoaderAnimationDrafts";

describe("Loader animation drafts", () => {
  it("renders three named directions and lets motion be paused", () => {
    render(<LoaderAnimationDrafts />);

    expect(screen.getByRole("region", { name: "Loader animation drafts" })).toBeVisible();
    expect(screen.getAllByRole("article")).toHaveLength(3);
    expect(screen.getByRole("heading", { name: "Cascade" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Frames" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Lockstep" })).toBeVisible();

    const pause = screen.getByRole("button", { name: "Pause animations" });
    fireEvent.click(pause);

    expect(screen.getByRole("button", { name: "Play animations" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("status")).toHaveTextContent("Animations paused");
  });
});
