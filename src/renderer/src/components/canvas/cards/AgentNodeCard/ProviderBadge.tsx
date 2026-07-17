import type { RuntimeProvider } from '../../../../../../types/agent-runtime';

function label(provider: RuntimeProvider): string {
  if (provider === 'codex-native') return 'codex';
  if (provider === 'claude-native') return 'claude';
  return provider;
}

export function ProviderBadge({
  provider,
  fallbackFrom
}: {
  provider?: RuntimeProvider;
  fallbackFrom?: Extract<RuntimeProvider, 'codex-native' | 'claude-native'> | null;
}): JSX.Element | null {
  if (!provider) return null;
  const selected = provider;
  const fallbackLabel = fallbackFrom ? `${label(fallbackFrom)}→${label(selected)}` : null;
  return (
    <span className="canvas-agent-provider-group">
      <span
        className={`canvas-agent-provider canvas-agent-provider--${label(selected)}`}
        data-provider={selected}
        aria-label={`provider: ${label(selected)}`}
      >
        {label(selected)}
      </span>
      {fallbackLabel ? (
        <span
          className="canvas-agent-provider__fallback"
          data-fallback-from={fallbackFrom}
          aria-label={`native unavailable, fallback: ${fallbackLabel}`}
          title={`native unavailable: ${fallbackLabel}`}
        >
          fallback {fallbackLabel}
        </span>
      ) : null}
    </span>
  );
}
