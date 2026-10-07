import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(cleanup);

HTMLDialogElement.prototype.showModal = function showModal() {
  this.open = true;
};

HTMLDialogElement.prototype.close = function close() {
  this.open = false;
};

// jsdom has no layout engine, so it ships no ResizeObserver; sizes never change there anyway.
globalThis.ResizeObserver ??= class ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
};
