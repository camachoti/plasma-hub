import {
  IconBell as TablerBell, IconCheck as TablerCheck, IconChevronLeft as TablerBack,
  IconDots as TablerMore, IconDownload as TablerDownload, IconLayoutSidebarRight as TablerPanel,
  IconLogout as TablerLogOut, IconMoodSmile as TablerEmoji, IconPaperclip as TablerAttach,
  IconPlus as TablerPlus, IconSearch as TablerSearch, IconSend as TablerSend,
  IconSettings as TablerSettings, IconSparkles as TablerMagic, IconTrash as TablerTrash,
} from "../../design-system/icons";

const icon = { stroke: 2 } as const;
export const IconSearch = () => <TablerSearch size={16} {...icon} />;
export const IconMore = () => <TablerMore size={18} {...icon} />;
export const IconPanel = () => <TablerPanel size={18} {...icon} />;
export const IconPlus = () => <TablerPlus size={18} {...icon} />;
export const IconBack = () => <TablerBack size={18} {...icon} />;
export const IconSend = () => <TablerSend size={16} {...icon} />;
export const IconAttach = () => <TablerAttach size={18} {...icon} />;
export const IconEmoji = () => <TablerEmoji size={18} {...icon} />;
export const IconSettings = () => <TablerSettings size={16} {...icon} />;
export const IconCheck = () => <TablerCheck size={18} {...icon} />;
export const IconBell = () => <TablerBell size={18} {...icon} />;
export const IconLogOut = () => <TablerLogOut size={18} {...icon} />;
export const IconTrash = () => <TablerTrash size={18} {...icon} />;
export const IconMagic = () => <TablerMagic size={18} {...icon} />;
export const IconDownload = () => <TablerDownload size={18} {...icon} />;
