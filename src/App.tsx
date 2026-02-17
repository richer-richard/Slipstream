import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open, save } from "@tauri-apps/plugin-dialog";
import Toolbar from "./Toolbar";
import { useScrollSync } from "./useScrollSync";

interface SyncPayload {
  content: string;
  html: string;
  source_window: string;
}

function App() {
  const [content, setContent] = useState("");
  const [html, setHtml] = useState("");
  const [filePath, setFilePath] = useState<string | null>(null);
  const [windowLabel, setWindowLabel] = useState("");
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const previewRef = useRef<HTMLDivElement>(null);
  const isRemoteUpdate = useRef(false);

  // Initialize: get current state from backend
  useEffect(() => {
    const init = async () => {
      const win = getCurrentWebviewWindow();
      setWindowLabel(win.label);

      try {
        const state = await invoke<SyncPayload>("get_content");
        setContent(state.content);
        setHtml(state.html);

        const path = await invoke<string | null>("get_file_path");
        setFilePath(path);
      } catch (e) {
        console.error("Failed to get initial state:", e);
      }
    };
    init();
  }, []);

  // Listen for sync events from other windows
  useEffect(() => {
    const unlisten = listen<SyncPayload>("content-sync", (event) => {
      const payload = event.payload;
      // Only apply if the event came from a different window
      if (payload.source_window !== windowLabel) {
        isRemoteUpdate.current = true;
        setContent(payload.content);
        setHtml(payload.html);
      }
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [windowLabel]);

  // Handle user typing
  const handleChange = useCallback(
    async (e: React.ChangeEvent<HTMLTextAreaElement>) => {
      const newContent = e.target.value;
      setContent(newContent);

      try {
        const result = await invoke<string>("update_content", {
          content: newContent,
          sourceWindow: windowLabel,
        });
        setHtml(result);
      } catch (e) {
        console.error("Failed to update content:", e);
      }
    },
    [windowLabel],
  );

  // Handle Tab key for indentation
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (e.key === "Tab") {
        e.preventDefault();
        const textarea = editorRef.current;
        if (!textarea) return;
        const start = textarea.selectionStart;
        const end = textarea.selectionEnd;
        const value = textarea.value;
        const newValue = value.substring(0, start) + "  " + value.substring(end);
        setContent(newValue);
        // Restore cursor position
        requestAnimationFrame(() => {
          textarea.selectionStart = textarea.selectionEnd = start + 2;
        });
        // Sync the change
        invoke<string>("update_content", {
          content: newValue,
          sourceWindow: windowLabel,
        }).then(setHtml);
      }
    },
    [windowLabel],
  );

  // Open file
  const handleOpen = useCallback(async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          { name: "Markdown", extensions: ["md", "markdown", "txt"] },
          { name: "All Files", extensions: ["*"] },
        ],
      });
      if (selected) {
        const result = await invoke<SyncPayload>("open_file", { path: selected });
        setContent(result.content);
        setHtml(result.html);
        setFilePath(selected);
      }
    } catch (e) {
      console.error("Failed to open file:", e);
    }
  }, []);

  // Save file
  const handleSave = useCallback(async () => {
    try {
      let path = filePath;
      if (!path) {
        const selected = await save({
          filters: [
            { name: "Markdown", extensions: ["md"] },
            { name: "All Files", extensions: ["*"] },
          ],
          defaultPath: "untitled.md",
        });
        if (!selected) return;
        path = selected;
      }
      await invoke("save_file", { path });
      setFilePath(path);
    } catch (e) {
      console.error("Failed to save file:", e);
    }
  }, [filePath]);

  // New window
  const handleNewWindow = useCallback(async () => {
    try {
      await invoke("new_window");
    } catch (e) {
      console.error("Failed to open new window:", e);
    }
  }, []);

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey) {
        if (e.key === "s") {
          e.preventDefault();
          handleSave();
        } else if (e.key === "o") {
          e.preventDefault();
          handleOpen();
        } else if (e.key === "n" && e.shiftKey) {
          e.preventDefault();
          handleNewWindow();
        }
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [handleSave, handleOpen, handleNewWindow]);

  useScrollSync(editorRef, previewRef, html);

  const fileName = filePath ? filePath.split("/").pop() : "Untitled";

  return (
    <div className="flex flex-col h-screen" style={{ background: "var(--bg-primary)" }}>
      {/* Titlebar */}
      <div
        className="titlebar-drag flex items-center justify-between px-4 shrink-0"
        style={{
          height: 52,
          borderBottom: "1px solid var(--border-color)",
          paddingTop: 6,
        }}
      >
        {/* Spacer for traffic lights */}
        <div className="w-20" />

        {/* Title */}
        <div className="flex items-center gap-2 select-none">
          <span className="text-sm font-medium" style={{ color: "var(--text-primary)" }}>
            {fileName}
          </span>
          {filePath && (
            <span className="text-xs" style={{ color: "var(--text-secondary)" }}>
              — {filePath}
            </span>
          )}
        </div>

        {/* Toolbar */}
        <div className="titlebar-no-drag">
          <Toolbar
            onOpen={handleOpen}
            onSave={handleSave}
            onNewWindow={handleNewWindow}
          />
        </div>
      </div>

      {/* Main Content: Split Pane */}
      <div className="flex flex-1 overflow-hidden">
        {/* Editor Pane */}
        <div
          className="flex flex-col w-1/2 overflow-hidden"
          style={{ borderRight: "1px solid var(--border-color)" }}
        >
          <div
            className="px-4 py-2 text-xs font-medium uppercase tracking-wider select-none shrink-0"
            style={{
              color: "var(--text-secondary)",
              background: "var(--bg-secondary)",
              borderBottom: "1px solid var(--border-color)",
            }}
          >
            Editor
          </div>
          <textarea
            ref={editorRef}
            className="editor-textarea flex-1 w-full p-4 overflow-auto"
            value={content}
            onChange={handleChange}
            onKeyDown={handleKeyDown}
            placeholder="Start writing Markdown..."
            spellCheck={false}
          />
        </div>

        {/* Preview Pane */}
        <div className="flex flex-col w-1/2 overflow-hidden">
          <div
            className="px-4 py-2 text-xs font-medium uppercase tracking-wider select-none shrink-0"
            style={{
              color: "var(--text-secondary)",
              background: "var(--bg-secondary)",
              borderBottom: "1px solid var(--border-color)",
            }}
          >
            Preview
          </div>
          <div
            ref={previewRef}
            className="markdown-preview flex-1 overflow-auto p-6"
            dangerouslySetInnerHTML={{ __html: html }}
          />
        </div>
      </div>

      {/* Status Bar */}
      <div
        className="flex items-center justify-between px-4 py-1.5 text-xs select-none shrink-0"
        style={{
          color: "var(--text-secondary)",
          background: "var(--bg-secondary)",
          borderTop: "1px solid var(--border-color)",
        }}
      >
        <span>
          {content.split("\n").length} lines &middot; {content.length} chars
        </span>
        <span>Markdown</span>
        <span>Window: {windowLabel}</span>
      </div>
    </div>
  );
}

export default App;
