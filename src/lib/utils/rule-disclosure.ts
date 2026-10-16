/** Rule lists open their sole item by default, not every form in a long list. */
export function initialExpandedRule(count: number): number | null {
  return count === 1 ? 0 : null;
}

export function expandedRuleAfterRemoval(expanded: number | null, removed: number, remaining: number): number | null {
  if (remaining === 1) return 0;
  if (remaining === 0 || expanded === null || expanded === removed) return null;
  return expanded > removed ? expanded - 1 : expanded;
}
