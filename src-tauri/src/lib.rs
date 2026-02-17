use pulldown_cmark::{html, Options, Parser};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Emitter, State, WebviewUrl, WebviewWindowBuilder};

/// Centralized document state — the single source of truth.
pub struct DocumentState {
    pub content: Mutex<String>,
    pub file_path: Mutex<Option<String>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SyncPayload {
    pub content: String,
    pub html: String,
    pub source_window: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub content: String,
}

/// Parse Markdown to HTML using pulldown-cmark with all extensions enabled.
fn markdown_to_html(markdown: &str) -> String {
    let options = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(markdown, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

/// Called by the frontend when the user types. Updates central state and broadcasts to all windows.
#[tauri::command]
fn update_content(
    app: tauri::AppHandle,
    state: State<'_, DocumentState>,
    content: String,
    source_window: String,
) -> Result<String, String> {
    let html = markdown_to_html(&content);

    // Update centralized state
    {
        let mut doc = state.content.lock().map_err(|e| e.to_string())?;
        *doc = content.clone();
    }

    // Broadcast to all windows
    let payload = SyncPayload {
        content: content.clone(),
        html: html.clone(),
        source_window,
    };
    app.emit("content-sync", &payload)
        .map_err(|e| e.to_string())?;

    Ok(html)
}

/// Get current document state (used when a new window opens).
#[tauri::command]
fn get_content(state: State<'_, DocumentState>) -> Result<SyncPayload, String> {
    let content = state.content.lock().map_err(|e| e.to_string())?.clone();
    let html = markdown_to_html(&content);
    Ok(SyncPayload {
        content,
        html,
        source_window: String::new(),
    })
}

/// Parse markdown to HTML (stateless utility).
#[tauri::command]
fn parse_markdown(content: String) -> String {
    markdown_to_html(&content)
}

/// Open a new editor window.
#[tauri::command]
fn new_window(app: tauri::AppHandle) -> Result<(), String> {
    let window_label = format!("editor-{}", uuid_simple());
    let _window = WebviewWindowBuilder::new(&app, &window_label, WebviewUrl::default())
        .title("Slipstream")
        .inner_size(1200.0, 800.0)
        .min_inner_size(600.0, 400.0)
        .transparent(true)
        .decorations(true)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Save content to the currently tracked file path, or prompt for one.
#[tauri::command]
fn save_file(state: State<'_, DocumentState>, path: String) -> Result<(), String> {
    let content = state.content.lock().map_err(|e| e.to_string())?.clone();
    std::fs::write(&path, &content).map_err(|e| e.to_string())?;
    let mut fp = state.file_path.lock().map_err(|e| e.to_string())?;
    *fp = Some(path);
    Ok(())
}

/// Load file content into the central state and broadcast.
#[tauri::command]
fn open_file(
    app: tauri::AppHandle,
    state: State<'_, DocumentState>,
    path: String,
) -> Result<SyncPayload, String> {
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let html = markdown_to_html(&content);

    {
        let mut doc = state.content.lock().map_err(|e| e.to_string())?;
        *doc = content.clone();
    }
    {
        let mut fp = state.file_path.lock().map_err(|e| e.to_string())?;
        *fp = Some(path);
    }

    let payload = SyncPayload {
        content: content.clone(),
        html: html.clone(),
        source_window: String::from("__file_open__"),
    };
    app.emit("content-sync", &payload)
        .map_err(|e| e.to_string())?;

    Ok(payload)
}

/// Get the current file path.
#[tauri::command]
fn get_file_path(state: State<'_, DocumentState>) -> Result<Option<String>, String> {
    let fp = state.file_path.lock().map_err(|e| e.to_string())?;
    Ok(fp.clone())
}

/// Simple UUID-like unique ID generator (no external dep needed).
fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}{}", dur.as_millis(), dur.subsec_nanos())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(DocumentState {
            content: Mutex::new(String::from(
                "# Welcome to Slipstream\n\nStart typing your Markdown here...\n\n## Features\n\n- **Real-time preview** with live rendering\n- **Multi-window sync** — open a new window and type in either\n- **Native file dialogs** for Open and Save\n- **GitHub Flavored Markdown** support\n\n---\n\n> Slipstream: A local-first, high-performance Markdown editor.\n",
            )),
            file_path: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            update_content,
            get_content,
            parse_markdown,
            new_window,
            save_file,
            open_file,
            get_file_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Slipstream");
}
