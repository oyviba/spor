import DOMPurify from "dompurify";
import { marked } from "marked";

marked.setOptions({ gfm: true, breaks: false });

/**
 * Agent text as Markdown. Sanitized: an agent's output is untrusted, and in
 * the app a script here could call spor's commands.
 */
export function Markdown({ text }: { text: string }) {
  const html = DOMPurify.sanitize(marked.parse(text, { async: false }) as string, {
    FORBID_TAGS: ["style", "form", "input", "img"],
  });
  return <div className="md" dangerouslySetInnerHTML={{ __html: html }} />;
}
