import type { JSX } from "solid-js";

function Svg(props: { children: JSX.Element; size?: number }) {
  const size = props.size ?? 16;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      stroke-width="1.4"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      {props.children}
    </svg>
  );
}

export const PlusIcon = () => <Svg><path d="M8 3v10M3 8h10" /></Svg>;
export const ChatsIcon = () => <Svg><path d="M3 4h10M3 8h10M3 12h6" /></Svg>;
export const SearchIcon = () => <Svg><circle cx="7" cy="7" r="4" /><path d="m10 10 3 3" /></Svg>;
export const SettingsIcon = () => <Svg><path d="M3 5h5M11 5h2M3 11h2M8 11h5" /><circle cx="9.5" cy="5" r="1.5" /><circle cx="6.5" cy="11" r="1.5" /></Svg>;
export const MicrophoneIcon = () => <Svg><rect x="5.75" y="1.75" width="4.5" height="8" rx="2.25" /><path d="M3 7.5a5 5 0 0 0 10 0M8 12.5v2" /></Svg>;
export const FolderIcon = () => <Svg size={14}><path d="M2 4.5a1 1 0 0 1 1-1h3l1.5 1.5H13a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z" /></Svg>;
export const ArrowUpIcon = () => <Svg><path d="M8 13V3M4 7l4-4 4 4" /></Svg>;
export const StopIcon = () => <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><rect x="1.5" y="1.5" width="9" height="9" rx="1.5" fill="currentColor" /></svg>;
export const CopyIcon = () => <Svg size={14}><rect x="5.5" y="5.5" width="8" height="8" rx="1.5" /><path d="M10.5 5.5v-2a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2" /></Svg>;
export const CheckIcon = () => <Svg size={14}><path d="m3 8.5 3.2 3L13 4.5" /></Svg>;
export const RetryIcon = () => <Svg size={14}><path d="M13 8a5 5 0 1 1-1.5-3.5M13 2v3h-3" /></Svg>;
export const EditIcon = () => <Svg size={14}><path d="M10.5 3 13 5.500 6 12.500l-3 .5.5-3z" /></Svg>;
export const ChevronIcon = () => <Svg size={12}><path d="m6 4 4 4-4 4" /></Svg>;
export const ChevronDownIcon = () => <Svg size={12}><path d="m4 6 4 4 4-4" /></Svg>;
export const ArrowDownIcon = () => <Svg size={14}><path d="M8 3v10M4 9l4 4 4-4" /></Svg>;
export const CloseIcon = () => <Svg size={14}><path d="m4 4 8 8M12 4l-8 8" /></Svg>;
export const PinIcon = () => <Svg size={14}><path d="M6 2.500h4l-.5 4 2 2v1h-7v-1l2-2zM8 9.500V14" /></Svg>;
export const TrashIcon = () => <Svg size={14}><path d="M3 4.500h10M6.500 4.500V3h3v1.500M4.500 4.500l.5 8.500h6l.5-8.500" /></Svg>;
export const ForkIcon = () => <Svg size={14}><circle cx="4.5" cy="3.5" r="1.5" /><circle cx="11.5" cy="3.5" r="1.5" /><circle cx="8" cy="12.5" r="1.5" /><path d="M4.5 5v1a2 2 0 0 0 2 2h3a2 2 0 0 0 2-2V5M8 8v3" /></Svg>;
export const SyncIcon = () => <Svg size={14}><path d="M13 8a5 5 0 0 1-8.700 3.400M3 8a5 5 0 0 1 8.700-3.400M11.500 2v2.800H8.700M4.500 14v-2.800h2.800" /></Svg>;
export const RewindIcon = () => <Svg size={14}><path d="M3 3v10M13 8H6M9 5 6 8l3 3" /></Svg>;
export const ExportIcon = () => <Svg size={14}><path d="M8 10V2.500M5 5.500l3-3 3 3M3 10v3h10v-3" /></Svg>;
export const CompareIcon = () => <Svg size={14}><rect x="2.500" y="3" width="4.500" height="10" rx="1" /><rect x="9" y="3" width="4.500" height="10" rx="1" /></Svg>;
export const ToolIcon = () => <Svg size={13}><path d="m3 11 4-3-4-3M8.500 12H13" /></Svg>;
export const FileIcon = () => <Svg size={13}><path d="M4 2.500h5l3 3v8H4zM9 2.500v3h3" /></Svg>;
export const ImportIcon = () => <Svg size={14}><path d="M8 2.500V10M5 7l3 3 3-3M3 13h10" /></Svg>;
export const PanelIcon = () => <Svg><rect x="2" y="3" width="12" height="10" rx="1.5" /><path d="M10 3v10" /></Svg>;
export const BackIcon = () => <Svg size={14}><path d="M10 3 5 8l5 5" /></Svg>;
export const ForwardIcon = () => <Svg size={14}><path d="m6 3 5 5-5 5" /></Svg>;
export const FolderOpenIcon = () => <Svg size={13}><path d="M2 4.5a1 1 0 0 1 1-1h3l1.5 1.5H13a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z" /></Svg>;
export const PlanTabIcon = () => <Svg size={18}><rect x="2" y="2" width="12" height="12" rx="3.2" /><path d="M5.3 5.7h.01M5.3 8h.01M5.3 10.3h.01M7.5 5.7h3.2M7.5 8h3.2M7.5 10.3h2" /></Svg>;
export const TerminalTabIcon = () => <Svg size={18}><path d="M3 4.6 6.4 8 3 11.4M8.6 11.4H13" /></Svg>;
export const ChangesTabIcon = () => <Svg size={18}><rect x="2" y="2" width="12" height="12" rx="3.2" /><path d="M8 4.7v3.4M6.3 6.4h3.4M6.3 10.8h3.4" /></Svg>;
export const BrowserTabIcon = () => <Svg size={18}><circle cx="8" cy="8" r="6" /><path d="M8 2c-1.7 1.6-2.5 3.6-2.5 6s.8 4.4 2.5 6M8 2c1.7 1.6 2.5 3.6 2.5 6s-.8 4.4-2.5 6M2.3 6h11.4M2.3 10h11.4" /></Svg>;
export const FilesTabIcon = () => <Svg size={18}><path d="M2 5.2c0-.9.7-1.6 1.6-1.6h2.2c.4 0 .8.2 1.1.5L8 5.2h4.4c.9 0 1.6.7 1.6 1.6v4.6c0 .9-.7 1.6-1.6 1.6H3.6c-.9 0-1.6-.7-1.6-1.6z" /></Svg>;
export const TasksTabIcon = () => <Svg size={18}><path d="M1.8 8.4h2.4l1.7-4 3.3 7.4 1.7-3.4h3.3" /></Svg>;
export const ArtifactsTabIcon = () => <Svg size={18}><path d="M8 2.3 10.9 7H5.1z" /><rect x="2.3" y="9" width="4.7" height="4.7" rx="1.2" /><circle cx="11.35" cy="11.35" r="2.35" /></Svg>;
export const ChartKindIcon = () => <Svg size={14}><path d="M3 13V8M8 13V3M13 13V6" /></Svg>;
export const QuizKindIcon = () => <Svg size={14}><path d="m3 8.5 3 3 7-7" /></Svg>;
export const DrawingKindIcon = () => <Svg size={14}><path d="M3 13 13 13 3 4Z" /></Svg>;
export const PageKindIcon = () => <Svg size={14}><rect x="2" y="2.5" width="12" height="11" rx="2" /><path d="M2 6h12" /></Svg>;
export const PhoneKindIcon = () => <Svg size={14}><rect x="4.5" y="1.5" width="7" height="13" rx="1.8" /><path d="M7 12.5h2" /></Svg>;
export const BotKindIcon = () => <Svg size={14}><path d="M3 12.5V4a1.5 1.5 0 0 1 1.5-1.5h7A1.5 1.5 0 0 1 13 4v5a1.5 1.5 0 0 1-1.5 1.5H5.5z" /></Svg>;
export const RestartIcon = () => <Svg size={15}><path d="M13 8a5 5 0 1 1-1.5-3.5M13 2v3h-3" /></Svg>;
export const DownloadIcon = () => <Svg size={15}><path d="M8 2.5V10M5 7l3 3 3-3M3 13h10" /></Svg>;
export const TableIcon = () => <Svg size={15}><rect x="2" y="3" width="12" height="10" rx="1.5" /><path d="M2 6.5h12M2 10h12M6.5 6.5V13" /></Svg>;
export const CodeIcon = () => <Svg size={15}><path d="m5.5 4.5-3.5 3.5 3.5 3.5M10.5 4.5 14 8l-3.5 3.5" /></Svg>;
export const PaperclipIcon = () => <Svg><path d="m13 7.5-5 5a3 3 0 0 1-4.3-4.2l5.4-5.4a2 2 0 0 1 2.8 2.8l-5.3 5.3a1 1 0 0 1-1.4-1.4l4.8-4.8" /></Svg>;
export const MenuLinesIcon = () => <Svg><path d="M3 4.5h10M3 8h10M3 11.5h10" /></Svg>;
export const KeyboardIcon = () => <Svg><rect x="1.5" y="4" width="13" height="8.5" rx="1.5" /><path d="M4 7h.01M6.5 7h.01M9 7h.01M11.5 7h.01M5 10h6" /></Svg>;
