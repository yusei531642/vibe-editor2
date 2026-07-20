import type { Page } from 'playwright/test';
import { readFile } from 'node:fs/promises';
import { extname, resolve } from 'node:path';

const CONTENT_TYPES: Record<string, string> = {
  '.css': 'text/css',
  '.html': 'text/html',
  '.js': 'text/javascript',
  '.svg': 'image/svg+xml',
  '.woff2': 'font/woff2'
};

export async function installMockApi(
  page: Page,
  options: { restore?: boolean; theme?: 'light' | 'dark' | 'claude-light' | 'claude-dark' } = {}
): Promise<void> {
  const dist = resolve(process.cwd(), 'dist');
  await page.route('http://vibe.local/**', async (route) => {
    const pathname = decodeURIComponent(new URL(route.request().url()).pathname);
    const file = resolve(dist, pathname === '/' ? 'index.html' : `.${pathname}`);
    if (!file.startsWith(`${dist}/`) && file !== resolve(dist, 'index.html')) {
      await route.abort();
      return;
    }
    try {
      await route.fulfill({
        body: await readFile(file),
        contentType: CONTENT_TYPES[extname(file)] ?? 'application/octet-stream'
      });
    } catch {
      await route.fulfill({ status: 404, body: 'not found' });
    }
  });
  await page.addInitScript(({ restore, theme }) => {
    const callbacks = new Map<number, (event: unknown) => void>();
    const listeners = new Map<string, Map<number, number>>();
    let nextCallback = 1;
    let nextListener = 1;
    const runtimeEvents = [
      {
        endpointId: 'native-leader', epoch: 101, sequence: 1, kind: 'lifecycle',
        payload: { type: 'lifecycle', state: 'spawning', detail: 'codex-native' },
        timestamp: '2026-07-17T00:00:00Z'
      },
      {
        endpointId: 'native-leader', epoch: 101, sequence: 2, kind: 'lifecycle',
        payload: { type: 'lifecycle', state: 'ready', detail: null },
        timestamp: '2026-07-17T00:00:01Z'
      },
      {
        endpointId: 'native-leader', epoch: 101, sequence: 3, kind: 'approvalRequest',
        payload: {
          type: 'approvalRequest', requestId: 'approval-1', method: 'shell',
          reason: 'Run deterministic verification', command: 'npm test', cwd: null
        },
        timestamp: '2026-07-17T00:00:02Z'
      }
    ];
    const snapshot = restore ? {
      teamId: 'team-e2e',
      endpoints: [{
        teamId: 'team-e2e', agentId: 'leader-e2e', endpointId: 'native-leader',
        backend: 'native', sessionId: 'thread-e2e', taskIds: [], live: false,
        provider: 'codex-native', restoreState: 'reconnectable'
      }],
      runtimeEvents,
      retainedEventCursors: runtimeEvents.map((event) => ({
        endpointId: event.endpointId,
        epoch: event.epoch,
        sequence: event.sequence,
        timestamp: event.timestamp
      })),
      runtimeDroppedCount: 0
    } : null;

    Object.assign(window, {
      __TAURI_INTERNALS__: {
        transformCallback(callback: (event: unknown) => void) {
          const id = nextCallback++;
          callbacks.set(id, callback);
          return id;
        },
        unregisterCallback(id: number) { callbacks.delete(id); },
        async invoke(command: string, args: Record<string, unknown>) {
          if (command === 'plugin:event|listen') {
            const event = String(args.event);
            const id = nextListener++;
            const eventListeners = listeners.get(event) ?? new Map<number, number>();
            eventListeners.set(id, Number(args.handler));
            listeners.set(event, eventListeners);
            return id;
          }
          if (command === 'plugin:event|unlisten') return null;
          return null;
        }
      },
      __TAURI_EVENT_PLUGIN_INTERNALS__: {
        unregisterListener(event: string, id: number) { listeners.get(event)?.delete(id); }
      },
      __emitTauri(event: string, payload: unknown) {
        for (const [id, callbackId] of listeners.get(event) ?? []) {
          callbacks.get(callbackId)?.({ event, id, payload });
        }
      }
    });

    const noop = async () => undefined;
    const api = {
      settings: {
        load: async () => ({
          schemaVersion: 1, language: 'en', theme: theme ?? 'light',
          teamSceneV2: true, hasCompletedOnboarding: true, lastOpenedRoot: null
        }),
        save: noop
      },
      app: {
        checkClaude: async () => ({ ok: true, version: 'mock' }),
        restoreAuthorizedProjectRoot: async () => restore ? '/mock/project' : '',
        pickAndActivateProjectRoot: async () => null,
        reconfirmProjectRoot: async () => null,
        setWindowTitle: noop,
        setWindowEffects: async () => ({ applied: false }),
        getTeamHubInfo: async () => ({ socket: 'mock', token: 'mock', bridgePath: 'mock' }),
        setupTeamMcp: async () => ({ ok: true, changed: false }),
        setRoleProfileSummary: noop
      },
      team: {
        restoreSnapshot: async (_projectRoot: string) => snapshot,
        projectionSnapshot: async () => snapshot,
        memberCommand: async () => ({ action: 'respondApproval', affectedAgentIds: ['leader-e2e'] })
      },
      agentRuntime: {
        modelCatalog: async (engine: 'claude' | 'codex') => ({
          engine,
          models: [{
            id: engine === 'claude' ? 'claude-sonnet-4-6' : 'gpt-5.4',
            label: engine === 'claude' ? 'Sonnet 4.6' : 'GPT-5.4',
            description: 'E2E mock model',
            isDefault: true,
            defaultEffort: 'high',
            supportedEfforts: ['low', 'medium', 'high']
          }]
        }),
        onEventReady: async () => () => undefined,
        registerClaudeEndpoint: async (request: { endpointId: string }) => ({
          endpointId: request.endpointId,
          sessionId: 'session-e2e'
        }),
        registerCodexEndpoint: async (request: { endpointId: string }) => ({
          endpointId: request.endpointId,
          threadId: 'thread-e2e'
        }),
        spawnTurn: async (request: { endpointId: string }) => ({
          endpointId: request.endpointId
        }),
        interrupt: async (endpointId: string) => ({ endpointId }),
        dispose: async (endpointId: string) => ({ endpointId }),
        reconnectCodex: async () => ({ endpointId: 'native-leader', threadId: 'thread-e2e' }),
        respondApproval: async () => ({ endpointId: 'native-leader' })
      },
      teamState: { read: async () => null },
      teamHistory: { list: async () => [], save: noop },
      worktree: {
        snapshot: async () => ({ schemaVersion: 1, teamId: 'team-e2e', assignments: [], mergeQueue: [] })
      },
      roleProfiles: { load: async () => [] },
      terminalTabs: { load: async () => null },
      git: { status: async () => ({ branch: 'main', ahead: 0, behind: 0, files: [] }) },
      sessions: { list: async () => [] },
      ping: async () => 'pong'
    };
    Object.assign(window, { api });
  }, { restore: options.restore ?? false, theme: options.theme ?? 'light' });
}

export async function emitRecruit(page: Page, state: 'requested' | 'ready'): Promise<void> {
  await page.evaluate((nextState) => {
    const emit = (window as unknown as { __emitTauri: (event: string, payload: unknown) => void })
      .__emitTauri;
    if (nextState === 'requested') {
      emit('team:recruit-request', {
        teamId: 'team-e2e', requesterAgentId: 'leader-e2e', requesterRole: 'leader',
        newAgentId: 'worker-e2e', roleProfileId: 'programmer', engine: 'codex',
        runtimeProvider: 'codex-native'
      });
      // 実配線と同じく request 直後に spawning へ進める。placeholder が
      // "Starting" (v2.recruit.spawning) を表示する golden path を検証するため。
      emit('team:recruit-lifecycle', {
        teamId: 'team-e2e', agentId: 'worker-e2e', roleProfileId: 'programmer',
        sequence: 1, state: 'spawning', endpointId: null, sessionId: null,
        taskIds: [], reason: null
      });
    } else {
      emit('team:recruit-lifecycle', {
        teamId: 'team-e2e', agentId: 'worker-e2e', roleProfileId: 'programmer',
        sequence: 2, state: 'ready', endpointId: 'native-worker', sessionId: 'thread-worker',
        taskIds: [], reason: null
      });
    }
  }, state);
}
