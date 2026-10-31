import { test, expect } from '@playwright/test';
import path from 'path';

test('capture app screenshots for documentation', async ({ page }) => {
  test.setTimeout(60000);
  await page.setViewportSize({ width: 1280, height: 800 });

  const userEmail = 'admin_screenshots@bingesync.local';
  await page.goto('/');

  // 1. Dev login
  await page.fill('#dev-email', userEmail);
  await page.fill('#dev-name', 'Admin');
  await page.click('#btn-dev-submit');
  await expect(page.locator('#user-display-name')).toContainText('Admin');

  // 2. Populate realistic scrobbles via Trakt import
  const sampleData = [
    {
      title: 'Inception',
      media_type: 'movie',
      year: 2010,
      watched_at: new Date(Date.now() - 3600000 * 3).toISOString(),
      user_rating: 10,
      genres: 'Action, Sci-Fi, Thriller',
      directors: 'Christopher Nolan',
      actors: 'Leonardo DiCaprio, Joseph Gordon-Levitt, Elliot Page',
      watch_status: 'completed'
    },
    {
      title: 'Dune: Part Two',
      media_type: 'movie',
      year: 2024,
      watched_at: new Date(Date.now() - 3600000 * 26).toISOString(),
      user_rating: 9,
      genres: 'Sci-Fi, Adventure',
      directors: 'Denis Villeneuve',
      actors: 'Timothée Chalamet, Zendaya, Rebecca Ferguson',
      watch_status: 'completed'
    },
    {
      title: 'Severance',
      media_type: 'episode',
      grandparent_title: 'Severance',
      season_number: 1,
      episode_number: 9,
      year: 2022,
      watched_at: new Date(Date.now() - 3600000 * 50).toISOString(),
      user_rating: 10,
      genres: 'Drama, Mystery, Sci-Fi',
      directors: 'Ben Stiller',
      actors: 'Adam Scott, Zach Cherry, Britt Lower',
      watch_status: 'completed'
    },
    {
      title: 'Interstellar',
      media_type: 'movie',
      year: 2014,
      watched_at: new Date(Date.now() - 3600000 * 75).toISOString(),
      user_rating: 10,
      genres: 'Adventure, Drama, Sci-Fi',
      directors: 'Christopher Nolan',
      actors: 'Matthew McConaughey, Anne Hathaway, Jessica Chastain',
      watch_status: 'completed'
    },
    {
      title: 'Breaking Bad',
      media_type: 'episode',
      grandparent_title: 'Breaking Bad',
      season_number: 5,
      episode_number: 14,
      year: 2013,
      watched_at: new Date(Date.now() - 3600000 * 98).toISOString(),
      user_rating: 10,
      genres: 'Crime, Drama, Thriller',
      directors: 'Rian Johnson',
      actors: 'Bryan Cranston, Aaron Paul, Anna Gunn',
      watch_status: 'completed'
    }
  ];

  await page.click('#btn-open-import-modal');
  await page.click('#btn-import-tab-trakt');
  await page.fill('#import-paste-input', JSON.stringify(sampleData));
  await page.click('#btn-submit-import');
  await expect(page.locator('#import-status-box')).toContainText('Import Complete!');
  await page.click('#btn-cancel-import-modal');

  // Screenshot 1: Timeline View
  await page.click('#tab-timeline');
  await page.waitForTimeout(600);
  const screenshotDir = path.resolve('../docs/screenshots');
  await page.screenshot({ path: path.join(screenshotDir, 'timeline.png') });

  // Screenshot 2: Stats & Viewing Heatmap View
  await page.click('#tab-stats');
  await page.waitForTimeout(600);
  await page.screenshot({ path: path.join(screenshotDir, 'stats.png') });

  // Screenshot 3: Settings - Connected Services (Media Server Sync & Guides)
  await page.click('#tab-settings');
  await page.click('#pill-settings-services');
  // Fill sample dummy values to show the UI in full setup state
  await page.fill('#user-setting-plex-url', 'http://192.168.1.100:32400');
  await page.fill('#user-setting-plex-api-key', 'X-Plex-Token-Sample123');
  await page.check('#user-setting-plex-sync-enabled');
  await page.fill('#user-setting-jellyfin-url', 'http://192.168.1.100:8096');
  await page.fill('#user-setting-jellyfin-api-key', 'jellyfin_api_key_sample456');
  await page.check('#user-setting-jellyfin-sync-enabled');
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(screenshotDir, 'connected_services.png') });

  // Screenshot 4: Admin Background Tasks View
  if (await page.locator('#tab-admin').isVisible()) {
    await page.click('#tab-admin');
    await page.click('#admin-subnav-pills button[data-subview="admin-sub-tasks"]');
    await page.waitForTimeout(600);
    await page.screenshot({ path: path.join(screenshotDir, 'background_tasks.png') });
  }
});

