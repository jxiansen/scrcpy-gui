// The splashscreen is plain HTML (not processed by Vite), so it uses Tauri's
// internal invoke bridge. `withGlobalTauri` is intentionally off.
(async function () {
    try {
        const invoke = window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke;
        if (!invoke) return;
        const version = await invoke('get_app_version');
        document.getElementById('version-text').textContent = `VERSION ${version}`;
    } catch (error) {
        console.error('Failed to get version:', error);
    }
})();
