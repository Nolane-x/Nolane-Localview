const iconProps = {
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.7,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

export function InspectIcon() { return <svg {...iconProps}><circle cx="12" cy="12" r="3"/><path d="M3 8V4h4M17 4h4v4M21 16v4h-4M7 20H3v-4"/></svg>; }
export function ResponsiveIcon() { return <svg {...iconProps}><rect x="3" y="5" width="13" height="14" rx="2"/><rect x="18" y="8" width="3" height="9" rx="1"/><path d="M8 16h3"/></svg>; }
export function ConsoleIcon() { return <svg {...iconProps}><rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/></svg>; }
export function NetworkIcon() { return <svg {...iconProps}><circle cx="5" cy="12" r="2"/><circle cx="19" cy="6" r="2"/><circle cx="19" cy="18" r="2"/><path d="m7 11 10-4M7 13l10 4"/></svg>; }
export function SparkIcon() { return <svg {...iconProps}><path d="m12 3 1.2 4.3L17 9l-3.8 1.7L12 15l-1.2-4.3L7 9l3.8-1.7L12 3ZM18.5 14l.7 2.3 2.3.7-2.3.7-.7 2.3-.7-2.3-2.3-.7 2.3-.7.7-2.3ZM5 14l.6 1.9 1.9.6-1.9.6L5 19l-.6-1.9-1.9-.6 1.9-.6L5 14Z"/></svg>; }
export function CommandIcon() { return <svg {...iconProps}><path d="M9 6h6M9 12h6M9 18h6M5 6h.01M5 12h.01M5 18h.01"/></svg>; }
export function SearchIcon() { return <svg {...iconProps}><circle cx="11" cy="11" r="6"/><path d="m16 16 4 4"/></svg>; }
export function CloseIcon() { return <svg {...iconProps}><path d="m7 7 10 10M17 7 7 17"/></svg>; }
export function PauseIcon() { return <svg {...iconProps}><path d="M9 6v12M15 6v12"/></svg>; }
export function PlayIcon() { return <svg {...iconProps}><path d="m9 6 9 6-9 6V6Z"/></svg>; }
export function ExternalIcon() { return <svg {...iconProps}><path d="M14 4h6v6M20 4l-9 9"/><path d="M18 13v5a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h5"/></svg>; }
export function ExpandIcon() { return <svg {...iconProps}><path d="M8 3H3v5M16 3h5v5M21 16v5h-5M3 16v5h5"/></svg>; }
export function ActivityIcon() { return <svg {...iconProps}><path d="M3 12h4l2-6 4 12 2-6h6"/></svg>; }
export function CheckIcon() { return <svg {...iconProps}><path d="m5 12 4 4L19 6"/></svg>; }
export function WarningIcon() { return <svg {...iconProps}><path d="M12 4 3 20h18L12 4Z"/><path d="M12 9v4M12 17h.01"/></svg>; }

export function HideIcon() { return <svg {...iconProps}><path d="M4 4l16 16"/><path d="M10.6 10.7a2 2 0 0 0 2.7 2.7"/><path d="M9.9 4.3A10.7 10.7 0 0 1 12 4c5.2 0 8.5 4.4 9.5 6-.5.8-1.6 2.3-3.2 3.6M6.6 6.7C4.6 8 3.2 10 2.5 11c1 1.6 4.3 6 9.5 6 1.3 0 2.5-.3 3.5-.7"/></svg>; }
export function SettingsIcon() { return <svg {...iconProps}><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6V21h-4v-.1a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H3v-4h.1a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1a1.7 1.7 0 0 0 1.9.3 1.7 1.7 0 0 0 1-1.6V3h4v.1a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.1v4H21a1.7 1.7 0 0 0-1.6 1Z"/></svg>; }
export function MoreIcon() { return <svg {...iconProps}><circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/></svg>; }
export function SourceIcon() { return <svg {...iconProps}><path d="m8 9-3 3 3 3M16 9l3 3-3 3M14 5l-4 14"/></svg>; }
export function RulerIcon() { return <svg {...iconProps}><path d="m4 16 12-12 4 4L8 20 4 16Z"/><path d="m13 7 4 4M10 10l2 2M7 13l2 2"/></svg>; }
export function CaptureIcon() { return <svg {...iconProps}><path d="M8 5h8l1 2h2a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2h2l1-2Z"/><circle cx="12" cy="13" r="3"/></svg>; }
