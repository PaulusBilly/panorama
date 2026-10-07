import { describe, expect, it } from "vitest";
import { isAllowedServiceEndpoint } from "@/runtime/snapshot";

describe("service endpoint validation", () => {
  it.each([
    "http://127.0.0.1:11470",
    "http://localhost:11470/",
    "http://[::1]:11470",
  ])("accepts loopback HTTP endpoint %s", (endpoint) => {
    expect(isAllowedServiceEndpoint(endpoint)).toBe(true);
  });

  it.each([
    "https://127.0.0.1:11470",
    "http://127.0.0.1.example.com:11470",
    "http://192.168.1.12:11470",
    "javascript:alert(1)",
    "not-a-url",
  ])("rejects non-local endpoint %s", (endpoint) => {
    expect(isAllowedServiceEndpoint(endpoint)).toBe(false);
  });
});
