import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { CastCrewCard } from "@/components/CastCrewCard";

const person = {
  id: "tmdb-person:240",
  name: "Jean-Luc Godard",
  department: "Directing",
  profileUrl: "https://image.tmdb.org/t/p/w500/godard.jpg",
};

describe("CastCrewCard", () => {
  it("renders a non-interactive portrait directory entry", () => {
    const { container } = render(<CastCrewCard person={person} />);
    expect(screen.getByRole("article")).toBeVisible();
    expect(screen.getByRole("heading", { name: "Jean-Luc Godard" })).toBeVisible();
    expect(screen.getByText("Directing")).toBeVisible();
    expect(container.querySelector("img")).toHaveClass("grayscale", "object-cover");
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("uses the missing-artwork surface after a portrait fails", () => {
    const { container } = render(<CastCrewCard person={person} />);
    const portrait = container.querySelector("img");
    expect(portrait).not.toBeNull();
    fireEvent.error(portrait!);
    expect(container.querySelector("img")).not.toBeInTheDocument();
    expect(container.querySelector(".bg-artwork-empty")).toBeInTheDocument();
  });

  it("omits an unavailable department without inventing copy", () => {
    render(<CastCrewCard person={{ ...person, department: null, profileUrl: null }} />);
    expect(screen.getByRole("heading", { name: "Jean-Luc Godard" })).toBeVisible();
    expect(screen.queryByText("Directing")).not.toBeInTheDocument();
    expect(document.querySelector("img")).not.toBeInTheDocument();
  });
});
