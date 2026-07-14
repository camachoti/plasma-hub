import { runtimeCapabilities } from "../shared/platform/runtime";

export function RuntimeCompatibilityNotice() {
  if (!runtimeCapabilities.isAndroid) return null;

  return (
    <div className="runtime-notice" role="status">
      Telegram via web ativo. Downloads nativos, yt-dlp e pastas do sistema ainda estão em modo desktop-first.
    </div>
  );
}
