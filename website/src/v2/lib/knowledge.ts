/**
 * SSOT-Loader: liest die Markdown-Wissensbasis des Bots
 * (`rust/knowledge/bot/*.md`) zur Build-Zeit ein. Wer das Bot-Wissen pflegt,
 * pflegt damit automatisch auch FAQ- und Feature-Seite.
 */

export interface MdBlock {
  kind: "h2" | "h3" | "p" | "li-group";
  text?: string;
  items?: string[];
}

export interface FaqItem {
  question: string;
  blocks: MdBlock[];
}

export interface FaqGroup {
  key: string;
  title: string;
  intro: MdBlock[];
  items: FaqItem[];
}

const raw = import.meta.glob("../../../../rust/knowledge/bot/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

function fileKey(path: string): string {
  const name = path.split("/").pop() ?? path;
  return name.replace(/\.md$/, "");
}

/** Zeilenweiser Mini-Parser: Überschriften, Absätze, Listen. */
export function parseMarkdown(md: string): MdBlock[] {
  const blocks: MdBlock[] = [];
  let paragraph: string[] = [];
  let list: string[] = [];

  const flushParagraph = () => {
    if (paragraph.length) {
      blocks.push({ kind: "p", text: paragraph.join(" ") });
      paragraph = [];
    }
  };
  const flushList = () => {
    if (list.length) {
      blocks.push({ kind: "li-group", items: list });
      list = [];
    }
  };

  for (const line of md.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) {
      flushParagraph();
      flushList();
      continue;
    }
    if (trimmed.startsWith("### ")) {
      flushParagraph();
      flushList();
      blocks.push({ kind: "h3", text: trimmed.slice(4) });
    } else if (trimmed.startsWith("## ")) {
      flushParagraph();
      flushList();
      blocks.push({ kind: "h2", text: trimmed.slice(3) });
    } else if (trimmed.startsWith("# ")) {
      // Dokumenttitel wird von der Seite gesetzt, nicht doppelt gerendert.
      flushParagraph();
      flushList();
    } else if (/^[-*] /.test(trimmed)) {
      flushParagraph();
      list.push(trimmed.slice(2));
    } else {
      flushList();
      paragraph.push(trimmed);
    }
  }
  flushParagraph();
  flushList();
  return blocks;
}

/** Zerlegt eine faq-*.md in Intro + Frage-Antwort-Paare (### = Frage). */
function splitFaq(md: string): { intro: MdBlock[]; items: FaqItem[] } {
  const blocks = parseMarkdown(md);
  const intro: MdBlock[] = [];
  const items: FaqItem[] = [];
  let current: FaqItem | null = null;

  for (const block of blocks) {
    if (block.kind === "h3") {
      current = { question: block.text ?? "", blocks: [] };
      items.push(current);
    } else if (current) {
      current.blocks.push(block);
    } else if (block.kind !== "h2") {
      intro.push(block);
    }
  }
  return { intro, items };
}

/** Kuratierte Reihenfolge + deutsche Gruppentitel der FAQ-Themen. */
const FAQ_ORDER: Array<{ key: string; title: string }> = [
  { key: "faq-einstieg", title: "Einstieg" },
  { key: "faq-funktionen", title: "Funktionen" },
  { key: "faq-raids", title: "Raids & Netzwerk" },
  { key: "faq-stats-overlay", title: "Stats & Overlay" },
  { key: "faq-analytics", title: "Analytics" },
  { key: "faq-community", title: "Community" },
  { key: "faq-werbung", title: "Announcements & Werbung" },
  { key: "faq-plaene", title: "Pläne & Kosten" },
  { key: "faq-affiliate", title: "Affiliate" },
  { key: "faq-support", title: "Support" },
];

export function loadFaqGroups(): FaqGroup[] {
  const byKey = new Map<string, string>();
  for (const [path, content] of Object.entries(raw)) {
    byKey.set(fileKey(path), content);
  }
  const groups: FaqGroup[] = [];
  for (const { key, title } of FAQ_ORDER) {
    const content = byKey.get(key);
    if (!content) continue;
    const { intro, items } = splitFaq(content);
    if (items.length) groups.push({ key, title, intro, items });
  }
  return groups;
}

/** Feature-Detailtexte aus den Nicht-FAQ-Wissensdateien. */
export function loadKnowledgeDoc(key: string): MdBlock[] | null {
  for (const [path, content] of Object.entries(raw)) {
    if (fileKey(path) === key) return parseMarkdown(content);
  }
  return null;
}
