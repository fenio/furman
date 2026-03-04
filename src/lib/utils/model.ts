import type { ModelMetadata, TensorInfo } from '$lib/types';

// ── Formatting ───────────────────────────────────────────────────────────────

export function formatParams(n: number): string {
  if (n >= 1e12) return (n / 1e12).toFixed(1) + 'T';
  if (n >= 1e9) return (n / 1e9).toFixed(1) + 'B';
  if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M';
  if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K';
  return n.toString();
}

export function formatVram(bytes: number): string {
  if (bytes >= 1024 ** 3) return (bytes / 1024 ** 3).toFixed(1) + ' GB';
  if (bytes >= 1024 ** 2) return (bytes / 1024 ** 2).toFixed(0) + ' MB';
  return (bytes / 1024).toFixed(0) + ' KB';
}

// ── VRAM Estimation ──────────────────────────────────────────────────────────

export interface VramEstimate {
  modelWeights: number;
  kvCache: number;
  overhead: number;
  total: number;
  gpuFit: { tier: number; fits: boolean }[];
}

const GPU_TIERS = [4, 6, 8, 12, 16, 24, 48, 80];

export function estimateVram(meta: ModelMetadata): VramEstimate {
  const modelWeights = meta.total_tensor_bytes;

  // KV cache = 2 × block_count × kv_heads × head_dim × context_length × 2 (FP16)
  let kvCache = 0;
  if (meta.block_count && meta.head_count_kv && meta.head_count && meta.embedding_size && meta.context_length) {
    const headDim = meta.embedding_size / meta.head_count;
    kvCache = 2 * meta.block_count * meta.head_count_kv * headDim * meta.context_length * 2;
  }

  const overhead = Math.round((modelWeights + kvCache) * 0.05);
  const total = modelWeights + kvCache + overhead;

  const gpuFit = GPU_TIERS.map(tier => ({
    tier,
    fits: total <= tier * 1024 ** 3,
  }));

  return { modelWeights, kvCache, overhead, total, gpuFit };
}

// ── Tensor Grouping ──────────────────────────────────────────────────────────

export type TensorCategory = 'attention' | 'ffn' | 'norm' | 'embedding' | 'output' | 'other';

export interface TensorGroup {
  label: string;
  key: string;
  totalBytes: number;
  count: number;
  percentage: number;
}

const CATEGORY_PATTERNS: [TensorCategory, RegExp][] = [
  // Attention: q/k/v/o projections
  ['attention', /(?:attn_|self_attn|attention|\.attn\.|_q\.|_k\.|_v\.|q_proj|k_proj|v_proj|o_proj|qkv_proj)/i],
  // FFN: feed-forward, mlp, gate
  ['ffn', /(?:ffn_|mlp|feed_forward|gate_proj|up_proj|down_proj|fc1|fc2|w1|w2|w3)/i],
  // Normalization layers
  ['norm', /(?:norm|layernorm|rmsnorm|ln_)/i],
  // Embedding layers
  ['embedding', /(?:embed|wte|wpe|token_embd|position_embd)/i],
  // Output/head layers
  ['output', /(?:output\.|lm_head|classifier|cls_|pooler)/i],
];

const CATEGORY_LABELS: Record<TensorCategory, string> = {
  attention: 'Attention',
  ffn: 'Feed-Forward',
  norm: 'Normalization',
  embedding: 'Embedding',
  output: 'Output',
  other: 'Other',
};

const CATEGORY_COLORS: Record<TensorCategory, string> = {
  attention: '#3b82f6',  // blue
  ffn: '#f59e0b',       // amber
  norm: '#10b981',       // emerald
  embedding: '#8b5cf6',  // violet
  output: '#ef4444',     // red
  other: '#6b7280',      // gray
};

function classifyTensor(name: string): TensorCategory {
  for (const [cat, pattern] of CATEGORY_PATTERNS) {
    if (pattern.test(name)) return cat;
  }
  return 'other';
}

export function groupTensorsByCategory(tensors: TensorInfo[]): TensorGroup[] {
  const totals = new Map<TensorCategory, { bytes: number; count: number }>();
  let totalBytes = 0;

  for (const t of tensors) {
    const cat = classifyTensor(t.name);
    const entry = totals.get(cat) ?? { bytes: 0, count: 0 };
    entry.bytes += t.size_bytes;
    entry.count++;
    totals.set(cat, entry);
    totalBytes += t.size_bytes;
  }

  const categories: TensorCategory[] = ['attention', 'ffn', 'norm', 'embedding', 'output', 'other'];
  return categories
    .filter(cat => totals.has(cat))
    .map(cat => {
      const { bytes, count } = totals.get(cat)!;
      return {
        label: CATEGORY_LABELS[cat],
        key: cat,
        totalBytes: bytes,
        count,
        percentage: totalBytes > 0 ? (bytes / totalBytes) * 100 : 0,
      };
    });
}

export function groupTensorsByLayer(tensors: TensorInfo[]): TensorGroup[] {
  const layerMap = new Map<number, { bytes: number; count: number }>();
  let maxBytes = 0;

  for (const t of tensors) {
    // Match layer numbers: blk.N, layers.N, h.N, block.N, etc.
    const m = t.name.match(/(?:blk|layers?|block|h)\.(\d+)/i);
    if (!m) continue;
    const layerNum = parseInt(m[1], 10);
    const entry = layerMap.get(layerNum) ?? { bytes: 0, count: 0 };
    entry.bytes += t.size_bytes;
    entry.count++;
    layerMap.set(layerNum, entry);
    if (entry.bytes > maxBytes) maxBytes = entry.bytes;
  }

  return Array.from(layerMap.entries())
    .sort((a, b) => a[0] - b[0])
    .map(([num, { bytes, count }]) => ({
      label: `Layer ${num}`,
      key: `layer-${num}`,
      totalBytes: bytes,
      count,
      percentage: maxBytes > 0 ? (bytes / maxBytes) * 100 : 0,
    }));
}

export function getCategoryColor(cat: string): string {
  return CATEGORY_COLORS[cat as TensorCategory] ?? CATEGORY_COLORS.other;
}

// ── Comparison Helpers ───────────────────────────────────────────────────────

export type CompareResult = 'better' | 'worse' | 'same' | 'neutral';

/** Compare two numeric values. Lower is better when `lowerIsBetter`, higher otherwise. */
export function compareValues(a: number | null | undefined, b: number | null | undefined, lowerIsBetter: boolean): CompareResult {
  if (a == null || b == null) return 'neutral';
  if (a === b) return 'same';
  if (lowerIsBetter) return a < b ? 'better' : 'worse';
  return a > b ? 'better' : 'worse';
}
