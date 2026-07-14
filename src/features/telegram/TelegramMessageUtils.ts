// @ts-nocheck

export function toNumberValue(value: any) {
  if (typeof value === 'number') return value;
  if (typeof value === 'bigint') return Number(value);
  if (value && typeof value.toJSNumber === 'function') return value.toJSNumber();
  if (value && typeof value.toString === 'function') return Number(value.toString());
  return 0;
}

export function sanitizeForFilename(value: any) {
  return String(value || 'Desconhecido')
    .replace(/[^\w.-]+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 160) || 'arquivo';
}

export function sanitizeForFolderName(value: any) {
  return String(value || 'Desconhecido')
    .replace(/[^\w-]/g, '_')
    .slice(0, 160) || 'Desconhecido';
}

export function sanitizeForAlbumFolderName(value: any) {
  return String(value || 'Album')
    .replace(/[\\/:*?"<>|\u0000-\u001F]/g, '_')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 120) || 'Album';
}

export function joinPath(...parts: string[]) {
  const filtered = parts.filter(Boolean);
  if (!filtered.length) return '';
  return filtered
    .map((part, index) => {
      if (index === 0) return part.replace(/\/+$/g, '');
      return part.replace(/^\/+|\/+$/g, '');
    })
    .join('/');
}

export function basename(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}

export function getMediaFileExtension(message: any) {
  if (message?.photo) return '.jpg';
  if (message?.video) return '.mp4';

  const fileExt = message?.file?.ext;
  if (fileExt) return fileExt.startsWith('.') ? fileExt : `.${fileExt}`;

  const mimeType = message?.document?.mimeType || '';
  if (mimeType === 'video/webm') return '.webm';
  if (mimeType === 'video/quicktime') return '.mov';
  if (mimeType.startsWith('video/')) return '.mp4';
  if (mimeType.startsWith('image/')) return '.jpg';
  if (mimeType.startsWith('audio/')) return '.mp3';
  return '.bin';
}

export function getMessageDownloadFilename(message: any) {
  let name = message?.file?.name || `media_${message?.id || 'file'}${getMediaFileExtension(message)}`;
  if (name.endsWith('.bin')) {
    const ext = getMediaFileExtension(message);
    if (ext !== '.bin') name = `${name.slice(0, -4)}${ext}`;
  }
  return sanitizeForFilename(name);
}

export function isDownloadableMedia(message: any) {
  if (!message || message.out) return false;
  if (message.photo || message.video) return true;
  const mime = message.document?.mimeType || '';
  return mime.startsWith('video/') || mime.startsWith('image/') || mime.startsWith('audio/');
}

export function getMessageText(message: any) {
  return String(message?.message || message?.text || '').trim();
}

export function getMessageGroupedId(message: any) {
  const groupedId = message?.groupedId;
  return groupedId == null ? null : String(groupedId);
}

export function messageCacheKey(chatId: string, topicId?: number) {
  return topicId ? `${chatId}::topic:${topicId}` : chatId;
}

export function isLikelyAlbumSeparator(message: any) {
  if (isDownloadableMedia(message)) return false;
  const text = getMessageText(message);
  if (!text) return false;
  if (/https?:\/\/|www\.|t\.me\//i.test(text)) return false;
  if (text.length > 48) return false;

  const compact = text.replace(/\s+/g, '');
  if (!compact) return false;

  const lettersOrNumbers = compact.match(/[\p{L}\p{N}]/gu)?.length || 0;
  const symbols = compact.length - lettersOrNumbers;

  if (lettersOrNumbers === 0) return true;
  if (compact.length <= 12 && symbols >= lettersOrNumbers) return true;
  return compact.length <= 16 && /^[\p{L}\p{N}\s._#-]+$/u.test(text);
}

export function formatTelegramMessage(m: any) {
  const getSenderName = () => {
    if (m.out) return null;
    const sender = m.sender;
    if (!sender) return null;
    if (sender.firstName || sender.lastName) {
      return [sender.firstName, sender.lastName].filter(Boolean).join(' ').trim();
    }
    return sender.title || sender.username || null;
  };

  const isVideo = !!(m.media?.document?.mimeType && typeof m.media.document.mimeType === 'string' && m.media.document.mimeType.includes('video'));
  const isPhoto = !!(m.media?.photo || m.media?.className === 'MessageMediaPhoto');
  const videoDuration = isVideo ? m.media?.document?.attributes?.find((a: any) => a.className === 'DocumentAttributeVideo')?.duration : null;
  const mediaSize = m.media?.document?.size || m.media?.photo?.sizes?.slice(-1)[0]?.size || null;
  const topicId = m.replyTo?.replyToTopId || m.replyTo?.replyToMsgId || null;

  return {
    id: m.id,
    message: m.message,
    date: m.date,
    out: m.out,
    senderId: m.senderId?.toString(),
    senderName: getSenderName(),
    replyToMsgId: m.replyTo?.replyToMsgId,
    topicId,
    media: m.media ? true : false,
    text: m.message,
    hasMedia: m.media ? true : false,
    isPhoto,
    isVideo,
    groupedId: m.groupedId ? m.groupedId.toString() : null,
    videoDuration,
    mediaSize,
    isDeleted: false,
  };
}

export function toUint8Array(data: any) {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  return new Uint8Array(data || []);
}

export function nextFrame() {
  return new Promise<void>(resolve => setTimeout(resolve, 0));
}
