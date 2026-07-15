import { runtimeCapabilities } from "./runtime";
import { debugLog, debugWarn } from "../debug/logger";

export function canUseServiceWorker() {
  return runtimeCapabilities.supportsServiceWorker;
}

export function registerAppServiceWorker() {
  if (!canUseServiceWorker()) return;

  window.addEventListener("load", () => {
    navigator.serviceWorker.register("/sw.js").then(registration => {
      debugLog("SW registered: ", registration);
    }).catch(registrationError => {
      debugWarn("SW registration failed: ", registrationError);
    });
  });
}
