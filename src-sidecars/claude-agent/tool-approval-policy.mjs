const TEAM_TOOL_PREFIX = 'mcp__vibe-team2__';

const WORKSPACE_AUTO_ALLOWED_TEAM_TOOLS = new Set([
  'team_diagnostics',
  'team_get_tasks',
  'team_info',
  'team_list_role_profiles',
  'team_read',
  'team_report',
  'team_status',
  'team_update_task'
]);

export function shouldAutoAllowTool(toolName, permission) {
  if (!toolName.startsWith(TEAM_TOOL_PREFIX)) return false;
  if (permission === 'full') return true;
  return WORKSPACE_AUTO_ALLOWED_TEAM_TOOLS.has(toolName.slice(TEAM_TOOL_PREFIX.length));
}
