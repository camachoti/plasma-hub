export interface Chat {
  id: string;
  title: string;
  isGroup: boolean;
  isChannel: boolean;
  isMember?: boolean;
  isInvite?: boolean;
  inviteHash?: string;
  about?: string;
  participantsCount?: number;
  hasTopics?: boolean;
  isFakeTwitter?: boolean;
  lastMessageText?: string;
  lastMessageDate?: number;
  unreadCount?: number;
  lastMessageHasMedia?: boolean;
  lastMessageIsVideo?: boolean;
  lastMessageIsPhoto?: boolean;
}

export interface ChatFullInfo {
  about?: string;
  participantsCount?: number;
  username?: string | null;
  pinnedMsgId?: number | null;
}

export interface ForumTopic {
  id: number;
  title: string;
  topMessageId: number;
  unreadCount: number;
  closed: boolean;
  pinned: boolean;
}

export interface Message {
  id: number;
  text: string;
  date: number;
  out: boolean;
  senderId: string | null;
  senderName?: string | null;
  hasMedia: boolean;
  isPhoto: boolean;
  isVideo: boolean;
  videoDuration?: number | null;
  mediaSize?: number | null;
  reactions?: Array<{ emoji: string; count: number; mine: boolean }>;
  is_deleted?: boolean;
  is_edited?: boolean;
  replyToMsgId?: number | null;
  groupedId?: string | null;
  topicId?: number | null;
}

export interface TimelineItem {
  type: 'message' | 'album';
  id: number;
  message: Message;
  messages?: Message[];
}
