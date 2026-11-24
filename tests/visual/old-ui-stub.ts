// Temporary IPC stub so the old UI can render in a plain browser for visual diffing.
const devices = ['706fc5a1'];
const deviceModels: Record<string, string> = { '706fc5a1': 'PJA110' };

const responses: Record<string, unknown> = {
  get_version: '4.2.0',
  'plugin:app|version': '4.2.0',
  'plugin:app|name': 'ScrcpyGUI',
  'plugin:app|identifier': 'com.kb.scrcpy-gui-v3',
  check_scrcpy: { found: true, message: 'Scrcpy Ready' },
  get_devices: { error: false, devices, deviceModels },
  get_mdns_devices: {
    error: false,
    services: [{
      name: 'adb-616f16f3c-YnOIRI',
      service: '_adb-tls-connect._tcp',
      address: '172.16.1.145:38517',
    }],
  },
  adb_shell: { success: true, output: 'PJA110' },
  get_render_drivers: {
    hostOs: 'windows',
    supportsRenderDriver: true,
    supportedDrivers: [],
    supportsHwdec: false,
  },
  check_scrcpy_update: { update_available: false },
  get_videos_dir: 'C:\\Users\\test\\Videos',
  get_scrcpy_bin_dir: 'C:\\Users\\test\\scrcpy-bin',
};

let callbackId = 0;
const callbacks = new Map<number, (e: unknown) => void>();

(window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
  unregisterListener: () => undefined,
};

(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
  metadata: {
    currentWindow: { label: 'main' },
    currentWebview: { label: 'main' },
  },
  invoke: (cmd: string, args: Record<string, unknown> = {}) => {
    if (cmd === 'plugin:event|listen') {
      const id = ++callbackId;
      callbacks.set(id, args.handler as (e: unknown) => void);
      return Promise.resolve(id);
    }
    if (cmd === 'plugin:event|unlisten') return Promise.resolve(undefined);
    if (cmd === 'plugin:window|current_window') return Promise.resolve('main');
    return Promise.resolve(responses[cmd] ?? {});
  },
  transformCallback: (fn: (e: unknown) => void) => {
    const id = ++callbackId;
    callbacks.set(id, fn);
    return id;
  },
};
