import { useEffect, useRef, useCallback } from "react";

interface Anchor {
  editorY: number;
  previewY: number;
}

/**
 * Synchronizes scroll positions between a textarea editor and an HTML preview pane.
 *
 * Uses a mirror-div technique to accurately measure editor line positions
 * (accounting for word wrap), combined with data-source-line attributes
 * from the Rust renderer for preview positions.
 *
 * Active-pane tracking via pointer/wheel events prevents feedback loops.
 */
export function useScrollSync(
  editorRef: React.RefObject<HTMLTextAreaElement | null>,
  previewRef: React.RefObject<HTMLDivElement | null>,
  html: string,
  content: string,
) {
  const anchorsRef = useRef<Anchor[]>([]);
  const activePaneRef = useRef<"editor" | "preview" | null>(null);
  const rafRef = useRef<number>(0);
  const contentRef = useRef(content);
  contentRef.current = content;

  // Build the anchor map: maps editor scroll positions ↔ preview scroll positions.
  // Uses a hidden mirror div to get pixel-accurate editor positions that
  // account for word wrap, instead of the naive lineNumber × lineHeight.
  const buildAnchors = useCallback(() => {
    const editor = editorRef.current;
    const preview = previewRef.current;
    if (!editor || !preview) return;

    // 1. Collect preview-side positions from data-source-line elements.
    const elements = preview.querySelectorAll<HTMLElement>("[data-source-line]");
    if (elements.length === 0) {
      anchorsRef.current = [];
      return;
    }

    const previewRect = preview.getBoundingClientRect();
    const targetLines = new Map<number, number>();
    for (const el of elements) {
      const line = parseInt(el.getAttribute("data-source-line")!, 10);
      if (isNaN(line)) continue;
      const elRect = el.getBoundingClientRect();
      targetLines.set(line, elRect.top - previewRect.top + preview.scrollTop);
    }

    // 2. Build a hidden mirror div matching the textarea's layout to measure
    //    accurate Y positions that account for word wrap.
    const currentContent = contentRef.current;
    const mirror = document.createElement("div");
    const cs = window.getComputedStyle(editor);
    mirror.style.position = "absolute";
    mirror.style.left = "-9999px";
    mirror.style.top = "0";
    mirror.style.visibility = "hidden";
    mirror.style.whiteSpace = "pre-wrap";
    mirror.style.wordWrap = "break-word";
    mirror.style.overflowWrap = "break-word";
    mirror.style.width = `${editor.clientWidth}px`;
    mirror.style.font = cs.font;
    mirror.style.lineHeight = cs.lineHeight;
    mirror.style.tabSize = cs.tabSize;
    mirror.style.letterSpacing = cs.letterSpacing;
    mirror.style.padding = cs.padding;
    mirror.style.boxSizing = "border-box";
    mirror.style.border = "none";

    const lines = currentContent.split("\n");
    const markers = new Map<number, HTMLSpanElement>();

    for (let i = 0; i < lines.length; i++) {
      if (targetLines.has(i)) {
        const marker = document.createElement("span");
        marker.textContent = "\u200B"; // Zero-width space for layout
        markers.set(i, marker);
        mirror.appendChild(marker);
      }
      mirror.appendChild(document.createTextNode(lines[i]));
      if (i < lines.length - 1) {
        mirror.appendChild(document.createTextNode("\n"));
      }
    }

    document.body.appendChild(mirror);

    // 3. Measure marker positions relative to mirror top.
    const mirrorRect = mirror.getBoundingClientRect();
    const newAnchors: Anchor[] = [];
    for (const [lineNum, marker] of markers) {
      const previewY = targetLines.get(lineNum);
      if (previewY === undefined) continue;
      const markerRect = marker.getBoundingClientRect();
      newAnchors.push({
        editorY: markerRect.top - mirrorRect.top,
        previewY,
      });
    }

    document.body.removeChild(mirror);

    // 4. Sort and add boundary anchors.
    newAnchors.sort((a, b) => a.editorY - b.editorY);

    if (newAnchors.length === 0 || newAnchors[0].editorY > 0) {
      newAnchors.unshift({ editorY: 0, previewY: 0 });
    }

    const editorMaxScroll = editor.scrollHeight - editor.clientHeight;
    const previewMaxScroll = preview.scrollHeight - preview.clientHeight;
    if (editorMaxScroll > 0 && previewMaxScroll > 0) {
      newAnchors.push({ editorY: editorMaxScroll, previewY: previewMaxScroll });
    }

    anchorsRef.current = newAnchors;
  }, [editorRef, previewRef]);

  // Rebuild anchors when rendered HTML changes.
  useEffect(() => {
    const raf = requestAnimationFrame(buildAnchors);
    return () => cancelAnimationFrame(raf);
  }, [html, buildAnchors]);

  // Rebuild anchors when the editor resizes (word wrap changes).
  useEffect(() => {
    const editor = editorRef.current;
    if (!editor) return;

    let timer: number;
    const observer = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = window.setTimeout(buildAnchors, 150);
    });
    observer.observe(editor);

    return () => {
      observer.disconnect();
      clearTimeout(timer);
    };
  }, [editorRef, buildAnchors]);

  // Translate a scroll position from one pane to the other using
  // binary search + linear interpolation on the anchor map.
  const translate = useCallback(
    (fromY: number, fromEditor: boolean): number => {
      const anchors = anchorsRef.current;
      if (anchors.length < 2) {
        // Fallback: percentage-based sync.
        const editor = editorRef.current;
        const preview = previewRef.current;
        if (!editor || !preview) return 0;
        const editorMax = editor.scrollHeight - editor.clientHeight;
        const previewMax = preview.scrollHeight - preview.clientHeight;
        if (fromEditor) return editorMax > 0 ? (fromY / editorMax) * previewMax : 0;
        return previewMax > 0 ? (fromY / previewMax) * editorMax : 0;
      }

      const getKey = (a: Anchor) => (fromEditor ? a.editorY : a.previewY);
      const getVal = (a: Anchor) => (fromEditor ? a.previewY : a.editorY);
      let lo = 0;
      let hi = anchors.length - 1;

      if (fromY <= getKey(anchors[lo])) {
        const ratio = getKey(anchors[lo]) > 0 ? fromY / getKey(anchors[lo]) : 0;
        return ratio * getVal(anchors[lo]);
      }
      if (fromY >= getKey(anchors[hi])) {
        return getVal(anchors[hi]);
      }

      while (hi - lo > 1) {
        const mid = (lo + hi) >> 1;
        if (getKey(anchors[mid]) <= fromY) lo = mid;
        else hi = mid;
      }

      const fromLo = getKey(anchors[lo]);
      const fromHi = getKey(anchors[hi]);
      const t = fromHi > fromLo ? (fromY - fromLo) / (fromHi - fromLo) : 0;
      return getVal(anchors[lo]) + t * (getVal(anchors[hi]) - getVal(anchors[lo]));
    },
    [editorRef, previewRef],
  );

  // Attach scroll + pointer/wheel listeners.
  // Only the pane the user is interacting with drives sync — this completely
  // eliminates the feedback loop that caused shivering.
  useEffect(() => {
    const editor = editorRef.current;
    const preview = previewRef.current;
    if (!editor || !preview) return;

    const setEditorActive = () => {
      activePaneRef.current = "editor";
    };
    const setPreviewActive = () => {
      activePaneRef.current = "preview";
    };

    const onEditorScroll = () => {
      if (activePaneRef.current !== "editor") return;
      cancelAnimationFrame(rafRef.current);
      rafRef.current = requestAnimationFrame(() => {
        preview.scrollTop = translate(editor.scrollTop, true);
      });
    };

    const onPreviewScroll = () => {
      if (activePaneRef.current !== "preview") return;
      cancelAnimationFrame(rafRef.current);
      rafRef.current = requestAnimationFrame(() => {
        editor.scrollTop = translate(preview.scrollTop, false);
      });
    };

    // Active-pane detection via pointer, wheel, keyboard, and focus.
    editor.addEventListener("pointerenter", setEditorActive);
    editor.addEventListener("wheel", setEditorActive, { passive: true });
    editor.addEventListener("keydown", setEditorActive);
    editor.addEventListener("focus", setEditorActive);
    preview.addEventListener("pointerenter", setPreviewActive);
    preview.addEventListener("wheel", setPreviewActive, { passive: true });

    // Scroll sync.
    editor.addEventListener("scroll", onEditorScroll, { passive: true });
    preview.addEventListener("scroll", onPreviewScroll, { passive: true });

    return () => {
      editor.removeEventListener("pointerenter", setEditorActive);
      editor.removeEventListener("wheel", setEditorActive);
      editor.removeEventListener("keydown", setEditorActive);
      editor.removeEventListener("focus", setEditorActive);
      preview.removeEventListener("pointerenter", setPreviewActive);
      preview.removeEventListener("wheel", setPreviewActive);
      editor.removeEventListener("scroll", onEditorScroll);
      preview.removeEventListener("scroll", onPreviewScroll);
      cancelAnimationFrame(rafRef.current);
    };
  }, [editorRef, previewRef, translate]);
}
