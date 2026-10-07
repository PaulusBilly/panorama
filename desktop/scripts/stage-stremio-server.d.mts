export type StremioServerManifest = {
  schemaVersion: 1;
  version: string;
  source: { url: string; sha256: string };
};

export function validateStremioServerManifest(value: unknown): StremioServerManifest;
export function stageStremioServer(
  targetDirectory: string,
  projectRoot?: string,
  fetchSource?: typeof fetch,
): Promise<StremioServerManifest>;
