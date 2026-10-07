import { expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

async function signInFromDetails(page: import("@playwright/test").Page) {
  await page.getByRole("button", { name: "Sign in to watch" }).click();
  const login = page.getByRole("dialog", { name: "Sign in to sync addons" });
  await login.getByLabel("Email").fill("viewer@example.com");
  await login.getByLabel("Password").fill("password");
  await login.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(login).not.toBeVisible();
}

test("renders the fixed catalog grid without horizontal overflow", async ({ page }) => {
  await page.setViewportSize({ width: 1640, height: 1024 });
  await page.goto("/");
  const cards = page.getByRole("article");
  await expect(cards).toHaveCount(9);
  const first = await cards.nth(0).boundingBox();
  const fifth = await cards.nth(4).boundingBox();
  expect(first?.width).toBe(372);
  expect((first?.width ?? 0) / (first?.height ?? 1)).toBeCloseTo(404 / 245, 2);
  expect(fifth?.y).toBeGreaterThan(first?.y ?? 0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(1640);
});

test("submits search with Enter and the arrow button", async ({ page }) => {
  await page.goto("/");
  const header = page.locator("header");
  const hero = page.getByRole("region", { name: "Featured film" });
  const closedLayout = {
    header: await header.boundingBox(),
    hero: await hero.boundingBox(),
  };
  await page.getByRole("button", { name: "Search" }).click();
  await expect.poll(async () => (await header.boundingBox())?.height ?? 0).toBeGreaterThan(closedLayout.header?.height ?? 0);
  await expect.poll(async () => (await hero.boundingBox())?.y ?? 0).toBeGreaterThan(closedLayout.hero?.y ?? 0);
  await page.waitForTimeout(400);
  const expandedHeaderHeight = await header.evaluate((element) => (element as HTMLElement).offsetHeight);
  await page.evaluate((top) => window.scrollTo(0, top), expandedHeaderHeight + 1);
  await expect(header).toHaveAttribute("inert", "");
  await page.evaluate(() => window.scrollBy(0, -1));
  await expect(header).not.toHaveAttribute("inert", "");
  await expect(header).toHaveClass(/bg-player-canvas/);
  const accessibility = await new AxeBuilder({ page }).analyze();
  expect(accessibility.violations.filter((violation) => violation.impact === "serious" || violation.impact === "critical")).toEqual([]);
  const search = page.getByRole("searchbox", { name: "Search movies" });
  await search.fill("Aftersun");
  await search.press("Enter");
  await expect(page).toHaveURL(/\/search\/films\?query=Aftersun$/);
  await expect(page.getByRole("region", { name: "Search results for Aftersun" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Featured film" })).toHaveCount(0);
  await expect(page.getByRole("tab", { name: "Films" })).toHaveAttribute("aria-selected", "true");
  await search.fill("Perfect Days");
  await page.getByRole("button", { name: "Submit search" }).click();
  await expect(page).toHaveURL(/\/search\/films\?query=Perfect\+Days$/);
  await expect(page.getByRole("region", { name: "Search results for Perfect Days" })).toBeVisible();
  const expandedResultsHeader = await header.boundingBox();
  await page.getByRole("button", { name: "Close search" }).click();
  await expect(page.getByRole("region", { name: "Search results for Perfect Days" })).toBeVisible();
  await expect.poll(async () => (await header.boundingBox())?.height ?? 0).toBeLessThan(expandedResultsHeader?.height ?? 0);
  await page.getByRole("button", { name: "Search" }).click();
  await expect(page.getByRole("searchbox", { name: "Search movies" })).toHaveValue("Perfect Days");
  await search.fill("Godard");
  await search.press("Enter");
  await page.getByRole("tab", { name: "Cast & Crew" }).click();
  await expect(page).toHaveURL(/\/search\/people\?query=Godard$/);
  await expect(page.getByTestId("people-grid").getByRole("article")).toHaveCount(6);
  await expect(page.getByRole("heading", { name: "Jean-Luc Godard" })).toBeVisible();
  const resultsAccessibility = await new AxeBuilder({ page }).analyze();
  expect(resultsAccessibility.violations.filter((violation) => violation.impact === "serious" || violation.impact === "critical")).toEqual([]);
  await page.getByRole("button", { name: "Close search" }).click();
  await page.getByRole("link", { name: "Panorama home" }).click();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole("region", { name: "Popular films" })).toBeVisible();
  await page.goBack();
  await expect(page).toHaveURL(/\/search\/people\?query=Godard$/);
  await expect(page.getByRole("region", { name: "Search results for Godard" })).toBeVisible();
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByTestId("people-grid")).toHaveCSS("grid-template-columns", /.+ .+/);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
});

test("keeps revealed header controls above film cards", async ({ page }) => {
  await page.goto("/");
  const header = page.locator("header");
  const grid = page.getByTestId("film-grid");
  await expect(grid).toBeVisible();
  await page.evaluate(() => window.scrollTo(0, document.querySelector<HTMLElement>("[data-testid='film-grid']")?.offsetTop ?? 0));
  await expect(header).toHaveAttribute("inert", "");
  await page.evaluate(() => window.scrollBy(0, -1));
  await expect(header).not.toHaveAttribute("inert", "");

  await page.getByRole("button", { name: "Search" }).click();
  await expect(page.getByRole("searchbox", { name: "Search movies" })).toBeVisible();
  await page.evaluate(() => (document.querySelector<HTMLElement>(".app-viewport") ?? window).scrollBy(0, 20));

  await page.waitForTimeout(400);
  await expect(header).not.toHaveAttribute("inert", "");
  await expect(grid).toBeVisible();
  await expect(page).toHaveURL(/\/$/);
});

test("restores direct searches and canonicalizes invalid query routes", async ({ page }) => {
  await page.goto("/search/people?query=%20Godard%20&query=Ignored");
  await expect(page).toHaveURL(/\/search\/people\?query=Godard$/);
  await expect(page.getByRole("searchbox", { name: "Search movies" })).toHaveValue("Godard");
  await expect(page.getByRole("tab", { name: "Cast & Crew" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("heading", { name: "Jean-Luc Godard" })).toBeVisible();

  await page.goto("/search/films?query=%20%20");
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByRole("region", { name: "Popular films" })).toBeVisible();

  const response = await page.goto("/search/lists?query=Godard");
  expect(response?.status()).toBe(404);
});

test("opens the account burger with the matching header icon size", async ({ page }) => {
  await page.goto("/");
  const header = page.locator("header");
  const searchIcon = page.getByRole("button", { name: "Search" }).locator("svg");
  const accountMenu = page.getByRole("button", { name: "Account menu" });
  const accountIcon = accountMenu.locator("svg");
  const [searchIconBox, accountIconBox] = await Promise.all([searchIcon.boundingBox(), accountIcon.boundingBox()]);
  expect(accountIconBox?.width).toBe(searchIconBox?.width);
  expect(accountIconBox?.height).toBe(searchIconBox?.height);

  await accountMenu.click();
  const accountPopover = page.locator("#account-menu");
  await expect(page.getByRole("button", { name: "Log In" })).toBeVisible();
  await expect(accountMenu).toHaveAttribute("aria-expanded", "true");
  await expect(accountPopover).toHaveCSS("opacity", "1");
  expect(await accountPopover.evaluate((element) => getComputedStyle(element).transitionProperty)).toContain("opacity");
  expect(await accountPopover.evaluate((element) => getComputedStyle(element).transitionProperty)).toContain("transform");
  await page.mouse.wheel(0, 240);
  await expect(accountMenu).toHaveAttribute("aria-expanded", "false");
  await expect(accountPopover).toBeHidden({ timeout: 100 });
  await expect(header).toHaveAttribute("inert", "");
  await page.mouse.wheel(0, -1);
  await expect(header).not.toHaveAttribute("inert", "");

  await page.evaluate(() => window.scrollTo(0, 0));
  await expect(header).not.toHaveAttribute("inert", "");
  await accountMenu.click();
  await page.getByRole("button", { name: "Log In" }).click();
  const login = page.getByRole("dialog", { name: "Sign in to sync addons" });
  await login.getByLabel("Email").fill("viewer@example.com");
  await login.getByLabel("Password").fill("password");
  await login.getByRole("button", { name: "Sign in", exact: true }).click();
  await expect(login).not.toBeVisible();

  await accountMenu.click();
  await expect(page.getByRole("button", { name: "Log Out" })).toBeVisible();
});

test("navigates from the hero Watch CTA and cards while the hero background stays non-interactive", async ({ page }) => {
  await page.goto("/");
  const hero = page.getByRole("region", { name: "Featured film" });
  const watch = hero.getByRole("button", { name: "WATCH Aftersun" });
  await expect(hero.getByRole("button")).toHaveCount(1);
  await expect(page.getByRole("button", { name: "Search" })).toHaveCSS("cursor", "pointer");
  await expect(page.getByRole("link", { name: "Panorama home" })).toHaveCSS("cursor", "pointer");
  await expect(watch).toHaveCSS("cursor", "pointer");
  const watchBackground = await watch.evaluate((element) => getComputedStyle(element).backgroundColor);
  await watch.hover();
  await expect.poll(() => watch.evaluate((element) => getComputedStyle(element).backgroundColor)).not.toBe(watchBackground);
  await watch.click();
  await expect(page).toHaveURL(/\/films\/100\/watch$/);
  await expect(page.getByRole("button", { name: "Sign in to watch" })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole("link", { name: "Panorama home" })).toHaveAttribute("href", "/");
  await page.getByRole("button", { name: "Open details for Aftersun" }).first().click();
  await expect(page).toHaveURL(/\/films\/100$/);
  await expect(page.getByRole("heading", { name: "Aftersun" })).toBeVisible();
  await expect(page.getByLabel("TMDB score 7.6 out of 10 from 2,485 ratings")).toBeVisible();
  await page.goBack();
  await expect(page).toHaveURL(/\/$/);
  await expect(page.getByTestId("film-grid")).toBeVisible();
});

test("renders the aligned synopsis heading", async ({ page }) => {
  await page.goto("/films/100");
  const synopsisHeading = page.getByRole("heading", { name: "Synopsis" });
  await expect(synopsisHeading).toHaveCSS("font-size", "14px");
  await expect(synopsisHeading).toHaveCSS("font-weight", "700");
  await expect(synopsisHeading).toHaveCSS("line-height", "14px");
});

test("places quality and plain-text duration below the genre", async ({ page }) => {
  await page.goto("/films/100");
  await signInFromDetails(page);
  const genre = page.getByText("Drama", { exact: true });
  const quality = page.getByLabel("Quality: HD");
  const audioChannels = page.getByLabel("Audio: 5.1");
  const duration = page.getByText("102 min", { exact: true });
  await expect(quality).toContainText("HD");
  await expect(quality.locator("svg")).toHaveCount(0);
  await expect(audioChannels).toBeVisible();
  await expect(audioChannels).toContainText("5.1");
  await expect(duration.locator("svg")).toHaveCount(0);
  const positions = await Promise.all([
    genre.evaluate((element) => element.getBoundingClientRect().bottom),
    quality.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      return { top: rect.top, center: rect.top + rect.height / 2 };
    }),
    duration.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      return rect.top + rect.height / 2;
    }),
  ]);
  expect(positions[1].top).toBeGreaterThan(positions[0]);
  expect(Math.abs(positions[1].center - positions[2])).toBeLessThanOrEqual(1);
});

test("keeps long title sizing independent from the bottom information columns", async ({ page }) => {
  await page.goto("/films/100");
  const title = page.locator("#film-title");
  const synopsis = page.locator("#synopsis-title").locator("..");
  const synopsisWidthBefore = await synopsis.evaluate((section) => section.getBoundingClientRect().width);
  await title.evaluate((heading) => {
    heading.textContent = "Jeanne Dielman, 23, quai du Commerce, 1080 Bruxelles Jeanne Dielman, 23, quai du Commerce, 1080 Bruxelles";
  });
  const measurements = await title.evaluate((heading) => {
    const synopsis = document.querySelector("#synopsis-title")?.parentElement;
    if (!(synopsis instanceof HTMLElement)) throw new Error("Missing synopsis section");
    const titleRect = heading.getBoundingClientRect();
    return {
      titleWidth: titleRect.width,
      titleHeight: titleRect.height,
      titleLineHeight: Number.parseFloat(getComputedStyle(heading).lineHeight),
      synopsisWidth: synopsis.getBoundingClientRect().width,
    };
  });

  expect(measurements.titleWidth).toBeGreaterThan(650);
  expect(measurements.titleHeight).toBeGreaterThan(measurements.titleLineHeight);
  expect(Math.abs(measurements.synopsisWidth - synopsisWidthBefore)).toBeLessThanOrEqual(1);
});

test("keeps the information columns stable while multiple directors wrap", async ({ page }) => {
  await page.goto("/films/100");
  const director = page.locator("p").filter({ hasText: /^Directed by / }).first();
  const directorNames = director.locator("strong");
  const synopsis = page.locator("#synopsis-title").locator("..");
  const informationPanel = synopsis.locator("..").locator(":scope > :first-child");
  const before = {
    directorHeight: await director.evaluate((element) => element.getBoundingClientRect().height),
    informationWidth: await informationPanel.evaluate((element) => element.getBoundingClientRect().width),
    synopsisWidth: await synopsis.evaluate((element) => element.getBoundingClientRect().width),
  };
  await expect(informationPanel).toHaveCSS("padding-right", "0px");
  expect(before.informationWidth).toBe(310);
  const informationGap = await informationPanel.evaluate((panel) => {
    const synopsis = panel.nextElementSibling;
    if (!(synopsis instanceof HTMLElement)) throw new Error("Missing synopsis section");
    return synopsis.getBoundingClientRect().left - panel.getBoundingClientRect().right;
  });
  expect(informationGap).toBe(70);
  await directorNames.evaluate((element) => {
    element.textContent = "Christopher Nolan, Asdsadas Example, Another Director, One More Filmmaker";
  });
  const after = {
    directorHeight: await director.evaluate((element) => element.getBoundingClientRect().height),
    informationWidth: await informationPanel.evaluate((element) => element.getBoundingClientRect().width),
    synopsisWidth: await synopsis.evaluate((element) => element.getBoundingClientRect().width),
  };

  expect({ informationWidth: after.informationWidth, synopsisWidth: after.synopsisWidth }).toEqual({
    informationWidth: before.informationWidth,
    synopsisWidth: before.synopsisWidth,
  });
  expect(after.directorHeight).toBeGreaterThan(before.directorHeight);
});

test("keeps the foreign title with the film title while aligning director and synopsis", async ({ page }) => {
  await page.goto("/films/101");
  const englishTitle = page.getByRole("heading", { name: "Perfect Days" });
  const nativeTitle = page.getByText("パーフェクト・デイズ");
  const synopsisHeading = page.getByRole("heading", { name: "Synopsis" });
  const director = page.locator("p").filter({ hasText: /^Directed by / }).first();

  await expect(nativeTitle).toHaveCSS("font-size", "16px");
  await expect(nativeTitle).toHaveCSS("margin-top", "4px");
  expect(await nativeTitle.evaluate((title) => title.parentElement?.querySelector("#film-title") !== null)).toBe(true);
  expect(await nativeTitle.evaluate((title) => getComputedStyle(title).color)).toBe(await synopsisHeading.evaluate((heading) => getComputedStyle(heading).color));
  const titleToInformationGap = await englishTitle.evaluate((title) => {
    const titleGroup = title.parentElement;
    const informationRow = titleGroup?.nextElementSibling;
    if (!(titleGroup instanceof HTMLElement) || !(informationRow instanceof HTMLElement)) throw new Error("Missing title or information row");
    return informationRow.getBoundingClientRect().top - titleGroup.getBoundingClientRect().bottom;
  });
  expect(titleToInformationGap).toBeCloseTo(20, 4);
  const tops = await Promise.all([
    director.evaluate((element) => element.getBoundingClientRect().top),
    synopsisHeading.evaluate((element) => element.getBoundingClientRect().top),
  ]);
  expect(Math.abs(tops[0] - tops[1])).toBeLessThanOrEqual(1);
  await expect(englishTitle).toBeVisible();
});

test("automatically prepares a source and opens the watch page", async ({ page }) => {
  await page.goto("/films/100");
  await signInFromDetails(page);
  await expect(page.getByText("HD")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Sources" })).toHaveCount(0);
  await page.getByRole("button", { name: "Check service" }).click();
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await expect(page).toHaveURL(/\/films\/100\/watch$/);
  await expect(page.getByText("Playing Aftersun")).toBeAttached();
  await page.getByRole("button", { name: "Sources" }).click();
  const sources = page.locator("#player-sources");
  await expect(sources).toBeVisible();
  await expect(sources.getByText("1080p")).toBeVisible();
});

test("returns to details and resumes saved playback", async ({ page }) => {
  await page.goto("/films/100");
  await signInFromDetails(page);
  await page.getByRole("button", { name: "Check service" }).click();
  await page.getByRole("button", { name: "Play", exact: true }).click();
  const position = page.getByRole("slider", { name: "Playback position" });
  await expect(position).toBeVisible();
  await page.locator(".player-video").click();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowRight");
  await expect(position).toHaveValue("30");
  await page.getByRole("button", { name: "Close" }).click();
  await expect(page).toHaveURL(/\/films\/100$/);
  const resume = page.getByRole("button", { name: /^Resume(?:,.*)?$/ });
  await expect(resume).toBeVisible();
  await resume.click();
  await expect(page).toHaveURL(/\/films\/100\/watch\?resume=1$/);
  await expect(position).toHaveValue("30");
});

test("supports direct, invalid, mobile, zoom, and reduced-motion visits", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 320, height: 800 });
  await page.goto("/films/101");
  await expect(page.getByText("パーフェクト・デイズ")).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(320);
  expect(await page.evaluate(() => matchMedia("(prefers-reduced-motion: reduce)").matches)).toBe(true);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.evaluate(() => { document.documentElement.style.zoom = "2"; });
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(1440);
  await page.goto("/films/not-a-number");
  await expect(page.getByRole("heading", { name: "This page could not be found." })).toBeVisible();
});

test("has no serious accessibility violations on details or watch", async ({ page }) => {
  await page.goto("/films/100");
  await expect(page.getByRole("heading", { name: "Aftersun" })).toBeVisible();
  let results = await new AxeBuilder({ page }).analyze();
  expect(results.violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
  await signInFromDetails(page);
  await page.getByRole("button", { name: "Check service" }).click();
  await page.getByRole("button", { name: "Play", exact: true }).click();
  await expect(page).toHaveURL(/\/films\/100\/watch$/);
  await expect(page).toHaveTitle("Panorama");
  results = await new AxeBuilder({ page }).analyze();
  expect(results.violations.filter((violation) => ["serious", "critical"].includes(violation.impact ?? ""))).toEqual([]);
});
