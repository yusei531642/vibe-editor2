import type { RuntimeEngine } from '../../../types/agent-runtime';

export async function reportV2RuntimeActionError(
  action: Promise<void>,
  engine: RuntimeEngine,
  onError: (message: string, engine: RuntimeEngine) => void
): Promise<void> {
  try {
    await action;
  } catch (error) {
    onError(error instanceof Error ? error.message : String(error), engine);
  }
}
