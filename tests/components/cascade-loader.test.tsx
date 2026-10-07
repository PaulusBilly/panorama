import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CascadeLoader } from "@/components/CascadeLoader";

describe("CascadeLoader", () => {
  it("renders the selected four-bar Cascade mark without visible copy", () => {
    render(<CascadeLoader />);

    const loader = screen.getByTestId("cascade-loader");
    expect(loader).toHaveClass("cascade-loader");
    expect(loader.querySelectorAll(".cascade-loader__bar")).toHaveLength(4);
    expect(loader).toHaveTextContent("");
  });
});
