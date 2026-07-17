import { expect, test } from 'playwright/test';
import { installMockApi } from './mock-api';

const MATRIX = [
  { width: 1440, height: 900 },
  { width: 1024, height: 768 },
  { width: 768, height: 800 }
] as const;

// DESIGN.md の Light/Dark は製品の正準テーマ (claude-light / claude-dark) を指す。
// 汎用 'dark' テーマは v2-shell の dark セレクタ対象外で light と同一描画になるため使わない。
const THEME_MATRIX = [
  { theme: 'claude-light', label: 'light' },
  { theme: 'claude-dark', label: 'dark' }
] as const;

for (const { theme, label } of THEME_MATRIX) {
  for (const viewport of MATRIX) {
    test(`Home ${viewport.width}x${viewport.height} ${label}`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await installMockApi(page, { theme });
      await page.goto('http://vibe.local/');
      await expect(page.getByRole('textbox', { name: 'Enter instructions' })).toBeFocused();
      // settings load 完了前は DEFAULT_SETTINGS のテーマのままなので、applyTheme が
      // mock settings のテーマを DOM に反映するまで待つ (light/dark が同一画像になる race 対策)
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      await page.addStyleTag({
        content: '*,*::before,*::after{animation:none!important;transition:none!important}'
      });
      await expect(page).toHaveScreenshot(
        `home-${viewport.width}x${viewport.height}-${label}.png`,
        { animations: 'disabled', caret: 'hide', scale: 'css' }
      );
    });
  }
}
