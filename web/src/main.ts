import { mount } from 'svelte';
import { registerSW } from 'virtual:pwa-register';
import App from './App.svelte';

const app = mount(App, { target: document.getElementById('app')! });

// PWA: precaches the app shell, engine WASM and net weights so the game works
// offline. registerType is 'prompt' — on a new deploy we show a toast instead
// of reloading, so an update never interrupts a game in progress.
const updateSW = registerSW({
  onNeedRefresh() {
    if (document.getElementById('sw-update-toast')) return;
    const toast = document.createElement('div');
    toast.id = 'sw-update-toast';
    toast.style.cssText =
      'position:fixed;left:50%;bottom:16px;transform:translateX(-50%);z-index:9999;' +
      'display:flex;gap:12px;align-items:center;padding:10px 14px;border-radius:12px;' +
      'background:#3a3a3a;color:#f7f3ec;font:14px system-ui,sans-serif;' +
      'box-shadow:0 4px 16px rgba(0,0,0,.35)';
    toast.innerHTML =
      '<span>Доступна новая версия</span>' +
      '<button id="sw-update-go" style="border:0;border-radius:8px;padding:6px 12px;cursor:pointer;' +
      'background:#1f5f43;color:#fff;font:inherit">Обновить</button>' +
      '<button id="sw-update-later" style="border:0;background:none;color:#bbb;cursor:pointer;' +
      'font:inherit;padding:6px 4px">Позже</button>';
    document.body.appendChild(toast);
    document.getElementById('sw-update-go')!.addEventListener('click', () => updateSW(true));
    document.getElementById('sw-update-later')!.addEventListener('click', () => toast.remove());
  },
});

export default app;
