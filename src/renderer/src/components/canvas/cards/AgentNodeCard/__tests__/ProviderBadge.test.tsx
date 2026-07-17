import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { ProviderBadge } from '../ProviderBadge';

describe('ProviderBadge', () => {
  afterEach(cleanup);

  it.each([
    ['codex-native', 'codex'],
    ['claude-native', 'claude'],
    ['pty', 'pty'],
    ['api', 'api']
  ] as const)('renders %s as the %s provider badge', (provider, label) => {
    render(<ProviderBadge provider={provider} />);
    expect(screen.getByLabelText(`provider: ${label}`)).toHaveAttribute(
      'data-provider',
      provider
    );
  });

  it('shows an explicit indicator when Claude native falls back to PTY', () => {
    render(<ProviderBadge provider="pty" fallbackFrom="claude-native" />);
    expect(screen.getByText('fallback claude→pty')).toHaveAttribute(
      'data-fallback-from',
      'claude-native'
    );
    expect(screen.getByLabelText('native unavailable, fallback: claude→pty')).toBeVisible();
  });

  it('renders no badge while the provider is unresolved', () => {
    const { container } = render(<ProviderBadge />);
    expect(container).toBeEmptyDOMElement();
  });

  it('does not claim fallback for an explicitly selected PTY runtime', () => {
    render(<ProviderBadge provider="pty" />);
    expect(screen.queryByText(/fallback/)).toBeNull();
  });
});
