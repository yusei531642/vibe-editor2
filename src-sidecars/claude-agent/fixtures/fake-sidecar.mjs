import readline from 'node:readline';

const version = 1;
const scenario = process.argv[2] || 'happy';
const send = (value) => process.stdout.write(`${JSON.stringify({ version, ...value })}\n`);
const ok = (id, result = {}) => send({ type: 'response', id, ok: true, result });
const event = (value) => send({ type: 'event', event: value });

if (scenario === 'protocol-mismatch') {
  process.stdout.write(`${JSON.stringify({ type: 'hello', version: 99, protocol: 'wrong' })}\n`);
} else {
  send({ type: 'hello', protocol: 'vibe-claude-agent', capabilities: ['fixture'] });
}

readline.createInterface({ input: process.stdin, crlfDelay: Infinity }).on('line', (line) => {
  const request = JSON.parse(line);
  if (scenario === 'crash' && request.method === 'turn') process.exit(23);
  if (request.method === 'spawn') {
    ok(request.id, { sessionId: null });
    if (scenario === 'mcp-options') {
      const server = request.params.mcpServers?.['vibe-team2'];
      event({
        type: 'diagnostic',
        message: `mcp:${server?.type}:${server?.command}:${Array.isArray(server?.args)}:${Boolean(server?.env?.VIBE_TEAM_TOKEN)}:${server?.env?.VIBE_TEAM_ID}:${server?.env?.VIBE_AGENT_ID}:${server?.env?.VIBE_TEAM_ROLE}`
      });
    }
  } else if (['resume', 'fork'].includes(request.method)) {
    ok(request.id, { sessionId: request.params.sessionId });
  } else if (['turn', 'write', 'inject', 'steer'].includes(request.method)) {
    ok(request.id, { accepted: true });
    if (scenario === 'team-delivery') {
      event({ type: 'diagnostic', message: `delivery:${request.method}:${request.params.input}` });
      return;
    }
    if (scenario === 'options') {
      event({
        type: 'diagnostic',
        message: `options:${request.params.model}:${request.params.effort}:${request.params.permission}`
      });
      event({ type: 'turnComplete', interrupted: false });
      return;
    }
    if (scenario === 'invalid-json') {
      process.stdout.write('invalid-json\n');
      return;
    }
    event({ type: 'session', sessionId: 'claude-fixture-session' });
    if (scenario === 'secret') {
      event({ type: 'messageComplete', message: `credential=${process.env.TEST_SECRET}` });
      return;
    }
    event({ type: 'messageDelta', delta: 'fixture delta' });
    event({ type: 'diagnostic', message: 'task running: fixture review' });
    event({ type: 'toolUse', toolName: 'Read', callId: 'tool-1', status: 'started' });
    event({
      type: 'approvalRequest',
      requestId: 'approval-1',
      method: 'Bash',
      reason: 'Fixture approval'
    });
  } else if (request.method === 'respondApproval') {
    ok(request.id, { resolved: true });
    event({ type: 'messageComplete', message: 'fixture complete' });
    event({ type: 'diff', diff: 'Edit changed project files' });
    event({ type: 'usage', inputTokens: 3, cachedInputTokens: 1, outputTokens: 2 });
  } else if (['interrupt', 'stop'].includes(request.method)) {
    ok(request.id, { interrupted: true });
  } else if (request.method === 'dispose') {
    ok(request.id, { disposed: true });
    setImmediate(() => process.exit(0));
  } else {
    ok(request.id);
  }
});
