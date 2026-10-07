export type SpikeWindowOptions = {
  width: number;
  height: number;
  backgroundColor: string;
  webPreferences: {
    preload: string;
    nodeIntegration: false;
    contextIsolation: true;
    sandbox: true;
    webSecurity: true;
  };
};

export function createSpikeWindowOptions(preload: string): SpikeWindowOptions {
  return {
    width: 1280,
    height: 720,
    backgroundColor: "#00000000",
    webPreferences: {
      preload,
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
      webSecurity: true,
    },
  };
}

function hasExactOrigin(target: string, expectedOrigin: string): boolean {
  try {
    const targetUrl = new URL(target);
    const expectedUrl = new URL(expectedOrigin);
    if (targetUrl.protocol === "file:" || expectedUrl.protocol === "file:") {
      return targetUrl.href === expectedUrl.href;
    }
    return targetUrl.origin === expectedUrl.origin;
  } catch {
    return false;
  }
}

export function isAllowedSpikeNavigation(target: string, rendererOrigin: string): boolean {
  return hasExactOrigin(target, rendererOrigin);
}

export function isAllowedSpikeMediaUrl(target: string, serviceOrigin: string): boolean {
  return hasExactOrigin(target, serviceOrigin);
}
