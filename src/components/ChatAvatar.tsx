import React, { useEffect, useState } from 'react';

import { telegramService } from '../features/telegram/TelegramService';

interface Props {
  chatId: string;
  title: string;
}

export const ChatAvatar: React.FC<Props> = ({ chatId, title }) => {
  const [imgData, setImgData] = useState<string | null>(null);
  
  useEffect(() => {
    setImgData(null);
    const fetchAvatar = async () => {
      if (!chatId || typeof chatId !== 'string' || chatId.startsWith('invite_')) return;
      const res = await telegramService.getAvatar(chatId);
      if (res && res.success && res.dataUrl) {
        setImgData(res.dataUrl);
      }
    };
    fetchAvatar();
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