import { runtimeCapabilities } from "../shared/platform/runtime";

export function RuntimeCompatibilityNotice() {
  if (!runtimeCapabilities.isAndroid) return null;

  return (
    <div className="runtime-notice" role="status">
      Telegram nativo em migração ativa. TDLib e downloads do Telegram estão habilitados; yt-dlp e Twitter/X ainda ficam no modo desktop.
    </div>
  );
}
