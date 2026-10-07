export type WindowsLibmpvManifest = {
  schemaVersion: 1;
  architecture: "x64";
  source: { url: string; sha256: string };
  files: { headers: string[]; runtime: string[]; rejectedLinkInputs: string[] };
  noticesDirectory: string;
};

export function validateWindowsLibmpvManifest(value: unknown): WindowsLibmpvManifest;
export function assertSafeArchiveMembers(members: string[]): void;
export function verifySha256(expected: string, actual: string): void;
export function assertPeX64(bytes: Uint8Array): void;
export function stageWindowsLibmpv(projectRoot?: string): Promise<{
  manifest: WindowsLibmpvManifest;
  immutableRoot: string;
  current: string;
}>;
