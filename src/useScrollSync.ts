import { useEffect, useRef, useCallback } from "react";

interface Anchor {
  editorY: number;
  previewY: number;
}

/**
 * Synchronizes scroll positions between a textarea editor and an HTML preview pane.
 * Uses data-source-line attributes injected by the Rust markdown renderer to build
 * an accurate mapping between source lines and rendered element positions.
 */
export function useScrollSync(
  editorRef: React.RefObject<HTMLTextAreaElement | null>,
  previewRef: React.RefObject<HTMLDivElement | null>,
  html: string,
) {
  const anchorsRef = useRef<Anchor[]>([]);
  const scrollSourceRef = useRef<"editor" | "preview" | null>(null);
  const rafRef = useRef<number>(0);

  // Rebuild the scroll anchor map whenever the rendered HTML changes.
  useEffect(() => {
    const editor = editorRef.current;
    const preview = previewRef.current;
    if (!editor || !preview) return;

    // Wait one frame for the DOM to update with new HTML content.
    const raf = requestAnimationFrame(() => {
      const elements = preview.querySelectorAll<HTMLElement>("[data-source-line]");
      const newAnchors: Anchor[] = [];

      // Compute the editor's line height from CSS.
      const editorStyle = window.getComputedStyle(editor);
      const lineHeight = parseFloat(editorStyle.lineHeight) || 23.8; // 14px * 1.7
      const editorPaddingTop = parseFloat(editorStyle.paddingTop) || 0;

      for (const el of elements) {
        const sourceLine = parseInt(el.getAttribute("data-source-line")!, 10);
        if (isNaN(sourceLine)) continue;

        // Editor Y: line number * line height + padding
        const editorY = sourceLine * lineHeight + editorPaddingTop;

        // Preview Y: element's offset relative to the preview scroll container
        const previewY = el.offsetTop - preview.offsetTop;

        newAnchors.push({ editorY, previewY });
      }

      // Sort by editor position (should already be in order, but ensure it).
      newAnchors.sort((a, b) => a.editorY - b.editorY);

      // Add boundary anchors for start and end.
      if (newAnchors.length === 0 || newAnchors[0].editorY > 0) {
        newAnchors.unshift({ editorY: 0, previewY: 0 });
      }

      const editorMaxScroll = editor.scrollHeight - editor.clientHeight;
      const previewMaxScroll = preview.scrollHeight - preview.clientHeight;
      if (editorMaxScroll > 0 && previewMaxScroll > 0) {
        const lastAnchor = newAnchors[newAnchors.length - 1];
        if (lastAnchor.editorY < editorMaxScroll || lastAnchor.previewY < previewMaxScroll) {
          newAnchors.push({ editorY: editorMaxScroll, previewY: previewMaxScroll });
        }
      }

      anchorsRef.current = newAnchors;
    });

    return () => cancelAnimationFrame(raf);
  }, [html, editorRef, previewRef]);

  // Linearly interpolate a scroll position from one pane to the other.
  const translate = useCallback(
    (fromY: number, fromEditor: boolean): number => {
      const anchors = anchorsRef.current;
      if (anchors.length < 2) {
        // Fallback to percentage-based sync
        const editor = editorRef.current;
        const preview = previewRef.current;
        if (!editor || !preview) return 0;
        const editorMax = editor.scrollHeight - editor.clientHeight;
        const previewMax = preview.scrollHeight - preview.clientHeight;
        if (fromEditor) {
          return editorMax > 0 ? (fromY / editorMax) * previewMax : 0;
        } else {
          return previewMax > 0 ? (fromY / previewMax) * editorMax : 0;
        }
      }

      // Binary search for the surrounding anchor pair.
      const getKey = (a: Anchor) => (fromEditor ? a.editorY : a.previewY);
      let lo = 0;
      let hi = anchors.length - 1;

      if (fromY <= getKey(anchors[lo])) {
        const ratio = getKey(anchors[lo]) > 0 ? fromY / getKey(anchors[lo]) : 0;
        return ratio * (fromEditor ? anchors[lo].previewY : anchors[lo].editorY);
      }
      if (fromY >= getKey(anchors[hi])) {
        return fromEditor ? anchors[hi].previewY : anchors[hi].editorY;
      }

      while (hi - lo > 1) {
        const mid = (lo + hi) >> 1;
        if (getKey(anchors[mid]) <= fromY) {
          lo = mid;
        } else {
          hi = mid;
        }
      }

      // Linear interpolation between anchors[lo] and anchors[hi].
      const fromLo = getKey(anchors[lo]);
      const fromHi = getKey(anchors[hi]);
      const t = fromHi > fromLo ? (fromY - fromLo) / (fromHi - fromLo) : 0;

      const toLo = fromEditor ? anchors[lo].previewY : anchors[lo].editorY;
      const toHi = fromEditor ? anchors[hi].previewY : anchors[hi].editorY;

      return toLo + t * (toHi - toLo);
    },
    [editorRef, previewRef],
  );

  // Attach scroll event listeners.
  useEffect(() => {
    const editor = editorRef.current;
    const preview = previewRef.current;
    if (!editor || !preview) return;

    const onEditorScroll = () => {
      if (scrollSourceRef.current === "preview") return;
      scrollSourceRef.current = "editor";

      cancelAnimationFrame(rafRef.current);
      rafRef.current = requestAnimationFrame(() => {
        const targetY = translate(editor.scrollTop, true);
        preview.scrollTop = targetY;
        // Clear the lock after the programmatic scroll settles.
        requestAnimationFrame(() => {
          scrollSourceRef.current = null;
        });
      });
    };

    const onPreviewScroll = () => {
      if (scrollSourceRef.current === "editor") return;
      scrollSourceRef.current = "preview";

      cancelAnimationFrame(rafRef.current);
      rafRef.current = requestAnimationFrame(() => {
        const targetY = translate(preview.scrollTop, false);
        editor.scrollTop = targetY;
        requestAnimationFrame(() => {
          scrollSourceRef.current = null;
        });
      });
    };

    editor.addEventListener("scroll", onEditorScroll, { passive: true });
    preview.addEventListener("scroll", onPreviewScroll, { passive: true });

    return () => {
      editor.removeEventListener("scroll", onEditorScroll);
      preview.removeEventListener("scroll", onPreviewScroll);
      cancelAnimationFrame(rafRef.current);
    };
  }, [editorRef, previewRef, translate]);
}
