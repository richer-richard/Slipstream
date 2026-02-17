interface ToolbarProps {
  onOpen: () => void;
  onSave: () => void;
  onNewWindow: () => void;
}

function ToolbarButton({
  onClick,
  title,
  children,
}: {
  onClick: () => void;
  title: string;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      className="flex items-center justify-center w-8 h-8 rounded-md transition-colors duration-150"
      style={{
        color: "var(--text-secondary)",
      }}
      onMouseEnter={(e) => {
        e.currentTarget.style.background = "var(--bg-secondary)";
        e.currentTarget.style.color = "var(--text-primary)";
      }}
      onMouseLeave={(e) => {
        e.currentTarget.style.background = "transparent";
        e.currentTarget.style.color = "var(--text-secondary)";
      }}
    >
      {children}
    </button>
  );
}

function Toolbar({ onOpen, onSave, onNewWindow }: ToolbarProps) {
  return (
    <div className="flex items-center gap-1">
      {/* Open */}
      <ToolbarButton onClick={onOpen} title="Open File (Cmd+O)">
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M2 13V5a1 1 0 011-1h3l2 2h5a1 1 0 011 1v6a1 1 0 01-1 1H3a1 1 0 01-1-1z" />
        </svg>
      </ToolbarButton>

      {/* Save */}
      <ToolbarButton onClick={onSave} title="Save File (Cmd+S)">
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <path d="M12.7 3.3l-2-2A1 1 0 0010 1H3a1 1 0 00-1 1v12a1 1 0 001 1h10a1 1 0 001-1V4a1 1 0 00-.3-.7z" />
          <path d="M11 1v4H5V1" />
          <path d="M5 10h6" />
          <path d="M5 12.5h3" />
        </svg>
      </ToolbarButton>

      {/* New Window */}
      <ToolbarButton onClick={onNewWindow} title="New Window (Cmd+Shift+N)">
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
          <rect x="1" y="2" width="11" height="9" rx="1" />
          <path d="M4 14h10a1 1 0 001-1V5" />
          <path d="M5 6.5h5" />
          <path d="M7.5 4v5" />
        </svg>
      </ToolbarButton>
    </div>
  );
}

export default Toolbar;
