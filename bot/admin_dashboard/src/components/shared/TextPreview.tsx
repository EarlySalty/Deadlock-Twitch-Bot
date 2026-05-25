interface TextPreviewProps {
  value: string;
  emptyMessage?: string;
}

type Segment = {
  type: 'text' | 'code';
  content: string;
};

function splitSegments(value: string): Segment[] {
  const segments: Segment[] = [];
  const parts = value.split(/```/g);
  parts.forEach((part, index) => {
    segments.push({
      type: index % 2 === 1 ? 'code' : 'text',
      content: part,
    });
  });
  return segments;
}

function renderTextBlock(block: string, key: string) {
  const trimmed = block.trim();
  if (!trimmed) {
    return null;
  }

  if (/^(?:-|\*)\s+/m.test(trimmed) && trimmed.split('\n').every((line) => /^(?:-|\*)\s+/.test(line.trim()))) {
    return (
      <ul key={key} className="list-disc space-y-2 pl-5 text-sm leading-7 text-white/90">
        {trimmed.split('\n').map((line, index) => (
          <li key={`${key}-${index}`}>{line.replace(/^(?:-|\*)\s+/, '')}</li>
        ))}
      </ul>
    );
  }

  const headingMatch = trimmed.match(/^(#{1,3})\s+(.+)$/);
  if (headingMatch) {
    const level = headingMatch[1].length;
    const title = headingMatch[2];
    const className =
      level === 1
        ? 'text-2xl font-semibold text-white'
        : level === 2
          ? 'text-xl font-semibold text-white'
          : 'text-lg font-semibold text-white';
    return (
      <h3 key={key} className={className}>
        {title}
      </h3>
    );
  }

  return (
    <p key={key} className="whitespace-pre-wrap break-words text-sm leading-7 text-white/90">
      {trimmed}
    </p>
  );
}

export function TextPreview({ value, emptyMessage = 'Noch kein Inhalt vorhanden.' }: TextPreviewProps) {
  const normalizedValue = value.trim();
  if (!normalizedValue) {
    return <p className="text-sm leading-7 text-text-secondary">{emptyMessage}</p>;
  }

  return (
    <div className="space-y-4">
      {splitSegments(value).map((segment, segmentIndex) => {
        if (segment.type === 'code') {
          return (
            <pre
              key={`code-${segmentIndex}`}
              className="overflow-x-auto rounded-[1.25rem] border border-white/10 bg-slate-950/60 p-4 text-sm leading-6 text-amber-100"
            >
              <code>{segment.content.trim()}</code>
            </pre>
          );
        }

        return (
          <div key={`text-${segmentIndex}`} className="space-y-4">
            {segment.content
              .split(/\n\s*\n/g)
              .map((block, blockIndex) => renderTextBlock(block, `block-${segmentIndex}-${blockIndex}`))}
          </div>
        );
      })}
    </div>
  );
}
