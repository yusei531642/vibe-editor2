import { useCallback, useEffect, useState } from 'react';

export type MotionPreference = 'full' | 'reduced';

function appRequestsReducedMotion(): boolean {
  if (typeof document === 'undefined') return false;
  const root = document.documentElement;
  return (
    root.dataset.motion === 'reduced' ||
    root.dataset.motion === 'none' ||
    root.dataset.reducedMotion === 'true'
  );
}

function systemRequestsReducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    typeof window.matchMedia === 'function' &&
    window.matchMedia('(prefers-reduced-motion: reduce)').matches
  );
}

/** OS 設定と renderer が html に反映する Motion 設定を一つの値として購読する。 */
export function useReducedMotion(override?: MotionPreference): boolean {
  const read = useCallback(
    (): boolean =>
      override
        ? override === 'reduced'
        : appRequestsReducedMotion() || systemRequestsReducedMotion(),
    [override]
  );
  const [reduced, setReduced] = useState(read);

  useEffect(() => {
    setReduced(read());
    if (override || typeof window === 'undefined') return;

    const media = window.matchMedia?.('(prefers-reduced-motion: reduce)');
    const update = (): void => setReduced(appRequestsReducedMotion() || Boolean(media?.matches));
    media?.addEventListener('change', update);

    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['data-motion', 'data-reduced-motion']
    });
    return () => {
      media?.removeEventListener('change', update);
      observer.disconnect();
    };
  }, [override, read]);

  return reduced;
}
