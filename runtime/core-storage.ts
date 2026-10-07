export type CoreStorageBackend = {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
};

export type CoreStorage = CoreStorageBackend;

function asString(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value);
}

export function createCoreStorage(backend: CoreStorageBackend | null): CoreStorage {
  return {
    getItem(key) {
      try {
        if (!backend) return null;
        const value = backend.getItem(key);
        if (value == null) return null;
        return typeof value === "string" ? value : String(value);
      } catch {
        return null;
      }
    },
    setItem(key, value) {
      try {
        backend?.setItem(key, asString(value));
      } catch {
        return;
      }
    },
    removeItem(key) {
      try {
        backend?.removeItem(key);
      } catch {
        return;
      }
    },
  };
}

function nativeStorage(scope: { localStorage?: Storage }): CoreStorageBackend | null {
  try {
    const storage = scope.localStorage;
    if (!storage) return null;
    const getItem = storage.getItem.bind(storage);
    const setItem = storage.setItem.bind(storage);
    const removeItem = storage.removeItem.bind(storage);
    return {
      getItem: (key) => getItem(key),
      setItem: (key, value) => setItem(key, value),
      removeItem: (key) => removeItem(key),
    };
  } catch {
    return null;
  }
}

export function installCoreStorage(scope: { localStorage?: Storage }): CoreStorage {
  const adapter = createCoreStorage(nativeStorage(scope));
  const storage = (() => {
    try {
      return scope.localStorage ?? null;
    } catch {
      return null;
    }
  })();

  if (!storage) return adapter;

  try {
    Object.defineProperty(storage, "getItem", {
      configurable: true,
      writable: true,
      value: (key: string) => adapter.getItem(key),
    });
    Object.defineProperty(storage, "setItem", {
      configurable: true,
      writable: true,
      value: (key: string, value: string) => {
        adapter.setItem(key, value);
      },
    });
    Object.defineProperty(storage, "removeItem", {
      configurable: true,
      writable: true,
      value: (key: string) => {
        adapter.removeItem(key);
      },
    });
  } catch {
    return adapter;
  }

  return adapter;
}
