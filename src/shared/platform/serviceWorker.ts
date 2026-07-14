import { runtimeCapabilities } from "./runtime";

export function canUseServiceWorker() {
  return runtimeCapabilities.supportsServiceWorker;
}

export function registerAppServiceWorker() {
  if (!canUseServiceWorker()) return;

  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").then(registration => {
      console.log("SW registered: ", registration);
    }).catch(registrationError => {
      console.log("SW registration failed: ", registrationError);
    });
  });
}
