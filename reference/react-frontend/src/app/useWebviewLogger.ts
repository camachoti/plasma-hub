import { useEffect } from "react";
import { debugLog, isDebugLoggingEnabled } from "../shared/debug/logger";
import { canUseServiceWorker } from "../shared/platform/serviceWorker";

const SENSITIVE_FIELD = /^(?:authorization|cookie|cookies|api[_-]?key|api[_-]?hash|access[_-]?token|auth[_-]?token|refresh[_-]?token|session(?:id)?|ct0|phone(?:number)?|password|code|text|caption|message|content)$/i;
const MAX_LOG_MESSAGE_LENGTH = 4_000;

function redactString(value: string) {
  return value
    .replace(/(\bBearer\s+)[^\s]+/gi, '$1[redacted]')
    .replace(/([?&](?:access_token|auth_token|token|api_key|key|signature)=)[^&#\s]+/gi, '$1[redacted]')
    .replace(/(\b(?:auth_token|ct0|sessionid|session|password|code)=)[^;\s,]+/gi, '$1[redacted]');
}

function redactValue(value: unknown, seen = new WeakSet<object>()): unknown {
  if (typeof value === 'string') return redactString(value);
  if (!value || typeof value !== 'object') return value;
  if (seen.has(value)) return '[circular]';
  seen.add(value);
  if (Array.isArray(value)) return value.map(item => redactValue(item, seen));

  return Object.fromEntries(Object.entries(value).map(([key, item]) => [
    key,
    SENSITIVE_FIELD.test(key) ? '[redacted]' : redactValue(item, seen),
  ]));
}

function serializeLogArg(arg: unknown) {
  if (arg instanceof Error) return redactString(`${arg.message}\n${arg.stack || ''}`);
  if (typeof arg === "object") {
    try {
      return JSON.stringify(redactValue(arg));
    } catch {
      return redactString(String(arg));
    }
  }
  return redactString(String(arg));
}

export function useWebviewLogger() {
  useEffect(() => {
    if (!isDebugLoggingEnabled()) return;

    const sendLog = (level: string, ...args: unknown[]) => {
      const rawMessage = args.map(serializeLogArg).join(" ");
      const message = rawMessage.length > MAX_LOG_MESSAGE_LENGTH
        ? `${rawMessage.slice(0, MAX_LOG_MESSAGE_LENGTH)}… [truncated]`
        : rawMessage;

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

    debugLog("Webview logger initialized!");

    if (canUseServiceWorker()) {
      const swListener = (event: MessageEvent) => {
        sendLog("SW_MSG", event.data);
      };
      navigator.serviceWorker.addEventListener("message", swListener);
      debugLog("Service Worker message listener registered");
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
