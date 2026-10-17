import { test, expect } from '@playwright/test';
import { execSync } from 'child_process';
import path from 'path';
import fs from 'fs';

test.describe('BingeSync Web UI & Webhooks', () => {
  test('complete flow: sign-in, manual scrobble, search, stats, webhook URL, and deletion', async ({ page, request }) => {
    const testEmail = `pw_${Date.now()}@bingesync.local`;

    // 1. Visit the app
    await page.goto('/');

    // Check title and welcome header
    await expect(page).toHaveTitle(/BingeSync/);
    await expect(page.locator('.auth-header h2')).toContainText('Welcome to BingeSync');

    // 2. Perform dev sign-in
    await page.fill('#dev-email', testEmail);
    await page.fill('#dev-name', 'Playwright Tester');
    await page.click('#btn-dev-submit');

    // Verify user is authenticated and header is populated
    await expect(page.locator('#user-display-name')).toHaveText('Playwright Tester');
    await expect(page.locator('#view-timeline')).toBeVisible();

    // 3. Open Log Watch modal and add a movie
    await page.click('#btn-open-log-modal');
    await expect(page.locator('#modal-log-scrobble')).toBeVisible();

    await page.fill('#scrobble-title', 'Oppenheimer');
    await page.fill('#scrobble-year', '2023');
    await page.fill('#scrobble-duration', '180');
    await page.fill('#scrobble-overview', 'The story of J. Robert Oppenheimer and the Manhattan Project.');
    await page.click('#btn-submit-scrobble');

    // Modal should close and timeline should display Oppenheimer
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();
    await expect(page.locator('.timeline-container')).toContainText('Oppenheimer');
    await expect(page.locator('.timeline-container')).toContainText('2023');

    // 4. Switch to History tab and test search filter
    await page.click('#tab-history');
    await expect(page.locator('#view-history')).toBeVisible();
    await expect(page.locator('#history-list')).toContainText('Oppenheimer');

    // Test search filter
    await page.fill('#history-search', 'NonExistentMovieXYZ');
    await expect(page.locator('#history-list')).toContainText('No scrobbles match your search criteria');

    await page.fill('#history-search', 'Oppenheimer');
    await expect(page.locator('#history-list')).toContainText('Oppenheimer');

    // 5. Switch to Stats tab and check metrics
    await page.click('#tab-stats');
    await expect(page.locator('#view-stats')).toBeVisible();
    await expect(page.locator('#stat-scrobbles')).toHaveText('1');
    await expect(page.locator('#stat-movies')).toHaveText('1');
    await expect(page.locator('#stat-episodes')).toHaveText('0');
    await expect(page.locator('#stat-hours')).toHaveText('3h');

    // 6. Switch to Webhook tab
    await page.click('#tab-settings');
    await expect(page.locator('#view-settings')).toBeVisible();

    const webhookUrlInput = page.locator('#webhook-url-input');
    await expect(webhookUrlInput).toBeVisible();
    const webhookUrl = await webhookUrlInput.inputValue();
    expect(webhookUrl).toContain('/api/v1/webhooks/plex/');

    // Test Copy button
    await page.click('#btn-copy-webhook');
    await expect(page.locator('#copy-status')).toBeVisible();

    // Configure Plex username filter in Connected Services
    await page.fill('#user-setting-plex-username', 'playwright_plex');
    await page.click('#btn-save-user-settings');
    await expect(page.locator('#user-settings-status')).toContainText('Settings saved successfully!');

    // 7. Simulate Plex sending a webhook event using the user's personal webhook URL
    const plexPayload = {
      event: 'media.scrobble',
      user: true,
      owner: true,
      Account: { title: 'playwright_plex' },
      Metadata: {
        type: 'episode',
        title: 'Fly',
        grandparentTitle: 'Breaking Bad',
        parentTitle: 'Season 3',
        index: 10,
        parentIndex: 3,
        year: 2010,
        duration: 2820000,
        summary: 'Walt and Jesse deal with a contamination problem in the lab.'
      }
    };

    const webhookResponse = await request.post(webhookUrl, {
      data: plexPayload,
      headers: { 'Content-Type': 'application/json' },
    });
    expect(webhookResponse.status()).toBe(201);

    // 8. Return to Timeline and verify Plex webhook scrobble appears immediately
    await page.click('#tab-timeline');
    await page.click('#btn-refresh-timeline');
    await expect(page.locator('.timeline-container')).toContainText('Fly');
    await expect(page.locator('.timeline-container')).toContainText('Breaking Bad');
    await expect(page.locator('.timeline-container')).toContainText('S03E10');

    // 9. Inspect item in Detail Modal and delete it
    await page.locator('#timeline-container .scrobble-card', { hasText: 'Oppenheimer' }).click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();
    await expect(page.locator('#detail-title')).toHaveText('Oppenheimer');
    await expect(page.locator('#detail-body')).toContainText('Manhattan Project');

    // Test Refresh Metadata button
    await expect(page.locator('#btn-refresh-detail-metadata')).toBeVisible();
    await page.click('#btn-refresh-detail-metadata');
    await expect(page.locator('#modal-item-detail')).toBeVisible();

    // Delete item
    page.once('dialog', dialog => dialog.accept());
    await page.click('#btn-delete-detail-item');
    await expect(page.locator('#modal-item-detail')).toBeHidden();

    // Oppenheimer should now be removed from timeline
    await expect(page.locator('.timeline-container')).not.toContainText('Oppenheimer');
    // But Fly should still remain
    await expect(page.locator('.timeline-container')).toContainText('Fly');
  });

  test('multi-user separation in Web UI', async ({ page }) => {
    const userA = `usera_${Date.now()}@bingesync.local`;
    const userB = `userb_${Date.now()}@bingesync.local`;

    // 1. Sign in as User A
    await page.goto('/');
    await page.fill('#dev-email', userA);
    await page.fill('#dev-name', 'User Alpha');
    await page.click('#btn-dev-submit');

    // Add a scrobble for User A
    await page.click('#btn-open-log-modal');
    await page.fill('#scrobble-title', 'The Matrix');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('.timeline-container')).toContainText('The Matrix');

    // Get User A's webhook URL
    await page.click('#tab-settings');
    const webhookA = await page.locator('#webhook-url-input').inputValue();

    // 2. Sign out
    await page.click('#btn-logout');
    await expect(page.locator('.auth-header h2')).toBeVisible();

    // 3. Sign in as User B
    await page.fill('#dev-email', userB);
    await page.fill('#dev-name', 'User Beta');
    await page.click('#btn-dev-submit');

    // Verify User B's timeline is completely empty
    await expect(page.locator('.timeline-container')).toContainText('No watch history yet');
    await expect(page.locator('.timeline-container')).not.toContainText('The Matrix');

    // Verify User B has a different webhook URL
    await page.click('#tab-settings');
    const webhookB = await page.locator('#webhook-url-input').inputValue();
    expect(webhookB).not.toEqual(webhookA);
  });

  test('Trakt history and rating import flow in Web UI', async ({ page }) => {
    const user = `trakt_ui_${Date.now()}@bingesync.local`;

    await page.goto('/');
    await page.fill('#dev-email', user);
    await page.fill('#dev-name', 'Trakt Importer');
    await page.click('#btn-dev-submit');

    // 1. Open Import Trakt modal
    await page.click('#btn-open-import-modal');
    await expect(page.locator('#modal-import-history')).toBeVisible();

    // 2. Paste Trakt JSON sample with exact past timestamp and 10/10 rating
    const traktData = JSON.stringify([
      {
        "watched_at": "2021-12-25T20:15:00.000Z",
        "type": "movie",
        "movie": {
          "title": "Interstellar (Trakt Import)",
          "year": 2014
        },
        "rating": 10
      }
    ]);

    await page.fill('#import-paste-input', traktData);
    await page.click('#btn-submit-import');

    // 3. Verify status message
    await expect(page.locator('#import-status-box')).toContainText('Import Complete');
    await expect(page.locator('#import-status-box')).toContainText('1 scrobbles');

    // Close modal
    await page.click('#btn-cancel-import-modal');
    await expect(page.locator('#modal-import-history')).toBeHidden();

    // 4. Verify item and star rating appear on timeline
    await expect(page.locator('.timeline-container')).toContainText('Interstellar (Trakt Import)');
    await expect(page.locator('.timeline-container')).toContainText('★ 10/10');
    // Date should be formatted Dec 25, 2021
    await expect(page.locator('.timeline-container')).toContainText('2021');

    // 5. Open detail modal and verify rating and watched timestamp
    await page.locator('#timeline-container .scrobble-card', { hasText: 'Interstellar (Trakt Import)' }).click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();
    await expect(page.locator('#detail-body')).toContainText('★ 10/10');
    await expect(page.locator('#detail-body')).toContainText('2021');
  });

  test('Trakt multi-file ZIP archive import flow in Web UI', async ({ page }) => {
    const user = `trakt_zip_${Date.now()}@bingesync.local`;

    await page.goto('/');
    await page.fill('#dev-email', user);
    await page.fill('#dev-name', 'ZIP Importer');
    await page.click('#btn-dev-submit');

    // Create a temporary zip with multi-file history and ratings
    const tempZipPath = path.resolve('test-trakt-temp.zip');
    execSync(`python3 -c "
import zipfile, json
with zipfile.ZipFile('${tempZipPath}', 'w') as z:
    z.writestr('ratings-movies-1.json', json.dumps([{'rated_at':'2024-01-01T00:00:00Z','rating':9,'type':'movie','movie':{'title':'Dune: Part Two','year':2024,'ids':{'imdb':'tt15239678'}}}]))
    z.writestr('watched-history-1.json', json.dumps([{'id':1,'watched_at':'2024-03-01T20:00:00Z','type':'movie','movie':{'title':'Dune: Part Two','year':2024,'ids':{'imdb':'tt15239678'}}}]))
    z.writestr('watched-history-2.json', json.dumps([{'id':2,'watched_at':'2023-01-01T15:00:00Z','type':'movie','movie':{'title':'Avatar 2','year':2022}}]))
"`);

    try {
      // 1. Open Import Trakt modal
      await page.click('#btn-open-import-modal');
      await expect(page.locator('#modal-import-history')).toBeVisible();

      // 2. Upload zip file
      await page.setInputFiles('#import-file-input', tempZipPath);
      await page.click('#btn-submit-import');

      // 3. Verify status message
      await expect(page.locator('#import-status-box')).toContainText('Import Complete');
      await expect(page.locator('#import-status-box')).toContainText('2 scrobbles');

      // Close modal
      await page.click('#btn-cancel-import-modal');
      await expect(page.locator('#modal-import-history')).toBeHidden();

      // 4. Verify latest month (March 2024) displays Dune: Part Two with rating
      await expect(page.locator('.timeline-container')).toContainText('Dune: Part Two');
      await expect(page.locator('.timeline-container')).toContainText('★ 9/10');

      // 5. Test drill-down: Drill up to Year view, then All Years, then down into 2023 January for Avatar 2
      await page.click('#btn-timeline-up');
      await expect(page.locator('.timeline-container')).toContainText('Months Overview');

      await page.click('#btn-timeline-up');
      await expect(page.locator('.timeline-container')).toContainText('Watch History by Year');

      await page.locator('.timeline-year-card', { hasText: '2023' }).click();
      await expect(page.locator('.timeline-container')).toContainText('2023 — Months Overview');

      await page.locator('.timeline-month-card', { hasText: 'January' }).click();
      await expect(page.locator('.timeline-container')).toContainText('Avatar 2');
    } finally {
      if (fs.existsSync(tempZipPath)) {
        fs.unlinkSync(tempZipPath);
      }
    }
  });

  test('timeline search, filtering, and delete by date', async ({ page }) => {
    const user = `tl_filter_${Date.now()}@bingesync.local`;

    await page.goto('/');
    await page.fill('#dev-email', user);
    await page.fill('#dev-name', 'Timeline Tester');
    await page.click('#btn-dev-submit');

    // 1. Log two items on different dates
    await page.click('#btn-open-log-modal');
    await page.fill('#scrobble-title', 'Gladiator');
    await page.selectOption('#scrobble-rating', '10');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('.timeline-container')).toContainText('Gladiator');

    // 2. Test timeline search
    await page.fill('#timeline-search', 'NonExistentTitle');
    await expect(page.locator('.timeline-container')).toContainText('No watch history matches');

    await page.fill('#timeline-search', 'Gladiator');
    await expect(page.locator('.timeline-container')).toContainText('Gladiator');

    // Clear search
    await page.click('#btn-reset-timeline-filters');
    await expect(page.locator('.timeline-container')).toContainText('Gladiator');

    // 3. Test Delete Day button
    const deleteDayBtn = page.locator('.day-header-delete-btn').first();
    await expect(deleteDayBtn).toBeVisible();

    page.once('dialog', dialog => dialog.accept());
    await deleteDayBtn.click();

    // After deleting day, timeline should be empty
    await expect(page.locator('.timeline-container')).toContainText('No watch history yet');
  });

  test('show detail modal IMDb link and delete all scrobbles for show', async ({ page, request }) => {
    const user = `show_del_${Date.now()}@bingesync.local`;

    await page.goto('/');
    await page.fill('#dev-email', user);
    await page.fill('#dev-name', 'Show Tester');
    await page.click('#btn-dev-submit');

    // Get webhook URL
    await page.click('#tab-settings');
    const webhookUrl = await page.locator('#webhook-url-input').inputValue();

    // Scrobble 2 episodes of The Sopranos via webhook
    for (let ep = 1; ep <= 2; ep++) {
      await request.post(webhookUrl, {
        data: {
          event: 'media.scrobble',
          Account: { title: 'tester' },
          Metadata: {
            type: 'episode',
            title: `Episode ${ep}`,
            grandparentTitle: 'The Sopranos',
            parentTitle: 'Season 1',
            index: ep,
            parentIndex: 1,
            guid: 'imdb://tt0141842',
            duration: 3000000
          }
        },
        headers: { 'Content-Type': 'application/json' },
      });
    }

    // Return to Timeline
    await page.click('#tab-timeline');
    await page.click('#btn-refresh-timeline');
    await expect(page.locator('.timeline-container')).toContainText('The Sopranos');

    // Open detail modal for Episode 1
    await page.locator('#timeline-container .scrobble-card', { hasText: 'Episode 1' }).click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();

    // Check IMDb link
    const imdbLink = page.locator('#detail-imdb-link');
    await expect(imdbLink).toBeVisible();
    await expect(imdbLink).toHaveAttribute('href', /imdb\.com\/title\/tt0141842/);

    // Check TVDB button
    const tvdbLink = page.locator('#detail-tvdb-link');
    await expect(tvdbLink).toBeVisible();
    await expect(tvdbLink).toHaveAttribute('href', /thetvdb\.com/);
    await expect(tvdbLink).toContainText('View on TheTVDB');

    // Check Delete Show button
    const deleteShowBtn = page.locator('#btn-delete-show-scrobbles');
    await expect(deleteShowBtn).toBeVisible();
    await expect(deleteShowBtn).toContainText('The Sopranos');

    // Click delete all for show
    page.once('dialog', dialog => dialog.accept());
    await deleteShowBtn.click();
    await expect(page.locator('#modal-item-detail')).toBeHidden();

    // Verify all Sopranos episodes were deleted
    await expect(page.locator('.timeline-container')).not.toContainText('The Sopranos');
  });

  test('multi-select batch delete in History view', async ({ page }) => {
    const user = `batch_${Date.now()}@bingesync.local`;

    await page.goto('/');
    await page.fill('#dev-email', user);
    await page.fill('#dev-name', 'Batch Tester');
    await page.click('#btn-dev-submit');

    // Add 2 manual scrobbles
    for (const title of ['Movie Alpha', 'Movie Beta']) {
      await page.click('#btn-open-log-modal');
      await page.fill('#scrobble-title', title);
      await page.click('#btn-submit-scrobble');
      await expect(page.locator('.timeline-container')).toContainText(title);
    }

    // Switch to History view
    await page.click('#tab-history');
    await expect(page.locator('#history-list')).toContainText('Movie Alpha');
    await expect(page.locator('#history-list')).toContainText('Movie Beta');

    // Verify TVDB links exist on items in history
    await expect(page.locator('#history-list .card-tvdb-link').first()).toBeVisible();
    await expect(page.locator('#history-list .card-tvdb-link').first()).toContainText('TVDB');

    // Enter select mode
    await page.click('#btn-toggle-select-mode');
    await expect(page.locator('#history-bulk-bar')).toBeVisible();

    // Select All
    await page.click('#bulk-select-all-checkbox');
    await expect(page.locator('#bulk-selected-count')).toContainText('2 selected');

    // Click Delete Selected
    page.once('dialog', dialog => dialog.accept());
    await page.click('#btn-delete-selected');

    // Both should now be deleted
    await expect(page.locator('#history-bulk-bar')).toBeHidden();
    await expect(page.locator('#history-list')).toContainText('No scrobbles match');
  });

  test('admin role and user management: list users and delete user', async ({ page, request }) => {
    const timestamp = Date.now();
    const adminEmail = `admin_${timestamp}@bingesync.local`;
    const standardEmail = `standard_${timestamp}@bingesync.local`;
    const standardName = `User_${timestamp}`;

    // 1. Create standard user via dev-login API
    await request.post('/api/v1/auth/dev-login', {
      data: {
        email: standardEmail,
        display_name: standardName,
        role: 'user',
      },
    });

    // 2. Sign in via UI as admin (email contains 'admin')
    await page.goto('/');
    await page.fill('#dev-email', adminEmail);
    await page.fill('#dev-name', 'System Admin');
    await page.click('#btn-dev-submit');

    // Admin tab should be visible
    await expect(page.locator('#tab-admin')).toBeVisible();

    // 3. Switch to Users tab
    await page.click('#tab-admin');
    await expect(page.locator('#view-admin')).toBeVisible();

    // Verify OIDC Backchannel Logout URL field is visible and populated
    await expect(page.locator('#admin-backchannel-url-input')).toBeVisible();
    await expect(page.locator('#admin-backchannel-url-input')).toHaveValue(/api\/v1\/auth\/backchannel-logout/);

    // Verify both users are displayed
    await expect(page.locator('#admin-users-tbody')).toContainText('System Admin');
    await expect(page.locator('#admin-users-tbody')).toContainText(standardName);
    await expect(page.locator('#admin-users-tbody')).toContainText(/admin/i);
    await expect(page.locator('#admin-users-tbody')).toContainText(/user/i);

    // 4. Delete the standard user
    page.once('dialog', dialog => dialog.accept());
    const deleteBtn = page.locator(`.btn-delete-user[data-email="${standardEmail}"]`);
    await expect(deleteBtn).toBeVisible();
    await deleteBtn.click();

    // 5. Verify the standard user is removed from the table
    await expect(page.locator('#admin-users-tbody')).not.toContainText(standardEmail);
    await expect(page.locator('#admin-users-tbody')).not.toContainText(standardName);
  });

  test('sonarr cleanup options: verify delete series, season, and all files in detail modal', async ({ page }) => {
    const userEmail = `sonarr_ui_${Date.now()}@bingesync.local`;

    // Intercept Sonarr library check to simulate series found in Sonarr
    await page.route('**/api/v1/integrations/check*', async route => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          found: true,
          service: 'sonarr',
          series_id: 88,
          title: 'Better Call Saul',
        }),
      });
    });

    let lastDeletePayload = null;
    await page.route('**/api/v1/integrations/sonarr/delete-series', async route => {
      lastDeletePayload = route.request().postDataJSON();
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          status: 'deleted',
          service: 'sonarr',
          series_id: 88,
          action: lastDeletePayload.action,
          message: 'Successfully processed Sonarr action',
        }),
      });
    });

    // Sign in
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Saul Fan');
    await page.click('#btn-dev-submit');

    // Add an episode scrobble (Better Call Saul, Season 2, Episode 3)
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'episode');
    await page.fill('#scrobble-show', 'Better Call Saul');
    await page.fill('#scrobble-title', 'Amarillo');
    await page.fill('#scrobble-season', '2');
    await page.fill('#scrobble-episode', '3');
    await page.click('#btn-submit-scrobble');

    // Open detail modal from timeline
    await page.locator('#timeline-container .scrobble-card', { hasText: 'Amarillo' }).click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();

    // Verify Sonarr card is visible with all 3 options
    await expect(page.locator('.arr-cleanup-card-sonarr')).toBeVisible();
    const actionSelect = page.locator('#sonarr-action-select');
    await expect(actionSelect).toBeVisible();

    // Verify the 3 options exist
    const optionsText = await actionSelect.locator('option').allTextContents();
    expect(optionsText).toContain('Delete show and files');
    expect(optionsText.some(t => t.includes('Delete and unmonitor season 2 files'))).toBe(true);
    expect(optionsText).toContain('Delete all files and unmonitor all seasons');

    // Test executing "delete_season"
    await actionSelect.selectOption('delete_season');
    page.once('dialog', dialog => dialog.accept());
    await page.click('#btn-arr-cleanup');

    // Verify success message appears and payload is correct
    await expect(page.locator('#detail-arr-container')).toContainText('Successfully processed Sonarr action');
    expect(lastDeletePayload).toEqual({
      series_id: 88,
      action: 'delete_season',
      season_number: 2,
      delete_files: true,
    });
  });

  test('tvdb attribution: verify attribution in footer, settings, and scrobble detail modal', async ({ page }) => {
    const userEmail = `tvdb_ui_${Date.now()}@bingesync.local`;

    await page.goto('/');

    // 1. Verify global footer attribution is present on the page
    const footer = page.locator('.app-footer');
    await expect(footer).toBeVisible();
    await expect(footer).toContainText('TheTVDB');
    await expect(footer).toContainText('This product uses TheTVDB API but is not endorsed or certified by TheTVDB');
    const footerLink = footer.locator('a.tvdb-link');
    await expect(footerLink).toHaveAttribute('href', 'https://thetvdb.com');

    // 2. Sign in
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'TVDB User');
    await page.click('#btn-dev-submit');

    // 3. Navigate to Settings and verify TheTVDB card
    await page.click('#tab-settings');
    await expect(page.locator('#settings-tvdb-card')).toBeVisible();
    await expect(page.locator('#settings-tvdb-card')).toContainText('TheTVDB Metadata Integration');
    await expect(page.locator('#settings-tvdb-card')).toContainText('Media metadata, plot outlines, genres, and cast details are provided by');

    // 4. Log a scrobble and verify detail modal attribution
    await page.click('#btn-open-log-modal');
    await page.fill('#scrobble-title', 'Chernobyl');
    await page.click('#btn-submit-scrobble');

    await page.click('#tab-timeline');
    await page.locator('#timeline-container .scrobble-card', { hasText: 'Chernobyl' }).click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();

    const detailAttribution = page.locator('.detail-tvdb-attribution');
    await expect(detailAttribution).toBeVisible();
    await expect(detailAttribution).toContainText('TheTVDB');
    await expect(detailAttribution.locator('a')).toHaveAttribute('href', 'https://thetvdb.com');
  });

  test('item other watches: verify other watches table lists previous watches of exact same episode', async ({ page }) => {
    const userEmail = `watches_e2e_${Date.now()}@bingesync.local`;

    await page.goto('/');

    // 1. Sign in
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Watch Tester');
    await page.click('#btn-dev-submit');

    // 2. Log first watch of Severance S01E01
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'episode');
    await page.fill('#scrobble-show', 'Severance');
    await page.fill('#scrobble-title', 'Good News About Hell');
    await page.fill('#scrobble-season', '1');
    await page.fill('#scrobble-episode', '1');
    await page.selectOption('#scrobble-rating', '9');
    await page.click('#btn-submit-scrobble');

    // Wait for scrobble card to appear
    await expect(page.locator('#timeline-container .scrobble-card', { hasText: 'Good News About Hell' })).toBeVisible();

    // 3. Log second watch of Severance S01E01 (same episode!)
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'episode');
    await page.fill('#scrobble-show', 'Severance');
    await page.fill('#scrobble-title', 'Good News About Hell');
    await page.fill('#scrobble-season', '1');
    await page.fill('#scrobble-episode', '1');
    await page.selectOption('#scrobble-rating', '10');
    await page.click('#btn-submit-scrobble');

    // 4. Log watch of Severance S01E02 (different episode!)
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'episode');
    await page.fill('#scrobble-show', 'Severance');
    await page.fill('#scrobble-title', 'Half Loop');
    await page.fill('#scrobble-season', '1');
    await page.fill('#scrobble-episode', '2');
    await page.selectOption('#scrobble-rating', '8');
    await page.click('#btn-submit-scrobble');

    // Wait for S01E02 card to appear
    await expect(page.locator('#timeline-container .scrobble-card', { hasText: 'Half Loop' })).toBeVisible();

    // 5. Navigate to History view
    await page.click('#tab-history');
    await expect(page.locator('#history-list')).toBeVisible();

    // Click on the S01E01 scrobble card in history
    await page.locator('#history-list .scrobble-card', { hasText: 'Good News About Hell' }).first().click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();

    // 6. Verify Other Watches section for S01E01
    const watchesSection = page.locator('#detail-watches-container');
    await expect(watchesSection).toBeVisible();
    await expect(watchesSection).toContainText('Other Watches');
    await expect(watchesSection).toContainText('1 other (2 total)');
    await expect(watchesSection.locator('.watches-table')).toBeVisible();
    await expect(watchesSection.locator('.other-watch-row')).toHaveCount(1);

    // Close detail modal
    await page.click('#btn-dismiss-detail-modal');
    await expect(page.locator('#modal-item-detail')).toBeHidden();

    // 7. Click on S01E02 scrobble card in history
    await page.locator('#history-list .scrobble-card', { hasText: 'Half Loop' }).first().click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();

    // Verify S01E02 shows 0 other watches (1 total) and "Only recorded watch for this episode"
    await expect(watchesSection).toContainText('0 others (1 total)');
    await expect(watchesSection).toContainText('Only recorded watch for this episode');
  });

  test('stats view displays top watched movies and updates with filters', async ({ page }) => {
    const userEmail = `pw_topmovies_${Date.now()}@bingesync.local`;

    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Movie Fan');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toHaveText('Movie Fan');

    // Log 2 watches of Dune and 1 watch of Inception
    for (let i = 0; i < 2; i++) {
      await page.click('#btn-open-log-modal');
      await page.selectOption('#scrobble-media-type', 'movie');
      await page.fill('#scrobble-title', 'Dune: Part Two');
      await page.fill('#scrobble-year', '2024');
      await page.fill('#scrobble-duration', '166');
      await page.click('#btn-submit-scrobble');
      await expect(page.locator('#modal-log-scrobble')).toBeHidden();
    }

    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'movie');
    await page.fill('#scrobble-title', 'Inception');
    await page.fill('#scrobble-year', '2010');
    await page.fill('#scrobble-duration', '148');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // Switch to Stats view
    await page.click('#tab-stats');
    await expect(page.locator('#view-stats')).toBeVisible();

    // Verify Top Watched Movies section exists and lists Dune and Inception
    const topMoviesSection = page.locator('#top-movies-section');
    await expect(topMoviesSection).toBeVisible();
    await expect(topMoviesSection).toContainText('Top Watched Movies');
    await expect(page.locator('#top-movies-list')).toContainText('Dune: Part Two (2024)');
    await expect(page.locator('#top-movies-list')).toContainText('2 plays');
    await expect(page.locator('#top-movies-list')).toContainText('Inception (2010)');
    await expect(page.locator('#top-movies-list')).toContainText('1 play');

    // Test filter pills: 'TV Shows' hides top movies, 'Movies' shows top movies
    await page.click('#pill-stats-episodes');
    await expect(topMoviesSection).toBeHidden();

    await page.click('#pill-stats-movies');
    await expect(topMoviesSection).toBeVisible();
  });

  test('timeline minimised search and date scrubber interaction', async ({ page }) => {
    const userEmail = `pw_scrubber_${Date.now()}@bingesync.local`;

    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Scrubber Tester');
    await page.click('#btn-dev-submit');

    // Log a movie
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'movie');
    await page.fill('#scrobble-title', 'Interstellar');
    await page.fill('#scrobble-year', '2014');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // Scrubber should be visible and contain the current day button
    const scrubber = page.locator('#timeline-scrubber');
    await expect(scrubber).toBeVisible();
    await expect(page.locator('#timeline-scrubber-list .scrubber-item').first()).toBeVisible();

    // Verify filters grid is hidden initially until search is clicked
    const filtersGrid = page.locator('#timeline-filters-grid');
    await expect(filtersGrid).toBeHidden();

    // Verify search expandable element exists and has search-expandable class
    const searchWrap = page.locator('#timeline-search-wrap');
    await expect(searchWrap).toHaveClass(/search-expandable/);

    // Clicking search opens filters grid
    await page.click('#timeline-search-wrap');
    await expect(filtersGrid).toBeVisible();

    // Search for Interstellar
    await page.fill('#timeline-search', 'Interstellar');
    await expect(page.locator('#timeline-container')).toContainText('Interstellar');

    // Clearing filters hides filter grid again
    await page.click('#btn-reset-timeline-filters');
    await expect(filtersGrid).toBeHidden();
  });

  test('admin settings, background tasks, and application logs management', async ({ page }) => {
    const adminEmail = `admin_suite_${Date.now()}@bingesync.local`;

    // Sign in via UI as admin
    await page.goto('/');
    await page.fill('#dev-email', adminEmail);
    await page.fill('#dev-name', 'System Admin');
    await page.click('#btn-dev-submit');

    // Verify admin tab is visible and navigate to Admin view
    await expect(page.locator('#tab-admin')).toBeVisible();
    await page.click('#tab-admin');
    await expect(page.locator('#view-admin')).toBeVisible();

    // 1. Test Subview: Services & API Keys
    await page.click('#admin-subnav-pills button[data-subview="admin-sub-settings"]');
    await expect(page.locator('#admin-sub-settings')).toBeVisible();
    await expect(page.locator('#admin-sub-users')).toBeHidden();
    // Allow any initial background fetch of settings to settle
    await page.waitForTimeout(100);

    // Fill settings form and save
    await page.fill('#setting-tvdb-api-key', 'tvdb_test_api_key_456');
    await page.fill('#setting-tvdb-pin', '1234');
    await page.click('#btn-save-admin-settings');

    // Verify success feedback
    await expect(page.locator('#admin-settings-status')).toContainText('Settings saved successfully!');

    // Verify API key redaction: starts with 'tv' and contains bullet '•'
    await expect(page.locator('#setting-tvdb-api-key')).toHaveValue(/^tv.*•/);
    const maskedVal = await page.locator('#setting-tvdb-api-key').inputValue();
    expect(maskedVal.startsWith('tv')).toBeTruthy();
    expect(maskedVal.includes('•')).toBeTruthy();

    // Toggle reveal button to view full key
    const toggleBtn = page.locator('#setting-tvdb-api-key + .btn-toggle-secret');
    await toggleBtn.click();
    await expect(page.locator('#setting-tvdb-api-key')).toHaveValue('tvdb_test_api_key_456');

    // Toggle again to re-mask
    await toggleBtn.click();
    await expect(page.locator('#setting-tvdb-api-key')).toHaveValue(/^tv.*•/);
    const remaskedVal = await page.locator('#setting-tvdb-api-key').inputValue();
    expect(remaskedVal.startsWith('tv')).toBeTruthy();
    expect(remaskedVal.includes('•')).toBeTruthy();

    // 2. Test Subview: Background Tasks
    await page.click('#admin-subnav-pills button[data-subview="admin-sub-tasks"]');
    await expect(page.locator('#admin-sub-tasks')).toBeVisible();

    // Verify task cards exist (poster_cache and tvdb_metadata)
    const taskCards = page.locator('.task-card');
    await expect(taskCards).toHaveCount(2);
    await expect(page.locator('.task-card', { hasText: 'Media Poster & Thumbnail Caching' })).toBeVisible();
    await expect(page.locator('.task-card', { hasText: 'TheTVDB v4 Metadata Enrichment' })).toBeVisible();

    // Trigger run now on a task
    const runBtn = page.locator('.task-card', { hasText: 'Media Poster & Thumbnail Caching' }).locator('.btn-trigger-task');
    await runBtn.click();
    await expect(runBtn).toBeVisible();

    // 3. Test Subview: Application Logs
    await page.click('#admin-subnav-pills button[data-subview="admin-sub-logs"]');
    await expect(page.locator('#admin-sub-logs')).toBeVisible();

    // Verify logs terminal has entries
    const logsViewer = page.locator('#admin-logs-viewer');
    await expect(logsViewer).toBeVisible();
    await expect(page.locator('#admin-logs-viewer .log-entry').first()).toBeVisible();

    // Test log level filter
    await page.selectOption('#admin-logs-filter-level', 'WARN');
    await page.click('#btn-refresh-admin-logs');
    await expect(logsViewer).toBeVisible();
  });

  test('favicon: verify favicon is linked and accessible', async ({ page }) => {
    await page.goto('/');
    const favicon = page.locator('link[rel="icon"]');
    await expect(favicon).toHaveAttribute('href', './favicon.svg');

    const response = await page.request.get('/favicon.svg');
    expect(response.status()).toBe(200);
    expect(response.headers()['content-type']).toContain('image/svg+xml');
  });

  test('scrobble in-progress: verify in-progress badge and watch status', async ({ page }) => {
    const userEmail = `progress_ui_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Progress UI User');
    await page.click('#btn-dev-submit');

    // Fetch user webhook token from settings
    await page.click('#tab-settings');
    const webhookUrl = await page.locator('#webhook-url-input').inputValue();
    const token = new URL(webhookUrl).pathname.split('/').pop();

    // Send a media.play event with 45% progress
    const res = await page.request.post(`/api/v1/webhooks/plex/${token}`, {
      data: {
        event: 'media.play',
        Metadata: {
          title: 'Dune Part Two',
          type: 'movie',
          year: 2024,
          duration: 10000,
          viewOffset: 4500,
        }
      }
    });
    expect(res.status()).toBe(200);

    // Navigate to timeline and verify in-progress badge and progress bar
    await page.click('#tab-timeline');
    const card = page.locator('#timeline-container .scrobble-card', { hasText: 'Dune Part Two' });
    await expect(card).toBeVisible();
    await expect(card.locator('.badge-in-progress')).toContainText('45%');
    await expect(card.locator('.scrobble-progress-bar')).toBeVisible();

    // Click card to open detail modal and verify status display
    await card.click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();
    await expect(page.locator('#detail-body')).toContainText('In Progress (45%)');
  });

  test('tautulli import: verify import modal, username defaulting, and deduplication', async ({ page }) => {
    const userEmail = `tautulli_ui_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Tautulli Streamer');
    await page.click('#btn-dev-submit');

    // 1. Open unified Import modal and switch to Tautulli tab
    await expect(page.locator('#btn-open-import-modal')).toBeVisible();
    await page.click('#btn-open-import-modal');
    await expect(page.locator('#modal-import-history')).toBeVisible();
    await page.click('#btn-import-tab-tautulli');
    await expect(page.locator('#import-pane-tautulli')).toBeVisible();

    // 2. Verify API URL and API Key are NOT in the import modal (only in Settings)
    await expect(page.locator('#tautulli-import-url')).toHaveCount(0);
    await expect(page.locator('#tautulli-import-api-key')).toHaveCount(0);
    await expect(page.locator('#link-goto-user-settings')).toBeVisible();

    // Verify username defaults to current user username
    const usernameVal = await page.locator('#tautulli-import-username').inputValue();
    expect(usernameVal.length).toBeGreaterThan(0);

    // 3. Paste Tautulli JSON history for this user
    const testMovieTitle = `Tautulli Film ${Date.now()}`;
    const sampleHistory = {
      response: {
        result: 'success',
        data: {
          data: [
            {
              user: usernameVal,
              media_type: 'movie',
              title: testMovieTitle,
              year: 2023,
              date: 1690000000,
              duration: 10800,
              rating_key: `rk_${Date.now()}`,
              percent_complete: 100,
            }
          ]
        }
      }
    };

    await page.fill('#tautulli-paste-input', JSON.stringify(sampleHistory));
    await page.click('#btn-submit-tautulli-import');

    // 4. Verify import status success feedback
    const statusBox = page.locator('#tautulli-import-status');
    await expect(statusBox).toBeVisible();
    await expect(statusBox).toContainText('Import Complete!');
    await expect(statusBox).toContainText('Imported: 1');

    // Close modal
    await page.click('#btn-cancel-tautulli-modal');
    await expect(page.locator('#modal-import-history')).toBeHidden();

    // 5. Verify item appeared on timeline
    await page.click('#tab-timeline');
    await expect(page.locator('#timeline-container .scrobble-card', { hasText: testMovieTitle })).toBeVisible();

    // 6. Test deduplication: re-open modal, submit same data again
    await page.click('#btn-open-import-modal');
    await page.click('#btn-import-tab-tautulli');
    await page.fill('#tautulli-paste-input', JSON.stringify(sampleHistory));
    await page.click('#btn-submit-tautulli-import');

    await expect(statusBox).toBeVisible();
    await expect(statusBox).toContainText('Import Complete!');
    await expect(statusBox).toContainText('Imported: 0');
    await expect(statusBox).toContainText('Skipped duplicates: 1');
  });

  test('timeline hierarchical drill-down navigation: Year -> Month -> Dates', async ({ page }) => {
    const user = `tl_hierarchy_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', user);
    await page.fill('#dev-name', 'Hierarchy Explorer');
    await page.click('#btn-dev-submit');

    // 1. Log a watch in current month
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'movie');
    await page.fill('#scrobble-title', 'Arrival');
    await page.fill('#scrobble-year', '2016');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // Verify Arrival appears in current month dates
    await expect(page.locator('.timeline-container')).toContainText('Arrival');

    // Verify breadcrumbs and drill-up button
    await expect(page.locator('#hierarchy-breadcrumbs')).toContainText('All Years');
    await expect(page.locator('#btn-timeline-up')).toBeVisible();

    // 2. Drill up to Months view for the current year
    await page.click('#btn-timeline-up');
    await expect(page.locator('.timeline-container')).toContainText('Months Overview');
    await expect(page.locator('.timeline-month-card', { hasText: 'October' })).toBeVisible();

    // 3. Drill up to Years view
    await page.click('#btn-timeline-up');
    await expect(page.locator('.timeline-container')).toContainText('Watch History by Year');
    await expect(page.locator('.timeline-year-card', { hasText: '2026' })).toBeVisible();

    // 4. Drill back down: click 2026 year card
    await page.locator('.timeline-year-card', { hasText: '2026' }).click();
    await expect(page.locator('.timeline-container')).toContainText('2026 — Months Overview');

    // Click October month card to view dates
    await page.locator('.timeline-month-card', { hasText: 'October' }).click();
    await expect(page.locator('.timeline-container')).toContainText('Arrival');
  });

  test('rating watched items: rate from item detail modal, update, and clear rating', async ({ page }) => {
    const userEmail = `rater_ui_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Film Critic');
    await page.click('#btn-dev-submit');

    // 1. Log a movie
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'movie');
    await page.fill('#scrobble-title', 'Blade Runner 2049');
    await page.fill('#scrobble-year', '2017');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // 2. Open detail modal
    const card = page.locator('#timeline-container .scrobble-card', { hasText: 'Blade Runner 2049' });
    await expect(card).toBeVisible();
    await card.click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();

    // 3. Select rating 9 and click Save Rating
    await page.selectOption('#detail-rating-select', '9');
    await page.click('#btn-save-detail-rating');

    // 4. Verify feedback and rating display in modal
    await expect(page.locator('#detail-rating-feedback')).toContainText('Rating saved');
    await expect(page.locator('#detail-rating-display')).toContainText('★ 9/10');

    // 5. Close modal and verify rating badge appears on card
    await page.click('#btn-dismiss-detail-modal');
    await expect(page.locator('#modal-item-detail')).toBeHidden();
    await expect(card.locator('.scrobble-rating')).toContainText('★ 9/10');

    // 6. Re-open modal, clear rating to 'No rating' and save
    await card.click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();
    await page.selectOption('#detail-rating-select', '');
    await page.click('#btn-save-detail-rating');

    await expect(page.locator('#detail-rating-feedback')).toContainText('Rating cleared');
    await expect(page.locator('#detail-rating-display')).toContainText('Unrated');

    await page.click('#btn-dismiss-detail-modal');
    await expect(page.locator('#modal-item-detail')).toBeHidden();
    await expect(card.locator('.scrobble-rating')).toBeHidden();
  });

  test('swagger ui & api documentation: verify /docs and openapi spec link', async ({ page, request }) => {
    // 1. Direct OpenAPI spec request
    const specRes = await request.get('/api/v1/openapi.json');
    expect(specRes.status()).toBe(200);
    const spec = await specRes.json();
    expect(spec.openapi).toBe('3.0.3');
    expect(spec.info.title).toBe('BingeSync API');
    expect(spec.paths['/api/v1/history/{id}/rate']).toBeDefined();

    // 2. Navigate to /docs
    await page.goto('/docs');
    await expect(page).toHaveTitle(/BingeSync API - Swagger UI/);
    await expect(page.locator('.bingesync-brand-title')).toContainText('BingeSync');
    await expect(page.locator('.bingesync-brand-title')).toContainText('API Docs');
    await expect(page.locator('#swagger-ui')).toBeVisible();
    await expect(page.locator('a[href*="openapi.json"]').first()).toBeVisible();
  });

  test('user settings: configure personal radarr, sonarr, and tautulli with secret redaction', async ({ page }) => {
    const userEmail = `user_settings_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Settings User');
    await page.click('#btn-dev-submit');

    // Navigate to Settings
    const settingsLoaded = page.waitForResponse(resp => resp.url().includes('/api/v1/user/settings') && resp.status() === 200);
    await page.click('#tab-settings');
    await settingsLoaded;
    await expect(page.locator('#view-settings')).toBeVisible();
    await expect(page.locator('#card-user-services')).toBeVisible();

    // Fill in personal service settings
    await page.fill('#user-setting-plex-username', 'daryl_plex, family');
    await page.fill('#user-setting-radarr-url', 'http://10.0.0.100:7878');
    await page.fill('#user-setting-radarr-api-key', 'my_radarr_key_abc');
    await page.fill('#user-setting-sonarr-url', 'http://10.0.0.100:8989');
    await page.fill('#user-setting-sonarr-api-key', 'my_sonarr_key_xyz');
    await page.fill('#user-setting-tautulli-url', 'http://10.0.0.100:8181');
    await page.fill('#user-setting-tautulli-api-key', 'my_tautulli_key_123');

    await page.click('#btn-save-user-settings');
    await expect(page.locator('#user-settings-status')).toContainText('Settings saved successfully!');

    // Verify values persisted
    await expect(page.locator('#user-setting-plex-username')).toHaveValue('daryl_plex, family');
    await expect(page.locator('#user-setting-radarr-api-key')).toHaveValue(/^my.*•/);
    await expect(page.locator('#user-setting-tautulli-api-key')).toHaveValue(/^my.*•/);

    // Toggle reveal for Tautulli key
    const toggleTautulli = page.locator('#user-setting-tautulli-api-key + .btn-toggle-secret');
    await toggleTautulli.click();
    await expect(page.locator('#user-setting-tautulli-api-key')).toHaveValue('my_tautulli_key_123');
    await toggleTautulli.click();
    await expect(page.locator('#user-setting-tautulli-api-key')).toHaveValue(/^my.*•/);
  });
});




