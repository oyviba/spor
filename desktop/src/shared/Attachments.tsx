// Images attached to a prompt: pasted, dropped or picked, kept by the
// backend in `.git/spor/attachments/`, and named in the prompt by path so
// the agent can look at them (Claude Code reads images with its Read tool).

import { useCallback, useEffect, useRef, useState } from "react";
import type { ClipboardEvent, DragEvent } from "react";
import { api } from "../api";
import { ImageIcon, X } from "./Icons";

export interface Attachment {
  /** A local preview of it. */
  url: string;
  name: string;
  /** Where the backend kept it; null while it's being saved. */
  path: string | null;
}

/** The prompt with its images named at the end, as the agent is sent them. */
export function withImages(text: string, images: Attachment[]): string {
  const paths = images.flatMap((a) => (a.path ? [a.path] : []));
  if (paths.length === 0) return text;
  const list = paths.map((p) => `- ${p}`).join("\n");
  return `${text.trim()}${text.trim() ? "\n\n" : ""}Attached images (open each with the Read tool):\n${list}`;
}

const imagesOf = (files: Iterable<File>) => [...files].filter((f) => f.type.startsWith("image/"));

/** A prompt's attachments, and the handlers that add them. */
export function useAttachments(onError: (e: unknown) => void) {
  const [images, setImages] = useState<Attachment[]>([]);
  const [dragging, setDragging] = useState(false);
  const latest = useRef(images);
  latest.current = images;
  // Previews are object URLs: let them go with the composer.
  useEffect(() => () => latest.current.forEach((a) => URL.revokeObjectURL(a.url)), []);

  const add = useCallback(
    (files: Iterable<File>) => {
      for (const file of imagesOf(files)) {
        const url = URL.createObjectURL(file);
        setImages((all) => [...all, { url, name: file.name || "Pasted image", path: null }]);
        file
          .arrayBuffer()
          .then((data) => api.attachImage(new Uint8Array(data)))
          .then((path) => setImages((all) => all.map((a) => (a.url === url ? { ...a, path } : a))))
          .catch((e) => {
            URL.revokeObjectURL(url);
            setImages((all) => all.filter((a) => a.url !== url));
            onError(e);
          });
      }
    },
    [onError],
  );
  const remove = (url: string) => {
    URL.revokeObjectURL(url);
    setImages((all) => all.filter((a) => a.url !== url));
  };
  /** After sending: the files stay where they are, for the agent to read. */
  const clear = () => {
    images.forEach((a) => URL.revokeObjectURL(a.url));
    setImages([]);
  };

  /** For the prompt's box: paste an image, or drop one on it. */
  const handlers = {
    onPaste: (e: ClipboardEvent) => {
      const files = imagesOf(e.clipboardData.files);
      if (files.length === 0) return;
      e.preventDefault();
      add(files);
    },
    onDragOver: (e: DragEvent) => {
      if (![...e.dataTransfer.items].some((i) => i.kind === "file")) return;
      e.preventDefault();
      setDragging(true);
    },
    onDragLeave: (e: DragEvent) => {
      if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false);
    },
    onDrop: (e: DragEvent) => {
      setDragging(false);
      if (e.dataTransfer.files.length === 0) return;
      e.preventDefault();
      add(e.dataTransfer.files);
    },
  };

  return {
    images,
    add,
    remove,
    clear,
    handlers,
    dragging,
    /** Still being saved: sending now would leave them out. */
    saving: images.some((a) => a.path == null),
  };
}

/** Thumbnails of what's attached, each with a remove button. */
export function AttachmentStrip({ images, onRemove }: { images: Attachment[]; onRemove: (url: string) => void }) {
  if (images.length === 0) return null;
  return (
    <div className="attachments">
      {images.map((a) => (
        <span key={a.url} className={`attachment${a.path ? "" : " saving"}`} title={a.path ?? `Saving ${a.name}…`}>
          <img src={a.url} alt={a.name} />
          <button className="attachment-x" onClick={() => onRemove(a.url)} aria-label={`Remove ${a.name}`}>
            <X size={10} />
          </button>
        </span>
      ))}
    </div>
  );
}

/** The footer's button for picking images from disk. */
export function AttachButton({ onFiles }: { onFiles: (files: File[]) => void }) {
  const input = useRef<HTMLInputElement>(null);
  return (
    <>
      <button
        type="button"
        className="icon-button"
        onClick={() => input.current?.click()}
        title="Attach images (or paste or drop them in)"
        aria-label="Attach images"
      >
        <ImageIcon size={14} />
      </button>
      <input
        ref={input}
        type="file"
        accept="image/png,image/jpeg,image/gif,image/webp"
        multiple
        hidden
        onChange={(e) => {
          onFiles([...(e.target.files ?? [])]);
          e.target.value = "";
        }}
      />
    </>
  );
}
