import assert from 'node:assert/strict';
import { parseFragment, type DefaultTreeAdapterMap } from 'parse5';

// Check the parsed HTML tree, not a tag-shaped substring. Template and SVG
// descendants also matter when user-controlled text is rendered by React.
export function assertNoScriptElements(html: string): void {
  const pending: DefaultTreeAdapterMap['node'][] = [parseFragment(html)];
  while (pending.length > 0) {
    const node = pending.pop()!;
    if ('tagName' in node) assert.notEqual(node.tagName, 'script');
    if ('childNodes' in node) pending.push(...node.childNodes);
    if ('content' in node) pending.push(node.content);
  }
}
