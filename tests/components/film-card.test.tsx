import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { FilmCard } from "@/components/FilmCard";
import type { PanoramaFilm } from "@/runtime/types";

const film: PanoramaFilm = {
  id: "tt0074991",
  type: "movie",
  name: "Obsession",
  year: "1976",
  director: "Brian De Palma",
  originCountry: null,
  rating: null,
  ratingCount: null,
  posterUrl: "https://example.com/poster.jpg",
  landscapeUrl: "https://example.com/background.jpg",
  logoUrl: null,
  description: null,
};

describe("FilmCard artwork", () => {
  it("shows a shimmer while artwork is loading", () => {
    render(<FilmCard film={film} onOpen={vi.fn()} />);
    expect(screen.getByTestId("artwork-shimmer")).toBeInTheDocument();
    expect(screen.getByTestId("artwork-shimmer").parentElement).not.toHaveClass("bg-artwork-empty");
  });

  it("falls back to the poster when landscape artwork fails", () => {
    const { container } = render(<FilmCard film={film} onOpen={vi.fn()} />);
    const image = container.querySelector("img");
    expect(image).toHaveAttribute("src", film.landscapeUrl);
    fireEvent.error(image!);
    expect(container.querySelector("img")).toHaveAttribute("src", film.posterUrl);
    expect(screen.getByTestId("artwork-shimmer")).toBeInTheDocument();
  });

  it("hides the shimmer once artwork loads", () => {
    const { container } = render(<FilmCard film={film} onOpen={vi.fn()} />);
    fireEvent.load(container.querySelector("img")!);
    expect(screen.queryByTestId("artwork-shimmer")).not.toBeInTheDocument();
  });

  it("shows a foreign origin country between director and year", () => {
    const { getByText } = render(
      <FilmCard film={{ ...film, originCountry: "France" }} onOpen={vi.fn()} />,
    );
    const credits = getByText("Brian De Palma").parentElement;
    expect(credits?.textContent).toBe("Brian De PalmaFrance1976");
  });

  it("uses the empty artwork token when both images fail", () => {
    const { container } = render(<FilmCard film={film} onOpen={vi.fn()} />);
    fireEvent.error(container.querySelector("img")!);
    fireEvent.error(container.querySelector("img")!);
    expect(container.querySelector("img")).not.toBeInTheDocument();
    expect(screen.queryByTestId("artwork-shimmer")).not.toBeInTheDocument();
    expect(container.firstElementChild?.firstElementChild).toHaveClass("bg-artwork-empty");
  });
});
