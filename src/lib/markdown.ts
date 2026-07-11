import { marked } from "marked";

const allowedTags = new Set([
  "a", "blockquote", "br", "code", "del", "em", "h1", "h2", "h3", "h4", "h5", "h6",
  "hr", "li", "ol", "p", "pre", "strong", "table", "tbody", "td", "th", "thead", "tr", "ul",
]);

function safeHref(value: string): string | null {
  try {
    const url = new URL(value, "https://gist.invalid");
    if (url.protocol === "http:" || url.protocol === "https:" || url.protocol === "mailto:") {
      return value;
    }
  } catch {
    // Invalid links are rendered as plain text.
  }
  return null;
}

function sanitizeHtml(html: string): string {
  const template = document.createElement("template");
  template.innerHTML = html;
  const elements = [...template.content.querySelectorAll("*")];

  for (const element of elements) {
    const tag = element.tagName.toLowerCase();
    if (!allowedTags.has(tag)) {
      element.replaceWith(document.createTextNode(element.textContent ?? ""));
      continue;
    }

    for (const attribute of [...element.attributes]) {
      const allowed = tag === "a" && (attribute.name === "href" || attribute.name === "title")
        || tag === "code" && attribute.name === "class";
      if (!allowed) element.removeAttribute(attribute.name);
    }

    if (tag === "a") {
      const href = element.getAttribute("href");
      if (!href) continue;
      const safe = safeHref(href);
      if (safe) {
        element.setAttribute("href", safe);
        element.setAttribute("rel", "noopener noreferrer");
      } else {
        element.removeAttribute("href");
      }
    }
  }

  return template.innerHTML;
}

export function renderSafeMarkdown(content: string): string {
  return sanitizeHtml(marked.parse(content, { breaks: true, gfm: true }) as string);
}
