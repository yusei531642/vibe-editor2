import { expect, test } from 'playwright/test';
import { emitRecruit, installMockApi } from './mock-api';

test('Home → send → recruit lifecycle → approval handling', async ({ page }) => {
  await installMockApi(page, { restore: true, theme: 'light' });
  await page.goto('http://vibe.local/');

  const composer = page.getByRole('textbox', { name: 'Enter instructions' });
  await expect(composer).toBeFocused();
  await composer.fill('Implement the golden path');
  await page.getByRole('button', { name: 'Send' }).click();
  await expect(page.getByText('Implement the golden path')).toBeVisible();

  await emitRecruit(page, 'requested');
  await page.getByRole('button', { name: 'Canvas' }).click();
  await expect(page.getByText('Starting')).toBeVisible();
  await emitRecruit(page, 'ready');
  await expect(page.getByText('Joined the team')).toBeVisible();

  await page.getByRole('button', { name: /Open 1 pending approvals/ }).click();
  // Activity feed 側にも同文言の要素があるため Approval Center にスコープする
  await expect(
    page.getByLabel('Approval Center').getByText('Run deterministic verification')
  ).toBeVisible();
  await page.getByRole('button', { name: 'Accept', exact: true }).click();
  await expect(page.getByText('No pending approvals')).toBeVisible();
});
