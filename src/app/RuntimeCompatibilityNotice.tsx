import { runtimeCapabilities } from "../shared/platform/runtime";

export function RuntimeCompatibilityNotice() {
  if (!runtimeCapabilities.isAndroid) return null;

  return (
    <div className="runtime-notice" role="status">
      Android preview: Telegram nativo, Twitter/X, yt-dlp e pastas do sistema ainda estão em modo desktop-first.
    </div>
  );
}
