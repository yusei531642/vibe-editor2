import readline from 'node:readline';
import { randomUUID } from 'node:crypto';
import { query } from '@anthropic-ai/claude-agent-sdk';

const PROTOCOL = 'vibe-claude-agent';
const VERSION = 1;
const state = {
  endpointId: null,
  cwd: null,
  systemPrompt: null,
  sessionId: null,
  forkNext: false,
  activeQuery: null,
  activeTask: null,
  disposed: false,
  approvals: new Map()
};

function send(message) {
  process.stdout.write(`${JSON.stringify({ version: VERSION, ...message })}\n`);
}

function response(id, result = {}) {
  send({ type: 'response', id, ok: true, result });
}

function failure(id, code, message, recoverable = false) {
  send({ type: 'response', id, ok: false, error: { code, message: safeText(message), recoverable } });
}

function event(payload) {
  send({ type: 'event', event: payload });
}

function safeText(value) {
  return String(value ?? '')
    .replace(/(api[_-]?key|token|authorization)\s*[:=]\s*\S+/gi, '$1=<redacted>')
    .slice(0, 16_384);
}

function approvalReason(toolName, options) {
  return safeText(options.title || options.description || options.decisionReason || `${toolName} requires approval`);
}

function canUseTool(toolName, _input, options) {
  const requestId = options.requestId || options.toolUseID || randomUUID();
  event({
    type: 'approvalRequest',
    requestId,
    method: toolName,
    reason: approvalReason(toolName, options)
  });
  return new Promise((resolve) => {
    const abort = () => {
      state.approvals.delete(requestId);
      resolve({ behavior: 'deny', message: 'Runtime interrupted', interrupt: true });
    };
    options.signal.addEventListener('abort', abort, { once: true });
    state.approvals.set(requestId, {
      resolve: (decision) => {
        options.signal.removeEventListener('abort', abort);
        resolve(decision);
      },
      suggestions: options.suggestions
    });
  });
}

function hooks() {
  const completed = async (input, toolUseId) => {
    event({
      type: 'toolUse',
      toolName: input.tool_name || 'tool',
      callId: toolUseId || null,
      status: 'completed'
    });
    return { continue: true };
  };
  const failed = async (input, toolUseId) => {
    event({
      type: 'toolUse',
      toolName: input.tool_name || 'tool',
      callId: toolUseId || null,
      status: 'failed',
      detail: safeText(input.error || 'Tool failed')
    });
    return { continue: true };
  };
  return {
    PostToolUse: [{ hooks: [completed] }],
    PostToolUseFailure: [{ hooks: [failed] }]
  };
}

function contentBlocks(message) {
  return Array.isArray(message?.message?.content) ? message.message.content : [];
}

function emitAssistant(message) {
  const text = [];
  for (const block of contentBlocks(message)) {
    if (block.type === 'text' && block.text) text.push(block.text);
    if (block.type === 'tool_use') {
      event({
        type: 'toolUse',
        toolName: block.name || 'tool',
        callId: block.id || null,
        status: 'started'
      });
      if (block.name === 'Edit' || block.name === 'Write' || block.name === 'MultiEdit') {
        event({ type: 'diff', diff: `${block.name} changed project files` });
      }
    }
  }
  if (text.length > 0) event({ type: 'messageComplete', message: safeText(text.join('')) });
}

function emitPartial(message) {
  const delta = message?.event?.delta?.text;
  if (typeof delta === 'string' && delta.length > 0) {
    event({ type: 'messageDelta', delta: safeText(delta) });
  }
}

function emitUsage(message) {
  const usage = message.usage || {};
  event({
    type: 'usage',
    inputTokens: Number(usage.input_tokens || 0),
    cachedInputTokens: Number(usage.cache_read_input_tokens || 0),
    outputTokens: Number(usage.output_tokens || 0)
  });
}

function projectSdkMessage(message) {
  if (message?.session_id && message.session_id !== state.sessionId) {
    state.sessionId = message.session_id;
    event({ type: 'session', sessionId: state.sessionId });
  }
  if (message?.type === 'stream_event') emitPartial(message);
  else if (message?.type === 'assistant') emitAssistant(message);
  else if (message?.type === 'result') {
    emitUsage(message);
    if (message.subtype !== 'success') {
      event({
        type: 'error',
        code: 'claude_query_failed',
        message: safeText(message.errors?.join('; ') || message.subtype),
        recoverable: true
      });
    }
  } else if (message?.type === 'tool_progress') {
    event({
      type: 'toolUse',
      toolName: message.tool_name || 'tool',
      callId: message.tool_use_id || null,
      status: 'running'
    });
  } else if (message?.type === 'system' && String(message.subtype).startsWith('task_')) {
    const detail = message.summary || message.description || message.task_id || message.subtype;
    event({ type: 'diagnostic', message: safeText(`task ${message.status || message.subtype}: ${detail}`) });
  }
}

function queryOptions(abortController) {
  const options = {
    abortController,
    canUseTool,
    cwd: state.cwd || undefined,
    hooks: hooks(),
    includeHookEvents: true,
    includePartialMessages: true,
    permissionMode: 'default',
    settingSources: ['user', 'project', 'local'],
    systemPrompt: state.systemPrompt
      ? { type: 'preset', preset: 'claude_code', append: state.systemPrompt }
      : { type: 'preset', preset: 'claude_code' }
  };
  if (state.sessionId) options.resume = state.sessionId;
  if (state.forkNext) options.forkSession = true;
  if (process.env.VIBE_CLAUDE_COMMAND) {
    options.pathToClaudeCodeExecutable = process.env.VIBE_CLAUDE_COMMAND;
  }
  return options;
}

async function startTurn(prompt) {
  if (state.activeTask) throw Object.assign(new Error('A Claude turn is already active'), { recoverable: true });
  const abortController = new AbortController();
  const activeQuery = query({ prompt, options: queryOptions(abortController) });
  state.activeQuery = activeQuery;
  state.forkNext = false;
  state.activeTask = (async () => {
    try {
      for await (const message of activeQuery) projectSdkMessage(message);
    } catch (error) {
      if (!abortController.signal.aborted && !state.disposed) {
        event({ type: 'error', code: 'claude_sdk_error', message: safeText(error?.message || error), recoverable: true });
      }
    } finally {
      state.activeQuery = null;
      state.activeTask = null;
    }
  })();
}

async function interruptActive() {
  if (!state.activeQuery) return;
  await state.activeQuery.interrupt();
}

async function handle(request) {
  const { id, method, params = {} } = request;
  switch (method) {
    case 'spawn':
      state.endpointId = params.endpointId;
      state.cwd = params.cwd || null;
      state.systemPrompt = params.systemPrompt || null;
      response(id, { sessionId: state.sessionId });
      return;
    case 'resume':
      state.sessionId = params.sessionId;
      state.forkNext = false;
      response(id, { sessionId: state.sessionId });
      return;
    case 'fork':
      state.sessionId = params.sessionId;
      state.forkNext = true;
      response(id, { sessionId: state.sessionId });
      return;
    case 'turn':
    case 'write':
    case 'inject':
      await startTurn(String(params.input ?? ''));
      response(id, { accepted: true, sessionId: state.sessionId });
      return;
    case 'steer':
      await interruptActive();
      await state.activeTask?.catch(() => {});
      await startTurn(String(params.input ?? ''));
      response(id, { accepted: true });
      return;
    case 'interrupt':
    case 'stop':
      await interruptActive();
      response(id, { interrupted: true });
      return;
    case 'respondApproval': {
      const pending = state.approvals.get(params.requestId);
      if (!pending) {
        failure(id, 'runtime_approval_not_pending', 'Approval request is not pending', true);
        return;
      }
      state.approvals.delete(params.requestId);
      if (params.decision === 'accept' || params.decision === 'acceptForSession') {
        pending.resolve({
          behavior: 'allow',
          updatedPermissions: params.decision === 'acceptForSession' ? pending.suggestions : undefined
        });
      } else {
        pending.resolve({ behavior: 'deny', message: 'User declined the tool request', interrupt: params.decision === 'cancel' });
      }
      response(id, { resolved: true });
      return;
    }
    case 'dispose':
      state.disposed = true;
      await interruptActive();
      response(id, { disposed: true });
      setImmediate(() => process.exit(0));
      return;
    default:
      failure(id, 'runtime_sidecar_method_unknown', `Unknown method: ${method}`, true);
  }
}

send({
  type: 'hello',
  protocol: PROTOCOL,
  capabilities: ['sessions', 'streaming', 'tools', 'permissions', 'interrupt', 'resume', 'fork']
});

const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
lines.on('line', (line) => {
  let request;
  try {
    request = JSON.parse(line);
  } catch {
    event({ type: 'error', code: 'runtime_sidecar_protocol', message: 'Invalid JSON request', recoverable: false });
    return;
  }
  if (request?.type !== 'request' || request.version !== VERSION || typeof request.id !== 'string') {
    if (typeof request?.id === 'string') failure(request.id, 'runtime_sidecar_protocol', 'Unsupported request envelope', false);
    return;
  }
  void handle(request).catch((error) => {
    failure(request.id, 'runtime_sidecar_operation_failed', error?.message || error, error?.recoverable === true);
  });
});

lines.on('close', async () => {
  state.disposed = true;
  await interruptActive().catch(() => undefined);
  process.exit(0);
});
