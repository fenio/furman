import './styles.css';

const target = document.getElementById('app')!;
const implementation = new URLSearchParams(location.search).get('target') ?? 'svelte-current';

switch (implementation) {
  case 'svelte-current': {
    const [{ mount }, { default: Benchmark }] = await Promise.all([
      import('svelte'),
      import('./FileListBenchmark.svelte'),
    ]);
    mount(Benchmark, { target });
    break;
  }
  case 'svelte-optimized': {
    const [{ mount }, { default: Benchmark }] = await Promise.all([
      import('svelte'),
      import('./OptimizedSvelteBenchmark.svelte'),
    ]);
    mount(Benchmark, { target });
    break;
  }
  case 'solid': {
    const { mountSolid } = await import('./SolidBenchmark');
    mountSolid(target);
    break;
  }
  case 'vanilla': {
    const { mountVanilla } = await import('./vanilla');
    mountVanilla(target);
    break;
  }
  default:
    throw new Error(`Unknown benchmark target: ${implementation}`);
}
