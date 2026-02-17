use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;
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

/// Convert a byte offset in the source markdown to a 0-based line number.
fn byte_offset_to_line(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
}

/// Percent-encode a filesystem path for use in a URL.
fn percent_encode_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for &byte in path.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'/' | b'~' => encoded.push(byte as char),
            _ => write!(encoded, "%{:02X}", byte).unwrap(),
        }
    }
    encoded
}

/// Percent-decode a URL path back to a filesystem path.
fn percent_decode_path(input: &str) -> String {
    let mut output = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&input[i + 1..i + 3], 16) {
                output.push(byte);
                i += 3;
                continue;
            }
        }
        output.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

/// Resolve a potentially relative image URL to an absolute localimage:// URL.
fn resolve_image_url(dest_url: &str, base_dir: Option<&str>) -> String {
    // Already an absolute URL — return as-is
    if dest_url.starts_with("http://")
        || dest_url.starts_with("https://")
        || dest_url.starts_with("data:")
        || dest_url.starts_with("file://")
        || dest_url.starts_with("localimage://")
    {
        return dest_url.to_string();
    }

    let base = match base_dir {
        Some(dir) => dir,
        None => return dest_url.to_string(),
    };

    let abs_path = if dest_url.starts_with('/') {
        dest_url.to_string()
    } else {
        let path = Path::new(base).join(dest_url);
        path.to_string_lossy().into_owned()
    };

    format!("localimage://localhost{}", percent_encode_path(&abs_path))
}

/// Guess MIME type from a file extension.
fn guess_mime_type(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

/// Parse Markdown to HTML using pulldown-cmark with all extensions enabled.
/// Injects `data-source-line="N"` attributes on block-level elements for scroll sync.
/// When `base_dir` is provided, relative image paths are resolved to localimage:// URLs.
fn markdown_to_html(markdown: &str, base_dir: Option<&str>) -> String {
    let options = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_TASKLISTS;

    // First pass: collect byte-offset → line-number for block-level start events.
    // We record which byte offsets correspond to block openers so we can inject
    // data-source-line attributes after the standard HTML renderer runs.
    let parser = Parser::new_ext(markdown, options);
    let mut block_lines: Vec<usize> = Vec::new(); // line numbers in order of appearance
    // We'll collect events and their ranges, then feed plain events to push_html
    let events_with_ranges: Vec<(Event, std::ops::Range<usize>)> =
        parser.into_offset_iter().collect();

    for (event, range) in &events_with_ranges {
        match event {
            Event::Start(
                Tag::Paragraph
                | Tag::Heading { .. }
                | Tag::BlockQuote(_)
                | Tag::CodeBlock(_)
                | Tag::List(_)
                | Tag::Table(_)
                | Tag::HtmlBlock,
            )
            | Event::Rule => {
                block_lines.push(byte_offset_to_line(markdown, range.start));
            }
            _ => {}
        }
    }

    // Second pass: render HTML but wrap block-level opening tags with line annotations.
    // Strategy: we intercept Start events for block-level tags and emit a wrapper
    // <div data-source-line="N"> before the standard tag, and close it after End.
    // However, this would break semantics. Instead, we do a simpler approach:
    // render standard HTML, then post-process to inject attributes.

    // Actually, the cleanest approach: render events one-by-one, injecting attributes
    // into the opening tags of block-level elements.
    let mut html_output = String::new();
    let mut in_table_header = false;
    let mut table_alignments: Vec<pulldown_cmark::Alignment> = Vec::new();
    let mut table_cell_index: usize = 0;
    let mut footnote_numbers: HashMap<String, usize> = HashMap::new();
    let mut block_idx: usize = 0;

    for (event, _range) in &events_with_ranges {
        match event {
            Event::Start(tag) => {
                let line_attr = match tag {
                    Tag::Paragraph
                    | Tag::Heading { .. }
                    | Tag::BlockQuote(_)
                    | Tag::CodeBlock(_)
                    | Tag::List(_)
                    | Tag::Table(_)
                    | Tag::HtmlBlock => {
                        let line = block_lines.get(block_idx).copied().unwrap_or(0);
                        block_idx += 1;
                        Some(line)
                    }
                    _ => None,
                };

                match tag {
                    Tag::Paragraph => {
                        write_open_tag(&mut html_output, "p", line_attr);
                    }
                    Tag::Heading { level, id, classes, attrs } => {
                        let tag_name = match level {
                            pulldown_cmark::HeadingLevel::H1 => "h1",
                            pulldown_cmark::HeadingLevel::H2 => "h2",
                            pulldown_cmark::HeadingLevel::H3 => "h3",
                            pulldown_cmark::HeadingLevel::H4 => "h4",
                            pulldown_cmark::HeadingLevel::H5 => "h5",
                            pulldown_cmark::HeadingLevel::H6 => "h6",
                        };
                        let mut extra_attrs = String::new();
                        if let Some(id) = id {
                            write!(extra_attrs, " id=\"{}\"", id).unwrap();
                        }
                        if !classes.is_empty() {
                            write!(extra_attrs, " class=\"{}\"", classes.join(" ")).unwrap();
                        }
                        for (key, val) in attrs {
                            if let Some(v) = val {
                                write!(extra_attrs, " {}=\"{}\"", key, v).unwrap();
                            } else {
                                write!(extra_attrs, " {}", key).unwrap();
                            }
                        }
                        if let Some(line) = line_attr {
                            write!(html_output, "<{} data-source-line=\"{}\"{}>", tag_name, line, extra_attrs).unwrap();
                        } else {
                            write!(html_output, "<{}{}>", tag_name, extra_attrs).unwrap();
                        }
                    }
                    Tag::BlockQuote(_) => {
                        write_open_tag(&mut html_output, "blockquote", line_attr);
                        html_output.push('\n');
                    }
                    Tag::CodeBlock(kind) => {
                        match kind {
                            pulldown_cmark::CodeBlockKind::Fenced(info) => {
                                let lang = info.split_whitespace().next().unwrap_or("");
                                if lang.is_empty() {
                                    if let Some(line) = line_attr {
                                        write!(html_output, "<pre data-source-line=\"{}\"><code>", line).unwrap();
                                    } else {
                                        html_output.push_str("<pre><code>");
                                    }
                                } else {
                                    if let Some(line) = line_attr {
                                        write!(html_output, "<pre data-source-line=\"{}\"><code class=\"language-{}\">", line, lang).unwrap();
                                    } else {
                                        write!(html_output, "<pre><code class=\"language-{}\">", lang).unwrap();
                                    }
                                }
                            }
                            pulldown_cmark::CodeBlockKind::Indented => {
                                if let Some(line) = line_attr {
                                    write!(html_output, "<pre data-source-line=\"{}\"><code>", line).unwrap();
                                } else {
                                    html_output.push_str("<pre><code>");
                                }
                            }
                        }
                    }
                    Tag::List(Some(start)) => {
                        if let Some(line) = line_attr {
                            if *start != 1 {
                                write!(html_output, "<ol start=\"{}\" data-source-line=\"{}\">", start, line).unwrap();
                            } else {
                                write!(html_output, "<ol data-source-line=\"{}\">", line).unwrap();
                            }
                        } else {
                            if *start != 1 {
                                write!(html_output, "<ol start=\"{}\">", start).unwrap();
                            } else {
                                html_output.push_str("<ol>");
                            }
                        }
                        html_output.push('\n');
                    }
                    Tag::List(None) => {
                        write_open_tag(&mut html_output, "ul", line_attr);
                        html_output.push('\n');
                    }
                    Tag::Item => {
                        html_output.push_str("<li>");
                    }
                    Tag::Table(alignments) => {
                        table_alignments = alignments.clone();
                        write_open_tag(&mut html_output, "table", line_attr);
                    }
                    Tag::TableHead => {
                        html_output.push_str("<thead><tr>");
                        in_table_header = true;
                        table_cell_index = 0;
                    }
                    Tag::TableRow => {
                        html_output.push_str("<tr>");
                        table_cell_index = 0;
                    }
                    Tag::TableCell => {
                        let tag = if in_table_header { "th" } else { "td" };
                        let align = table_alignments.get(table_cell_index);
                        match align {
                            Some(pulldown_cmark::Alignment::Left) => {
                                write!(html_output, "<{} style=\"text-align: left\">", tag).unwrap();
                            }
                            Some(pulldown_cmark::Alignment::Center) => {
                                write!(html_output, "<{} style=\"text-align: center\">", tag).unwrap();
                            }
                            Some(pulldown_cmark::Alignment::Right) => {
                                write!(html_output, "<{} style=\"text-align: right\">", tag).unwrap();
                            }
                            _ => {
                                write!(html_output, "<{}>", tag).unwrap();
                            }
                        }
                    }
                    Tag::Emphasis => html_output.push_str("<em>"),
                    Tag::Strong => html_output.push_str("<strong>"),
                    Tag::Strikethrough => html_output.push_str("<del>"),
                    Tag::Link { link_type: _, dest_url, title, id: _ } => {
                        write!(html_output, "<a href=\"{}\"", dest_url).unwrap();
                        if !title.is_empty() {
                            write!(html_output, " title=\"{}\"", title).unwrap();
                        }
                        html_output.push('>');
                    }
                    Tag::Image { link_type: _, dest_url, title: _, id: _ } => {
                        let resolved_src = resolve_image_url(dest_url, base_dir);
                        write!(html_output, "<img src=\"{}\" alt=\"", resolved_src).unwrap();
                        // alt text will be filled by Text events; we handle it below
                        // Actually, pulldown-cmark puts alt text as child events
                        // For simplicity, we just open the tag and collect text
                        // The end tag handler will close it
                    }
                    Tag::FootnoteDefinition(label) => {
                        let n = footnote_numbers.len() + 1;
                        footnote_numbers.entry(label.to_string()).or_insert(n);
                        write!(html_output, "<div class=\"footnote-definition\" id=\"{}\"><sup class=\"footnote-definition-label\">{}</sup>", label, footnote_numbers[label.as_ref()]).unwrap();
                    }
                    Tag::HtmlBlock => {
                        // HTML blocks are rendered as-is by Html events
                    }
                    Tag::MetadataBlock(_) => {}
                    Tag::DefinitionList => html_output.push_str("<dl>"),
                    Tag::DefinitionListTitle => html_output.push_str("<dt>"),
                    Tag::DefinitionListDefinition => html_output.push_str("<dd>"),
                }
            }
            Event::End(tag_end) => {
                match tag_end {
                    TagEnd::Paragraph => html_output.push_str("</p>\n"),
                    TagEnd::Heading(level) => {
                        let tag_name = match level {
                            pulldown_cmark::HeadingLevel::H1 => "h1",
                            pulldown_cmark::HeadingLevel::H2 => "h2",
                            pulldown_cmark::HeadingLevel::H3 => "h3",
                            pulldown_cmark::HeadingLevel::H4 => "h4",
                            pulldown_cmark::HeadingLevel::H5 => "h5",
                            pulldown_cmark::HeadingLevel::H6 => "h6",
                        };
                        write!(html_output, "</{}>\n", tag_name).unwrap();
                    }
                    TagEnd::BlockQuote(_) => html_output.push_str("</blockquote>\n"),
                    TagEnd::CodeBlock => html_output.push_str("</code></pre>\n"),
                    TagEnd::List(ordered) => {
                        if *ordered {
                            html_output.push_str("</ol>\n");
                        } else {
                            html_output.push_str("</ul>\n");
                        }
                    }
                    TagEnd::Item => html_output.push_str("</li>\n"),
                    TagEnd::Table => html_output.push_str("</tbody></table>\n"),
                    TagEnd::TableHead => {
                        html_output.push_str("</tr></thead><tbody>\n");
                        in_table_header = false;
                    }
                    TagEnd::TableRow => html_output.push_str("</tr>\n"),
                    TagEnd::TableCell => {
                        let tag = if in_table_header { "th" } else { "td" };
                        write!(html_output, "</{}>", tag).unwrap();
                        table_cell_index += 1;
                    }
                    TagEnd::Emphasis => html_output.push_str("</em>"),
                    TagEnd::Strong => html_output.push_str("</strong>"),
                    TagEnd::Strikethrough => html_output.push_str("</del>"),
                    TagEnd::Link => html_output.push_str("</a>"),
                    TagEnd::Image => {
                        html_output.push_str("\" />");
                    }
                    TagEnd::FootnoteDefinition => html_output.push_str("</div>\n"),
                    TagEnd::HtmlBlock => {}
                    TagEnd::MetadataBlock(_) => {}
                    TagEnd::DefinitionList => html_output.push_str("</dl>"),
                    TagEnd::DefinitionListTitle => html_output.push_str("</dt>"),
                    TagEnd::DefinitionListDefinition => html_output.push_str("</dd>"),
                }
            }
            Event::Text(text) => {
                pulldown_cmark_escape::escape_html_body_text(
                    &mut pulldown_cmark_escape::FmtWriter(&mut html_output),
                    text,
                )
                .unwrap();
            }
            Event::Code(text) => {
                html_output.push_str("<code>");
                pulldown_cmark_escape::escape_html_body_text(
                    &mut pulldown_cmark_escape::FmtWriter(&mut html_output),
                    text,
                )
                .unwrap();
                html_output.push_str("</code>");
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                html_output.push_str(html);
            }
            Event::SoftBreak => html_output.push('\n'),
            Event::HardBreak => html_output.push_str("<br />\n"),
            Event::Rule => {
                let line = block_lines.get(block_idx).copied().unwrap_or(0);
                block_idx += 1;
                write!(html_output, "<hr data-source-line=\"{}\" />\n", line).unwrap();
            }
            Event::FootnoteReference(label) => {
                let n = {
                    let next = footnote_numbers.len() + 1;
                    *footnote_numbers.entry(label.to_string()).or_insert(next)
                };
                write!(html_output, "<sup class=\"footnote-reference\"><a href=\"#{}\">{}</a></sup>", label, n).unwrap();
            }
            Event::TaskListMarker(checked) => {
                if *checked {
                    html_output.push_str("<input disabled=\"\" type=\"checkbox\" checked=\"\"/>\n");
                } else {
                    html_output.push_str("<input disabled=\"\" type=\"checkbox\"/>\n");
                }
            }
            Event::InlineMath(text) => {
                html_output.push_str("<code class=\"math math-inline\">");
                pulldown_cmark_escape::escape_html_body_text(
                    &mut pulldown_cmark_escape::FmtWriter(&mut html_output),
                    text,
                )
                .unwrap();
                html_output.push_str("</code>");
            }
            Event::DisplayMath(text) => {
                html_output.push_str("<pre class=\"math math-display\"><code>");
                pulldown_cmark_escape::escape_html_body_text(
                    &mut pulldown_cmark_escape::FmtWriter(&mut html_output),
                    text,
                )
                .unwrap();
                html_output.push_str("</code></pre>\n");
            }
        }
    }

    html_output
}

/// Helper to write an opening HTML tag with an optional data-source-line attribute.
fn write_open_tag(out: &mut String, tag: &str, line: Option<usize>) {
    if let Some(line) = line {
        write!(out, "<{} data-source-line=\"{}\">", tag, line).unwrap();
    } else {
        write!(out, "<{}>", tag).unwrap();
    }
}

/// Called by the frontend when the user types. Updates central state and broadcasts to all windows.
#[tauri::command]
fn update_content(
    app: tauri::AppHandle,
    state: State<'_, DocumentState>,
    content: String,
    source_window: String,
) -> Result<String, String> {
    let base_dir = {
        let fp = state.file_path.lock().map_err(|e| e.to_string())?;
        fp.as_ref()
            .and_then(|p| Path::new(p).parent().map(|d| d.to_string_lossy().into_owned()))
    };
    let html = markdown_to_html(&content, base_dir.as_deref());

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
    let base_dir = {
        let fp = state.file_path.lock().map_err(|e| e.to_string())?;
        fp.as_ref()
            .and_then(|p| Path::new(p).parent().map(|d| d.to_string_lossy().into_owned()))
    };
    let html = markdown_to_html(&content, base_dir.as_deref());
    Ok(SyncPayload {
        content,
        html,
        source_window: String::new(),
    })
}

/// Parse markdown to HTML (stateless utility).
#[tauri::command]
fn parse_markdown(content: String) -> String {
    markdown_to_html(&content, None)
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
    let base_dir = Path::new(&path)
        .parent()
        .map(|d| d.to_string_lossy().into_owned());
    let html = markdown_to_html(&content, base_dir.as_deref());

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
        .register_uri_scheme_protocol("localimage", |_ctx, request| {
            let raw_path = request.uri().path();
            let path = percent_decode_path(raw_path);
            match std::fs::read(&path) {
                Ok(data) => {
                    let mime = guess_mime_type(&path);
                    http::Response::builder()
                        .header("Content-Type", mime)
                        .header("Access-Control-Allow-Origin", "*")
                        .body(data)
                        .unwrap()
                }
                Err(_) => http::Response::builder()
                    .status(http::StatusCode::NOT_FOUND)
                    .body(Vec::new())
                    .unwrap(),
            }
        })
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
