import { diffLines } from 'diff';

export interface DiffSource { backend: string; path: string; connection_id: string }
export interface DiffPair { left: DiffSource; right: DiffSource }
export interface DiffLine { kind: 'same' | 'added' | 'removed'; text: string; left: number | null; right: number | null; noNewline: boolean }

export function comparisonDiffPair(comparison: [string, string, string, string, string, string], relative: string): DiffPair {
  const [leftPath, leftBackend, leftId, rightPath, rightBackend, rightId] = comparison;
  const join = (root: string) => root.replace(/\/+$/, '') + '/' + relative;
  return { left: { backend: leftBackend, path: join(leftPath), connection_id: leftId },
    right: { backend: rightBackend, path: join(rightPath), connection_id: rightId } };
}

export function textDiff(left: string, right: string): DiffLine[] {
  if (left.split('\n').length + right.split('\n').length > 10000) {
    throw new Error('Too many lines for the built-in diff (10,000 combined). Compare these files externally.');
  }
  const changes = diffLines(left, right, { timeout: 1000 });
  if (!changes) throw new Error('Diff is too complex for the built-in viewer. Compare these files externally.');
  let leftNumber = 1;
  let rightNumber = 1;
  return changes.flatMap(change => {
    const lines = change.value.split('\n');
    if (lines.at(-1) === '') lines.pop();
    return lines.map((text, index) => ({
      kind: change.added ? 'added' as const : change.removed ? 'removed' as const : 'same' as const,
      text,
      left: change.added ? null : leftNumber++,
      right: change.removed ? null : rightNumber++,
      noNewline: index === lines.length - 1 && !change.value.endsWith('\n'),
    }));
  });
}
