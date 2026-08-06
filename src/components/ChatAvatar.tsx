import React, { useEffect, useState } from 'react';

import { telegramService } from '../features/telegram/TelegramService';

interface Props {
  chatId: string;
  title: string;
}

const avatarMemoryCache = new Map<string, string>();
const avatarPendingRequests = new Map<string, Promise<string | null>>();

const loadAvatar = async (chatId: string) => {
  const cached = avatarMemoryCache.get(chatId);
  if (cached) return cached;

  const pending = avatarPendingRequests.get(chatId);
  if (pending) return pending;

  const request = telegramService.getAvatar(chatId)
    .then(res => {
      if (res?.success && res.dataUrl) {
        avatarMemoryCache.set(chatId, res.dataUrl);
        return res.dataUrl;
      }
      return null;
    })
    .finally(() => avatarPendingRequests.delete(chatId));
  avatarPendingRequests.set(chatId, request);
  return request;
};

const ChatAvatarComponent: React.FC<Props> = ({ chatId, title }) => {
  const [imgData, setImgData] = useState<string | null>(null);
  
  useEffect(() => {
    let isMounted = true;
    const cached = avatarMemoryCache.get(chatId);
    setImgData(cached || null);

    const fetchAvatar = async () => {
      if (!chatId || typeof chatId !== 'string' || chatId.startsWith('invite_')) return;
      const dataUrl = await loadAvatar(chatId);
      if (isMounted && dataUrl) setImgData(dataUrl);
    };
    fetchAvatar();
    return () => {
      isMounted = false;
    };
  }, [chatId]);

  if (imgData) {
    return <img src={imgData} alt={title} className="chat-avatar-img" />;
  }

  return (
    <div className="chat-avatar-text">
       {title ? title.charAt(0).toUpperCase() : '?'}
    </div>
  );
};

export const ChatAvatar = React.memo(ChatAvatarComponent);
