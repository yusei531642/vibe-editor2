export type V2ComposerIntent = 'message' | 'goal' | 'team';

export interface V2ComposerAttachment {
  path: string;
  name: string;
}

export interface BuildV2RuntimeInputRequest {
  text: string;
  intent: V2ComposerIntent;
  attachments: V2ComposerAttachment[];
  activeGoal: string | null;
}

export function attachmentName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

export function buildV2RuntimeInput({
  text,
  intent,
  attachments,
  activeGoal,
}: BuildV2RuntimeInputRequest): string {
  const trimmed = text.trim();
  const sections: string[] = [];

  if (intent === 'goal') {
    sections.push(`Create and pursue this as the active goal:\n${trimmed}`);
  } else {
    if (activeGoal) sections.push(`Current active goal:\n${activeGoal}`);
    if (intent === 'team') {
      sections.push(`Create a team to pursue this request:\n${trimmed}`);
    } else if (trimmed) {
      sections.push(trimmed);
    }
  }

  if (attachments.length > 0) {
    const paths = attachments.map(({ path }) => `- ${JSON.stringify(path)}`).join('\n');
    sections.push(`Files explicitly attached by the user:\n${paths}`);
  }

  if (sections.length === 0) {
    return 'Continue the current task using the attached context.';
  }
  return sections.join('\n\n');
}
