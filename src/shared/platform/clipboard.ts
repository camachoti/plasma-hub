export function writeClipboardText(text: string) {
  return navigator.clipboard.writeText(text);
}
