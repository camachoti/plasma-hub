import React from 'react';
import type { Message, TimelineItem } from './TelegramDashboardTypes';

export const getTimelineItems = (msgs: Message[]): TimelineItem[] => {
  const items: TimelineItem[] = [];
  let currentAlbum: { id: string; messages: Message[] } | null = null;

  for (const m of msgs) {
    if (m.groupedId) {
      if (currentAlbum && currentAlbum.id === m.groupedId) {
        currentAlbum.messages.push(m);
      } else {
        if (currentAlbum) {
          items.push({
            type: 'album',
            id: currentAlbum.messages[0].id,
            message: currentAlbum.messages.find(msg => msg.text) || currentAlbum.messages[0],
            messages: [...currentAlbum.messages],
          });
        }
        currentAlbum = { id: m.groupedId, messages: [m] };
      }
    } else {
      if (currentAlbum) {
        items.push({
          type: 'album',
          id: currentAlbum.messages[0].id,
          message: currentAlbum.messages.find(msg => msg.text) || currentAlbum.messages[0],
          messages: [...currentAlbum.messages],
        });
        currentAlbum = null;
      }
      items.push({
        type: 'message',
        id: m.id,
        message: m,
      });
    }
  }

  if (currentAlbum) {
    items.push({
      type: 'album',
      id: currentAlbum.messages[0].id,
      message: currentAlbum.messages.find(msg => msg.text) || currentAlbum.messages[0],
      messages: [...currentAlbum.messages],
    });
  }

  return items;
};

export const formatBytes = (bytes: number) => {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
};

const topicColors = [
  '#5ca9e6, #7d95ff',
  '#e8786e, #f5a623',
  '#6ec6b8, #43a047',
  '#ab7ae6, #e66fa0',
  '#f5a623, #f7c948',
  '#5cb8e6, #4fc3f7',
  '#e66fa0, #ef5350',
  '#66bb6a, #aed581',
];

export const getTopicColor = (id: number) => topicColors[Math.abs(id) % topicColors.length];

export const ListContainer = React.forwardRef<HTMLDivElement, any>(({ style, children, ...props }, ref) => {
  const getPadding = (baseVal: any, extraPx: number) => {
    if (baseVal === undefined || baseVal === null) return `${extraPx}px`;
    if (typeof baseVal === 'number') return `${baseVal + extraPx}px`;
    return `calc(${baseVal} + ${extraPx}px)`;
  };

  return (
    <div
      {...props}
      ref={ref}
      style={{
        ...style,
        paddingTop: getPadding(style?.paddingTop, 18),
        paddingBottom: getPadding(style?.paddingBottom, 32),
      }}
    >
      {children}
    </div>
  );
});
