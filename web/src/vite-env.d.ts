/// <reference types="svelte" />
/// <reference types="vite/client" />

declare module '*.wasm?url' {
  const url: string;
  export default url;
}

// injected by Vite `define` from package.json — see vite.config.ts
declare const __APP_VERSION__: string;
