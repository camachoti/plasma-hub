import { useEffect } from "react";
import { canUseServiceWorker } from "../shared/platform/serviceWorker";

function serializeLogArg(arg: unknown) {
  if (arg instanceof Error) return `${arg.message}\n${arg.stack}`;
  if (typeof arg === "object") {
    try {
      return JSON.stringify(arg);
    } catch {
      return String(arg);
    }
  }
  return String(arg);
}

export function useWebviewLogger() {
  useEffect(() => {
    const sendLog = (level: string, ...args: unknown[]) => {
      const message = args.map(serializeLogArg).join(" ");

      fetch("http://127.0.0.1:1425/log", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          time: new Date().toLocaleTimeString(),
          level,
          message,
        }),
      }).catch(() => {});
    };

    const originalLog = console.log;
    const originalWarn = console.warn;
    const originalError = console.error;

    console.log = (...args) => {
      originalLog(...args);
      sendLog("LOG", ...args);
    };
    console.warn = (...args) => {
      originalWarn(...args);
      sendLog("WARN", ...args);
    };
    console.error = (...args) => {
      originalError(...args);
      sendLog("ERROR", ...args);
    };

    console.log("Webview logger initialized!");

    if (canUseServiceWorker()) {
      const swListener = (event: MessageEvent) => {
        sendLog("SW_MSG", event.data);
      };
      navigator.serviceWorker.addEventListener("message", swListener);
      console.log("Service Worker message listener registered");
      return () => {
        console.log = originalLog;
        console.warn = originalWarn;
        console.error = originalError;
        navigator.serviceWorker.removeEventListener("message", swListener);
      };
    }

    return () => {
      console.log = originalLog;
      console.warn = originalWarn;
      console.error = originalError;
    };
  }, []);
}
