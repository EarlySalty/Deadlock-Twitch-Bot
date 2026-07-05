import type { JSX, ReactNode } from "react";
import type { MdBlock } from "../lib/knowledge";

/** Inline-Markdown: [Text](url), **fett**, `code`. Reicht für die Wissensbasis. */
function renderInline(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const pattern = /\[([^\]]+)\]\(([^)]+)\)|\*\*([^*]+)\*\*|`([^`]+)`/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  let key = 0;

  while ((match = pattern.exec(text)) !== null) {
    if (match.index > lastIndex) nodes.push(text.slice(lastIndex, match.index));
    if (match[1] !== undefined) {
      nodes.push(
        <a key={key++} href={match[2]} className="md-link">
          {match[1]}
        </a>,
      );
    } else if (match[3] !== undefined) {
      nodes.push(<strong key={key++}>{match[3]}</strong>);
    } else if (match[4] !== undefined) {
      nodes.push(<code key={key++}>{match[4]}</code>);
    }
    lastIndex = pattern.lastIndex;
  }
  if (lastIndex < text.length) nodes.push(text.slice(lastIndex));
  return nodes;
}

export function MdBlocks({ blocks }: { blocks: MdBlock[] }): JSX.Element {
  return (
    <div className="md">
      {blocks.map((block, i) => {
        switch (block.kind) {
          case "h2":
            return <h3 key={i}>{renderInline(block.text ?? "")}</h3>;
          case "h3":
            return <h4 key={i}>{renderInline(block.text ?? "")}</h4>;
          case "li-group":
            return (
              <ul key={i}>
                {(block.items ?? []).map((item, j) => (
                  <li key={j}>{renderInline(item)}</li>
                ))}
              </ul>
            );
          default:
            return <p key={i}>{renderInline(block.text ?? "")}</p>;
        }
      })}
    </div>
  );
}
