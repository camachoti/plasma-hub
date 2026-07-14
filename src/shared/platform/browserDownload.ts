export function downloadUrlInBrowser(url: string, filename: string, target?: string) {
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  if (target) anchor.target = target;
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
}
