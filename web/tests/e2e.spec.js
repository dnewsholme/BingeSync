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
    await page.click('[data-subview="settings-sub-services"]');
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

    // Sign out to clean up session
    await page.click('#btn-logout');
    await expect(page.locator('.auth-header h2')).toBeVisible();
  });

  test('Trakt history and rating import flow in Web UI', async ({ page }) => {
    const user = `trakt_ui_${Date.now()}@bingesync.local`;

    await page.goto('/');
    await page.fill('#dev-email', user);
    await page.fill('#dev-name', 'Trakt Importer');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toContainText('Trakt Importer');

    // 1. Open Import Trakt modal
    await page.click('#btn-open-import-modal');
    await expect(page.locator('#modal-import-history')).toBeVisible();

    // 2. Paste Trakt JSON sample with exact past timestamp and 10/10 rating
    const movieTitle = `Interstellar_${Date.now()}`;
    const traktData = JSON.stringify([
      {
        "watched_at": "2021-12-25T20:15:00.000Z",
        "type": "movie",
        "movie": {
          "title": movieTitle,
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
    await expect(page.locator('.timeline-container')).toContainText(movieTitle);
    await expect(page.locator('.timeline-container')).toContainText('★ 10/10');
    // Date should be formatted Dec 25, 2021
    await expect(page.locator('.timeline-container')).toContainText('2021');

    // 5. Open detail modal and verify rating and watched timestamp
    await page.locator('#timeline-container .scrobble-card', { hasText: movieTitle }).click();
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
    await expect(page.locator('#user-display-name')).toContainText('ZIP Importer');

    const duneTitle = `Dune_${Date.now()}`;
    const avatarTitle = `Avatar_${Date.now()}`;

    // Create a temporary zip with multi-file history and ratings
    const tempZipPath = path.resolve('test-trakt-temp.zip');
    execSync(`python3 -c "
import zipfile, json
with zipfile.ZipFile('${tempZipPath}', 'w') as z:
    z.writestr('ratings-movies-1.json', json.dumps([{'rated_at':'2024-01-01T00:00:00Z','rating':9,'type':'movie','movie':{'title':'${duneTitle}','year':2024,'ids':{'imdb':'tt15239678'}}}]))
    z.writestr('watched-history-1.json', json.dumps([{'id':1,'watched_at':'2024-03-01T20:00:00Z','type':'movie','movie':{'title':'${duneTitle}','year':2024,'ids':{'imdb':'tt15239678'}}}]))
    z.writestr('watched-history-2.json', json.dumps([{'id':2,'watched_at':'2023-01-01T15:00:00Z','type':'movie','movie':{'title':'${avatarTitle}','year':2022}}]))
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

      // 4. Verify latest month (March 2024) displays Dune with rating
      await expect(page.locator('.timeline-container')).toContainText(duneTitle);
      await expect(page.locator('.timeline-container')).toContainText('★ 9/10');

      // 5. Test drill-down: Drill up to Year view, then All Years, then down into 2023 January for Avatar 2
      await page.click('#btn-timeline-up');
      await expect(page.locator('.timeline-container')).toContainText('Months Overview');

      await page.click('#btn-timeline-up');
      await expect(page.locator('.timeline-container')).toContainText('Watch History by Year');

      await page.locator('.timeline-year-card', { hasText: '2023' }).click();
      await expect(page.locator('.timeline-container')).toContainText('2023 — Months Overview');

      await page.locator('.timeline-month-card', { hasText: 'January' }).click();
      await expect(page.locator('.timeline-container')).toContainText(avatarTitle);
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
    await expect(footerLink).toHaveAttribute('href', /^https:\/\/thetvdb\.com\/?$/);

    await expect(footer).toContainText('TMDB');
    await expect(footer).toContainText('This product uses the TMDB API but is not endorsed or certified by TMDB');
    const tmdbFooterLink = footer.locator('a.tmdb-link');
    await expect(tmdbFooterLink).toHaveAttribute('href', /^https:\/\/www\.themoviedb\.org\/?$/);

    // 2. Sign in
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'TVDB User');
    await page.click('#btn-dev-submit');

    // 3. Navigate to Settings and verify TheTVDB card
    await page.click('#tab-settings');
    await page.click('[data-subview="settings-sub-data"]');
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

    // Verify extended stats metrics cards & charts
    await expect(page.locator('#stat-unique-movies')).toContainText('2 unique');
    await expect(page.locator('#day-of-week-chart')).toBeVisible();
    await expect(page.locator('#time-of-day-chart')).toBeVisible();

    // Verify time range filters
    const pill7d = page.locator('#pill-stats-range-7d');
    await expect(pill7d).toBeVisible();
    await pill7d.click();
    await expect(pill7d).toHaveClass(/active/);
    await expect(page.locator('#stat-scrobbles')).toHaveText('3');
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

  test('search all history option searches across all dates and returns all results', async ({ page, request }) => {
    const userEmail = `pw_searchall_${Date.now()}@bingesync.local`;

    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Matrix Explorer');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toHaveText('Matrix Explorer');

    // 1. Log one watch of The Matrix today via UI
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'movie');
    await page.fill('#scrobble-title', 'The Matrix');
    await page.fill('#scrobble-year', '1999');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // 2. Log a second watch of The Matrix from year 2021 via API using page session
    const res = await page.request.post('/api/v1/history', {
      data: {
        title: 'The Matrix Reloaded',
        media_type: 'movie',
        year: 2003,
        watched_at: '2021-05-12T14:30:00Z'
      }
    });
    expect(res.ok()).toBeTruthy();

    // 3. In default timeline month view (current month), searching "Matrix" only shows 1 item
    await page.click('#timeline-search-wrap');
    await page.fill('#timeline-search', 'Matrix');
    await expect(page.locator('#timeline-container')).toContainText('The Matrix');
    await expect(page.locator('#timeline-container')).not.toContainText('The Matrix Reloaded');

    // 4. Toggle "All History (All Dates)" option
    const searchAllToggle = page.locator('#timeline-search-all-label');
    await expect(searchAllToggle).toBeVisible();
    await page.click('#timeline-search-all-label');
    await expect(page.locator('#timeline-search-all-dates')).toBeChecked();

    // 5. Both movies from 2021 and current year should now be displayed!
    await expect(page.locator('#timeline-container')).toContainText('The Matrix');
    await expect(page.locator('#timeline-container')).toContainText('The Matrix Reloaded');
    await expect(page.locator('#hierarchy-breadcrumbs')).toContainText('All History Search: "Matrix"');

    // 6. Test History view all results toggle
    await page.click('#tab-history');
    await expect(page.locator('#view-history')).toBeVisible();
    await page.fill('#history-search', 'Matrix');
    await expect(page.locator('#history-search-all-label')).toBeVisible();
    await page.click('#history-search-all-label');
    await expect(page.locator('#history-search-all-results')).toBeChecked();
    await expect(page.locator('#pagination-info')).toContainText('Showing all 2 results across history');
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

    // Verify task cards exist (poster_cache, tvdb_metadata, user_sync, tautulli_sync, media_sync, db_backup, and db_maintenance)
    const taskCards = page.locator('.task-card');
    await expect(taskCards).toHaveCount(7);
    const posterCard = page.locator('.task-card', { hasText: 'Media Poster & Thumbnail Caching' });
    await expect(posterCard).toBeVisible();
    await expect(page.locator('.task-card', { hasText: /Metadata Enrichment/ })).toBeVisible();
    await expect(page.locator('.task-card', { hasText: 'Plex & Media Server User Cache Sync' })).toBeVisible();
    await expect(page.locator('.task-card', { hasText: 'Tautulli Continuous 1-Way Sync' })).toBeVisible();
    await expect(page.locator('.task-card', { hasText: 'Watch History Sync (Plex & Jellyfin)' })).toBeVisible();
    const backupCard = page.locator('.task-card', { hasText: 'Database Backup' });
    await expect(backupCard).toBeVisible();
    await expect(backupCard).toContainText('Backups Stored:');
    const maintenanceCard = page.locator('.task-card', { hasText: 'Database Maintenance' });
    await expect(maintenanceCard).toBeVisible();

    // Verify View Logs button exists on each background task card
    await expect(posterCard.locator('.btn-task-logs')).toBeVisible();

    // Verify schedule selector and next run time are displayed
    await expect(posterCard.locator('.task-schedule-select')).toBeVisible();
    await expect(posterCard.locator('.task-next-run')).toBeVisible();

    // Change schedule interval to 6 hours
    await posterCard.locator('.task-schedule-select').selectOption('6');
    await expect(posterCard.locator('.schedule-save-status')).toHaveText(/Saving\.\.\.|Saved!/, { timeout: 3000 });

    // Trigger run now on a task
    const runBtn = posterCard.locator('.btn-trigger-task');
    await runBtn.click();
    await expect(runBtn).toBeVisible();

    // Click "View Logs" on poster card to navigate to logs filtered for that task
    await posterCard.locator('.btn-task-logs').click();
    await expect(page.locator('#admin-sub-logs')).toBeVisible();
    await expect(page.locator('#admin-logs-active-filter')).toBeVisible();
    await expect(page.locator('#admin-logs-active-filter-name')).toContainText('Media Poster');

    // Verify logs terminal has entries
    const logsViewer = page.locator('#admin-logs-viewer');
    await expect(logsViewer).toBeVisible();

    // Clear active task logs filter
    await page.click('#btn-clear-admin-logs-filter');
    await expect(page.locator('#admin-logs-active-filter')).toBeHidden();

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
    await expect(page.locator('#user-display-name')).toContainText('Progress UI User');

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

    // Close detail modal and sign out
    await page.click('#btn-close-detail-modal');
    await expect(page.locator('#modal-item-detail')).toBeHidden();
    await page.click('#btn-logout');
    await expect(page.locator('.auth-header h2')).toBeVisible();
  });

  test('tautulli import: verify import modal, username defaulting, and deduplication', async ({ page }) => {
    const userEmail = `tautulli_ui_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Tautulli Streamer');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toContainText('Tautulli Streamer');

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
    const watchDateSec = Math.floor(Date.now() / 1000) - 86400 * 30; // 30 days ago
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
              date: watchDateSec,
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

    // 5. Verify item appeared on timeline with source=PLEX and import tag=tautulli
    await page.click('#tab-timeline');
    const importedCard = page.locator('#timeline-container .scrobble-card', { hasText: testMovieTitle });
    await expect(importedCard).toBeVisible();
    await expect(importedCard.locator('.source-tag-card')).toContainText('PLEX');
    await expect(importedCard.locator('.import-tag-card')).toContainText('tautulli');

    // Inspect detail modal
    await importedCard.click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();
    await expect(page.locator('#detail-source-display')).toContainText('plex');
    await expect(page.locator('#detail-import-display')).toContainText('tautulli');
    await page.click('#btn-close-detail-modal');
    await expect(page.locator('#modal-item-detail')).toBeHidden();

    // 6. Test deduplication: re-open modal, submit same data again
    await page.click('#btn-open-import-modal');
    await page.click('#btn-import-tab-tautulli');
    await page.fill('#tautulli-paste-input', JSON.stringify(sampleHistory));
    await page.click('#btn-submit-tautulli-import');

    await expect(statusBox).toBeVisible();
    await expect(statusBox).toContainText('Import Complete!');
    await expect(statusBox).toContainText('Imported: 0');
    await expect(statusBox).toContainText('Skipped duplicates: 1');

    // Close modal and sign out
    await page.click('#btn-cancel-tautulli-modal');
    await expect(page.locator('#modal-import-history')).toBeHidden();
    await page.click('#btn-logout');
    await expect(page.locator('.auth-header h2')).toBeVisible();
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
    await page.click('[data-subview="settings-sub-services"]');
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

  test('jellyfin webhooks: displays Jellyfin webhook URL, supports username filtering, and processes scrobbles', async ({ page }) => {
    const userEmail = `jellyfin_ui_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Jellyfin User');
    await page.click('#btn-dev-submit');

    // 1. Navigate to Settings and verify Jellyfin Webhook URL and Guide
    await page.click('#tab-settings');
    await expect(page.locator('#view-settings')).toBeVisible();
    await expect(page.locator('#jellyfin-webhook-url-input')).toBeVisible();

    const jellyfinUrl = await page.locator('#jellyfin-webhook-url-input').inputValue();
    expect(jellyfinUrl).toContain('/api/v1/webhooks/jellyfin/');
    const token = jellyfinUrl.split('/').pop();

    // Verify copy button exists and is clickable
    await expect(page.locator('#btn-copy-jellyfin-webhook')).toBeVisible();

    // Configure Jellyfin username filter
    await page.click('[data-subview="settings-sub-services"]');
    await page.fill('#user-setting-jellyfin-username', 'jellystreamer');
    await page.click('#btn-save-user-settings');
    await expect(page.locator('#user-settings-status')).toContainText('Settings saved successfully!');
    await expect(page.locator('#user-setting-jellyfin-username')).toHaveValue('jellystreamer');

    // 2. Post a Jellyfin scrobble matching username filter
    const movieTitle = `Jellyfin Hit ${Date.now()}`;
    const scrobbleRes = await page.request.post(`/api/v1/webhooks/jellyfin/${token}`, {
      data: {
        NotificationType: 'PlaybackStop',
        ItemType: 'Movie',
        Name: movieTitle,
        Year: 2024,
        PlayedToCompletion: true,
        NotificationUsername: 'jellystreamer',
        RunTimeTicks: 72000000000,
        PlaybackPositionTicks: 72000000000,
        Provider_imdb: 'tt9999999',
      }
    });
    expect(scrobbleRes.status()).toBe(201);
    const scrobbleJson = await scrobbleRes.json();
    expect(scrobbleJson.status).toBe('scrobbled');

    // 3. Post a Jellyfin webhook with a mismatched username -> should be ignored
    const ignoreRes = await page.request.post(`/api/v1/webhooks/jellyfin/${token}`, {
      data: {
        NotificationType: 'PlaybackStop',
        ItemType: 'Movie',
        Name: 'Ignored Other User Movie',
        PlayedToCompletion: true,
        NotificationUsername: 'another_user',
        RunTimeTicks: 72000000000,
        PlaybackPositionTicks: 72000000000,
      }
    });
    expect(ignoreRes.status()).toBe(200);
    const ignoreJson = await ignoreRes.json();
    expect(ignoreJson.status).toBe('ignored');
    expect(ignoreJson.reason).toBe('username_mismatch');

    // 4. Verify movie appears in Timeline
    await page.click('#tab-timeline');
    const movieCard = page.locator('#timeline-container .scrobble-card', { hasText: movieTitle });
    await expect(movieCard).toBeVisible();
  });

  test('multilingual support: switch between English, Spanish, German, and French', async ({ page }) => {
    const userEmail = `i18n_ui_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Polyglot User');
    await page.click('#btn-dev-submit');

    await expect(page.locator('#tab-timeline')).toBeVisible();

    // Default is English
    await expect(page.locator('#tab-timeline')).toContainText('Timeline');
    await expect(page.locator('#tab-history')).toContainText('History');
    await expect(page.locator('#tab-stats')).toContainText('Stats');

    // Verify dark dropdown styling (no white background)
    const selectBg = await page.locator('#nav-language-select').evaluate(el => window.getComputedStyle(el).backgroundColor);
    expect(selectBg).not.toBe('rgb(255, 255, 255)');

    // Switch to Spanish (es)
    await page.selectOption('#nav-language-select', 'es');
    await expect(page.locator('#tab-timeline')).toContainText('Cronología');
    await expect(page.locator('#tab-history')).toContainText('Historial');
    await expect(page.locator('#tab-stats')).toContainText('Estadísticas');
    await expect(page.locator('#btn-open-log-modal')).toContainText('Registrar');
    await expect(page.locator('#view-timeline .view-title')).toContainText('Cronología de visualización');
    await expect(page.locator('#timeline-search')).toHaveAttribute('placeholder', 'Buscar en la cronología...');
    await expect(page.locator('#timeline-search-all-label')).toContainText('Todo el historial');

    // Switch to German (de)
    await page.selectOption('#nav-language-select', 'de');
    await expect(page.locator('#tab-timeline')).toContainText('Zeitachse');
    await expect(page.locator('#tab-history')).toContainText('Verlauf');
    await expect(page.locator('#tab-stats')).toContainText('Statistiken');
    await expect(page.locator('#btn-open-log-modal')).toContainText('Eintrag anlegen');
    await expect(page.locator('#view-timeline .view-title')).toContainText('Wiedergabe-Zeitachse');
    await expect(page.locator('#timeline-search')).toHaveAttribute('placeholder', 'Zeitachse durchsuchen...');

    // Switch to French (fr)
    await page.selectOption('#nav-language-select', 'fr');
    await expect(page.locator('#tab-timeline')).toContainText('Chronologie');
    await expect(page.locator('#tab-history')).toContainText('Historique');
    await expect(page.locator('#tab-stats')).toContainText('Statistiques');
    await expect(page.locator('#btn-open-log-modal')).toContainText('Enregistrer');
    await expect(page.locator('#view-timeline .view-title')).toContainText('Chronologie de visionnage');
    await expect(page.locator('#timeline-search')).toHaveAttribute('placeholder', 'Rechercher dans la chronologie...');

    // Switch back to English (en)
    await page.selectOption('#nav-language-select', 'en');
    await expect(page.locator('#tab-timeline')).toContainText('Timeline');
    await expect(page.locator('#tab-history')).toContainText('History');
    await expect(page.locator('#view-timeline .view-title')).toContainText('Watch Timeline');
  });

  test('history items: clickable actors and directors show other history items with them in', async ({ page }) => {
    const userEmail = `cast_director_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Cinema Buff');
    await page.click('#btn-dev-submit');

    await expect(page.locator('#tab-timeline')).toBeVisible();

    // 1. Log two watches featuring Christopher Nolan and Leonardo DiCaprio via page.request
    const res1 = await page.request.post('/api/v1/history', {
      data: {
        title: 'Inception',
        media_type: 'movie',
        year: 2010,
        directors: 'Christopher Nolan',
        actors: 'Leonardo DiCaprio, Joseph Gordon-Levitt, Elliot Page'
      }
    });
    expect(res1.ok()).toBeTruthy();

    const res2 = await page.request.post('/api/v1/history', {
      data: {
        title: 'Shutter Island',
        media_type: 'movie',
        year: 2010,
        directors: 'Martin Scorsese',
        actors: 'Leonardo DiCaprio, Mark Ruffalo, Ben Kingsley'
      }
    });
    expect(res2.ok()).toBeTruthy();

    // 2. Open History tab and click "Inception"
    await page.click('#tab-history');
    await expect(page.locator('#view-history')).toBeVisible();
    const inceptionCard = page.locator('#history-list .scrobble-card', { hasText: 'Inception' }).first();
    await expect(inceptionCard).toBeVisible();
    await inceptionCard.click();

    // 3. Verify modal opened and cast/directors are clickable person tags
    await expect(page.locator('#modal-item-detail')).toBeVisible();
    const leoTag = page.locator('#modal-item-detail .person-tag', { hasText: 'Leonardo DiCaprio' });
    await expect(leoTag).toBeVisible();

    const nolanTag = page.locator('#modal-item-detail .person-tag', { hasText: 'Christopher Nolan' });
    await expect(nolanTag).toBeVisible();

    // 4. Click "Leonardo DiCaprio" tag -> shows other history items with them in
    await leoTag.click();
    const personSection = page.locator('#detail-person-watches-container');
    await expect(personSection).toBeVisible();
    await expect(personSection).toContainText('Leonardo DiCaprio');
    await expect(personSection.locator('.person-history-item')).toHaveCount(2);

    // 5. Click "View in History Tab" button
    await page.click('.btn-open-history-filtered');
    await expect(page.locator('#modal-item-detail')).toBeHidden();

    // 6. Verify History view is filtered by Leonardo DiCaprio
    await expect(page.locator('#history-person-badge')).toBeVisible();
    await expect(page.locator('#history-person-badge')).toContainText('Leonardo DiCaprio');
    await expect(page.locator('#history-list .scrobble-card')).toHaveCount(2);

    // Clear filter
    await page.click('#btn-clear-history-person');
    await expect(page.locator('#history-person-badge')).toBeHidden();
  });

  test('background jobs: user cache sync displays in admin tasks and can be triggered', async ({ page }) => {
    const adminEmail = `admin_tasks_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', adminEmail);
    await page.fill('#dev-name', 'Admin Tester');
    await page.click('#btn-dev-submit');

    await expect(page.locator('#tab-admin')).toBeVisible();
    await page.click('#tab-admin');

    // Switch to Tasks subtab
    await page.click('#admin-subnav-pills button[data-subview="admin-sub-tasks"]');
    await expect(page.locator('#admin-sub-tasks')).toBeVisible();

    // Verify User Cache Sync task card is present
    const userSyncCard = page.locator('.task-card', { hasText: 'Plex & Media Server User Cache Sync' });
    await expect(userSyncCard).toBeVisible();

    // Trigger run
    const runBtn = userSyncCard.locator('button.btn-trigger-task');
    if (await runBtn.count() > 0) {
      await runBtn.click();
      await expect(userSyncCard).toBeVisible();
    }
  });

  test('history items: specific director and cinematographer fields display on card and detail modal', async ({ page }) => {
    const userEmail = `cinema_crew_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Film Aficionado');
    await page.click('#btn-dev-submit');

    await expect(page.locator('#tab-timeline')).toBeVisible();

    // 1. Open Log modal and fill title, director, cinematographer
    await page.click('#btn-open-log-modal');
    await expect(page.locator('#modal-log-scrobble')).toBeVisible();
    await page.fill('#scrobble-title', 'Dune: Part Two');
    await page.fill('#scrobble-director', 'Denis Villeneuve');
    await page.fill('#scrobble-cinematographer', 'Greig Fraser');
    await page.fill('#scrobble-year', '2024');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // 2. Open History tab
    await page.click('#tab-history');
    await expect(page.locator('#view-history')).toBeVisible();

    const duneCard = page.locator('#history-list .scrobble-card', { hasText: 'Dune: Part Two' }).first();
    await expect(duneCard).toBeVisible();

    // Check director and cinematographer chips on card
    await expect(duneCard.locator('.scrobble-card-director')).toContainText('Denis Villeneuve');
    await expect(duneCard.locator('.scrobble-card-cinematographer')).toContainText('Greig Fraser');

    // 3. Open Detail Modal
    await duneCard.click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();

    const directorTag = page.locator('#modal-item-detail .person-tag', { hasText: 'Denis Villeneuve' });
    await expect(directorTag).toBeVisible();

    const dopTag = page.locator('#modal-item-detail .person-tag', { hasText: 'Greig Fraser' });
    await expect(dopTag).toBeVisible();

    // 4. Click cinematographer tag -> shows watches featuring Greig Fraser
    await dopTag.click();
    const watchesSec = page.locator('#detail-person-watches-container');
    await expect(watchesSec).toBeVisible();
    await expect(watchesSec).toContainText('Greig Fraser');
    await expect(watchesSec.locator('.person-history-item')).toHaveCount(1);
  });

  test('history items: fix incorrect match by specifying tvdb id', async ({ page }) => {
    const userEmail = `fix_match_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Match Fixer');
    await page.click('#btn-dev-submit');

    await expect(page.locator('#tab-timeline')).toBeVisible();

    // 1. Log a watch item
    await page.click('#btn-open-log-modal');
    await expect(page.locator('#modal-log-scrobble')).toBeVisible();
    await page.fill('#scrobble-title', 'Incorrect Match Show');
    await page.selectOption('#scrobble-media-type', 'episode');
    await page.fill('#scrobble-show', 'Breaking Bad');
    await page.fill('#scrobble-season', '1');
    await page.fill('#scrobble-episode', '1');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // 2. Open History tab and click the card to open detail modal
    await page.click('#tab-history');
    await expect(page.locator('#view-history')).toBeVisible();

    const card = page.locator('#history-list .scrobble-card', { hasText: 'Breaking Bad' }).first();
    await expect(card).toBeVisible();
    await card.click();

    await expect(page.locator('#modal-item-detail')).toBeVisible();

    // 3. Verify Fix Match button is present and click it
    const btnFixMatch = page.locator('#btn-fix-match');
    await expect(btnFixMatch).toBeVisible();
    await btnFixMatch.click();

    // 4. Verify Fix Match box appears
    const fixBox = page.locator('#detail-fix-match-box');
    await expect(fixBox).toBeVisible();

    // 5. Fill TVDB ID and submit
    await page.fill('#input-fix-match-tvdb-id', '81189');
    await page.click('#btn-submit-fix-match');

    // 6. Verify feedback and that the TVDB ID appears in footer
    await expect(page.locator('#modal-item-detail')).toContainText('TheTVDB ID: 81189');

    // 7. Dismiss detail modal
    await page.click('#btn-dismiss-detail-modal');
    await expect(page.locator('#modal-item-detail')).toBeHidden();
  });

  test('admin background tasks: force refresh metadata, poster cache, and user sync tasks', async ({ page }) => {
    page.on('dialog', dialog => dialog.accept());

    const adminEmail = `admin_force_meta_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', adminEmail);
    await page.fill('#dev-name', 'Admin Force Refresher');
    await page.click('#btn-dev-submit');

    await expect(page.locator('#tab-admin')).toBeVisible();
    await page.click('#tab-admin');

    // Switch to Background Tasks subtab
    await page.click('#admin-subnav-pills button[data-subview="admin-sub-tasks"]');
    await expect(page.locator('#admin-sub-tasks')).toBeVisible();

    // Verify top "Force Refresh All Metadata" button
    const btnForceAll = page.locator('#btn-force-refresh-all-metadata');
    await expect(btnForceAll).toBeVisible();

    // Verify "Force Refresh All" button inside Metadata Enrichment task card
    const tvdbCard = page.locator('.task-card', { hasText: /Metadata Enrichment/ });
    await expect(tvdbCard).toBeVisible();
    const btnForceTvdb = tvdbCard.locator('button.btn-force-task');
    await expect(btnForceTvdb).toBeVisible();

    // Verify "Force Refresh All" button inside Media Poster Caching card
    const posterCard = page.locator('.task-card', { hasText: 'Media Poster & Thumbnail Caching' });
    await expect(posterCard).toBeVisible();
    const btnForcePoster = posterCard.locator('button.btn-force-task');
    await expect(btnForcePoster).toBeVisible();

    // Verify "Force Refresh All" button inside User Cache Sync card
    const userSyncCard = page.locator('.task-card', { hasText: 'Plex & Media Server User Cache Sync' });
    await expect(userSyncCard).toBeVisible();
    const btnForceUserSync = userSyncCard.locator('button.btn-force-task');
    await expect(btnForceUserSync).toBeVisible();

    // Click force refresh poster button
    await btnForcePoster.click();
    await expect(btnForcePoster).toHaveText(/Triggering|Triggered|Force Refresh All/);

    // Click force refresh user sync button
    await btnForceUserSync.click();
    await expect(btnForceUserSync).toHaveText(/Triggering|Triggered|Force Refresh All/);

    // Click force refresh metadata button
    await btnForceAll.click();
    await expect(tvdbCard).toBeVisible();
  });

  test('mobile responsive: bottom app navigation, stacked forms, and responsive modal on phone viewport', async ({ page }) => {
    // 1. Set mobile viewport (iPhone 14 dimensions: 390 x 844)
    await page.setViewportSize({ width: 390, height: 844 });

    const userEmail = `mobile_ui_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Mobile Tester');
    await page.click('#btn-dev-submit');

    await expect(page.locator('#tab-timeline')).toBeVisible();

    // 2. Verify bottom app navigation bar positioning
    const navLinks = page.locator('#nav-links');
    await expect(navLinks).toBeVisible();
    const navBox = await navLinks.boundingBox();
    expect(navBox).not.toBeNull();
    // Nav bar should be docked at the bottom of the viewport
    expect(navBox.y + navBox.height).toBeGreaterThanOrEqual(840);

    // 3. Test navigation tabs on mobile (Timeline -> History -> Stats -> Settings)
    await page.click('#tab-history');
    await expect(page.locator('#view-history')).toBeVisible();

    await page.click('#tab-stats');
    await expect(page.locator('#view-stats')).toBeVisible();

    await page.click('#tab-settings');
    await expect(page.locator('#view-settings')).toBeVisible();

    // 4. Test mobile scrobble modal fit and interactions
    await page.click('#btn-open-log-modal');
    await expect(page.locator('#modal-log-scrobble')).toBeVisible();

    const logModalDialog = page.locator('#modal-log-scrobble .modal-dialog');
    const modalBox = await logModalDialog.boundingBox();
    expect(modalBox).not.toBeNull();
    // Modal width should not exceed screen width
    expect(modalBox.width).toBeLessThanOrEqual(390);

    // Close modal
    await page.click('#btn-close-log-modal');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();
  });

  test('library cleanup: list watched items on radarr/sonarr, view reclaimable space, filter, and execute cleanup action', async ({ page }) => {
    let lastRadarrDelete = null;
    let lastSonarrDelete = null;
    let lastMediaHistoryDelete = null;

    // Route /api/v1/integrations/cleanup
    await page.route('**/api/v1/integrations/cleanup', async route => {
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          radarr_configured: true,
          sonarr_configured: true,
          radarr_error: null,
          sonarr_error: null,
          total_items: 2,
          total_size_bytes: 32212254720,
          total_size_formatted: '30.0 GB',
          items: [
            {
              id: 'radarr:101',
              media_type: 'movie',
              service: 'radarr',
              service_id: 101,
              title: 'Inception',
              year: 2010,
              size_on_disk: 10737418240,
              size_formatted: '10.0 GB',
              has_file: true,
              monitored: true,
              poster_url: null,
              last_watched_at: '2026-10-15T18:00:00Z',
              watch_count: 1,
              episode_file_count: null,
              total_episode_count: null,
              watched_episodes_count: null,
              seasons: []
            },
            {
              id: 'sonarr:201',
              media_type: 'series',
              service: 'sonarr',
              service_id: 201,
              title: 'Breaking Bad',
              year: 2008,
              size_on_disk: 21474836480,
              size_formatted: '20.0 GB',
              has_file: true,
              monitored: true,
              poster_url: null,
              last_watched_at: '2026-10-17T20:00:00Z',
              watch_count: 7,
              episode_file_count: 62,
              total_episode_count: 62,
              watched_episodes_count: 7,
              seasons: [
                {
                  season_number: 1,
                  monitored: true,
                  episode_file_count: 7,
                  watched: true
                }
              ]
            }
          ]
        }),
      });
    });

    // Route Radarr delete
    await page.route('**/api/v1/integrations/radarr/delete', async route => {
      lastRadarrDelete = route.request().postDataJSON();
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'deleted', movie_id: 101 }),
      });
    });

    // Route Sonarr delete
    await page.route('**/api/v1/integrations/sonarr/delete-series', async route => {
      lastSonarrDelete = route.request().postDataJSON();
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'deleted', series_id: 201 }),
      });
    });

    // Route History by-media delete
    await page.route('**/api/v1/history/by-media*', async route => {
      lastMediaHistoryDelete = route.request().url();
      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ status: 'deleted', deleted_count: 1 }),
      });
    });

    const userEmail = `cleanup_ui_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Cleanup Tester');
    await page.click('#btn-dev-submit');

    // 1. Click Cleanup Tab
    await expect(page.locator('#tab-cleanup')).toBeVisible();
    await page.click('#tab-cleanup');
    await expect(page.locator('#view-cleanup')).toBeVisible();

    // 2. Verify summary metrics
    await expect(page.locator('#cleanup-total-size')).toHaveText('30.0 GB');
    await expect(page.locator('#cleanup-radarr-count')).toHaveText('1');
    await expect(page.locator('#cleanup-sonarr-count')).toHaveText('1');

    // 3. Verify cards
    const cards = page.locator('#cleanup-items-grid .cleanup-card');
    await expect(cards).toHaveCount(2);
    await expect(page.locator('#cleanup-items-grid')).toContainText('Inception');
    await expect(page.locator('#cleanup-items-grid')).toContainText('Breaking Bad');

    // 4. Test filter chips
    await page.click('#btn-cleanup-filter-movies');
    await expect(page.locator('#cleanup-items-grid .cleanup-card')).toHaveCount(1);
    await expect(page.locator('#cleanup-items-grid')).toContainText('Inception');

    await page.click('#btn-cleanup-filter-series');
    await expect(page.locator('#cleanup-items-grid .cleanup-card')).toHaveCount(1);
    await expect(page.locator('#cleanup-items-grid')).toContainText('Breaking Bad');

    await page.click('#btn-cleanup-filter-all');
    await expect(page.locator('#cleanup-items-grid .cleanup-card')).toHaveCount(2);

    // 5. Test Radarr modal & action with "Also remove from watch history" checked
    await page.locator('.cleanup-card', { hasText: 'Inception' }).click();
    await expect(page.locator('#modal-cleanup-action')).toBeVisible();
    await expect(page.locator('#cleanup-modal-item-title')).toContainText('Inception');
    await expect(page.locator('#opt-radarr-delete-both')).toBeVisible();
    await expect(page.locator('#opt-radarr-delete-entry-only')).toBeVisible();
    await expect(page.locator('#opt-cleanup-history-only')).toBeVisible();
    await expect(page.locator('#cleanup-also-remove-history')).toBeVisible();

    // Check the also-remove-history checkbox
    await page.check('#cleanup-also-remove-history');

    page.once('dialog', dialog => dialog.accept());
    await page.click('#opt-radarr-delete-both');
    await expect(page.locator('#cleanup-modal-status')).toContainText('Successfully removed');
    expect(lastRadarrDelete).toEqual({
      movie_id: 101,
      delete_files: true,
    });
    expect(lastMediaHistoryDelete).toContain('media_type=movie');
    expect(lastMediaHistoryDelete).toContain('Inception');

    // 6. Test Sonarr modal & "Remove from Watch History Only" action
    lastMediaHistoryDelete = null;
    await page.locator('.cleanup-card', { hasText: 'Breaking Bad' }).click();
    await expect(page.locator('#modal-cleanup-action')).toBeVisible();
    await expect(page.locator('#cleanup-modal-item-title')).toContainText('Breaking Bad');
    await expect(page.locator('#opt-sonarr-delete-series')).toBeVisible();
    await expect(page.locator('#opt-cleanup-history-only')).toBeVisible();

    page.once('dialog', dialog => dialog.accept());
    await page.click('#opt-cleanup-history-only');
    await expect(page.locator('#cleanup-modal-status')).toContainText('Successfully removed');
    expect(lastMediaHistoryDelete).toContain('media_type=series');
    expect(lastMediaHistoryDelete).toContain('Breaking');
  });

  test('admin settings: configure metadata source and tmdb api key', async ({ page }) => {
    const adminEmail = `admin_tmdb_${Date.now()}@bingesync.local`;

    await page.goto('/');

    // 1. Sign in as admin
    await page.fill('#dev-email', adminEmail);
    await page.fill('#dev-name', 'Admin TMDB User');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toContainText('Admin TMDB User');

    // 2. Navigate to Admin > Services & API Keys
    await page.click('#tab-admin');
    await page.click('[data-subview="admin-sub-settings"]');
    await expect(page.locator('#admin-sub-settings')).toBeVisible();

    // 3. Verify metadata source dropdown exists
    const sourceSelect = page.locator('#setting-metadata-source');
    await expect(sourceSelect).toBeVisible();

    // 4. Select TMDB as metadata source
    await sourceSelect.selectOption('tmdb');

    // 5. Fill TMDB API key and base URL
    await page.fill('#setting-tmdb-api-key', 'tmdb_ui_test_key_789');
    await page.fill('#setting-tmdb-base-url', 'https://api.themoviedb.org/3');

    // 6. Save settings
    await page.click('#btn-save-admin-settings');
    await expect(page.locator('#admin-settings-status')).toContainText('Settings saved successfully');

    // 7. Verify TMDB key is masked
    await expect(page.locator('#setting-tmdb-api-key')).toHaveValue(/^tm.*•/);

    // 8. Verify toggle unhide secret
    const toggleBtn = page.locator('#setting-tmdb-api-key + .btn-toggle-secret');
    await toggleBtn.click();
    await expect(page.locator('#setting-tmdb-api-key')).toHaveValue('tmdb_ui_test_key_789');
    await toggleBtn.click();
    await expect(page.locator('#setting-tmdb-api-key')).toHaveValue(/^tm.*•/);

    // 9. Verify About subtab, dynamic system components, and check for updates
    await page.click('#admin-subnav-pills button[data-subview="admin-sub-about"]');
    await expect(page.locator('#admin-sub-about')).toBeVisible();
    await expect(page.locator('#admin-about-version-card')).toBeVisible();
    await expect(page.locator('#admin-version-tag')).toContainText('v0.');
    const checkUpdatesBtn = page.locator('#btn-check-version-updates');
    await expect(checkUpdatesBtn).toBeVisible();
    await checkUpdatesBtn.click();
    await expect(page.locator('#admin-version-status-desc')).toBeVisible();

    // Verify dynamic system component versions table in About
    await expect(page.locator('#admin-system-components-table')).toBeVisible();
    await expect(page.locator('#admin-system-components-tbody')).toContainText('Docker Base Image');
    await expect(page.locator('#admin-system-components-tbody')).toContainText('SQLite Engine');
    await expect(page.locator('#admin-system-components-tbody')).toContainText('Rust Toolchain');

    // Switch back to settings subtab for standard user group testing
    await page.click('#admin-subnav-pills button[data-subview="admin-sub-settings"]');
    await expect(page.locator('#admin-sub-settings')).toBeVisible();
    await expect(page.locator('#setting-tmdb-base-url')).toHaveValue('https://api.themoviedb.org/3');

    // 10. Verify standard user group setting
    await expect(page.locator('#setting-oidc-user-group')).toBeVisible();
    await page.fill('#setting-oidc-user-group', 'family, friends');
    await page.click('#btn-save-admin-settings');
    await expect(page.locator('#admin-settings-status')).toContainText('Settings saved successfully');
    await expect(page.locator('#setting-oidc-user-group')).toHaveValue('family, friends');

    // 9. Reset source back to tvdb to leave environment clean
    await sourceSelect.selectOption('tvdb');
    await page.click('#btn-save-admin-settings');
    await expect(page.locator('#admin-settings-status')).toContainText('Settings saved successfully');
  });

  test('admin database backup and restore flow', async ({ page }) => {
    const adminEmail = `admin_backup_${Date.now()}@bingesync.local`;

    await page.goto('/');

    // 1. Sign in as admin
    await page.fill('#dev-email', adminEmail);
    await page.fill('#dev-name', 'Admin Backup User');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toContainText('Admin Backup User');

    // 2. Navigate to Admin > Database Backups & Restore
    await page.click('#tab-admin');
    await page.click('#pill-admin-backups');
    await expect(page.locator('#admin-sub-backups')).toBeVisible();

    // 3. Verify backup path and create backup
    await expect(page.locator('#admin-backups-path')).toBeVisible();
    await page.click('#btn-admin-create-backup');

    // 4. Verify backup is created and appears in table
    await expect(page.locator('#admin-backups-tbody tr').first()).toContainText('bingesync_backup_', { timeout: 10000 });
    const backupRow = page.locator('#admin-backups-tbody tr').first();
    await expect(backupRow).toContainText('bingesync_backup_');

    // 5. Open restore modal
    await backupRow.locator('.btn-restore-backup').click();
    await expect(page.locator('#modal-restore-backup')).toBeVisible();
    await expect(page.locator('#restore-target-filename')).toContainText('bingesync_backup_');

    // 6. Confirm restore
    await page.click('#btn-confirm-restore-backup');
    await expect(page.locator('#restore-modal-status')).toContainText('restored', { timeout: 10000 });

    // 7. Modal closes and backups table is visible
    await expect(page.locator('#modal-restore-backup')).not.toBeVisible({ timeout: 5000 });
  });

  test('theme accent switcher changes theme and persists', async ({ page }) => {
    await page.goto('/');

    // Sign in
    const testEmail = `theme_${Date.now()}@bingesync.local`;
    await page.fill('#dev-email', testEmail);
    await page.fill('#dev-name', 'Theme User');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toContainText('Theme User');

    // Verify theme selector was removed from header
    await expect(page.locator('#nav-theme-wrap')).toHaveCount(0);
    await expect(page.locator('#nav-theme-select')).toHaveCount(0);

    // Go to Settings view and test theme selection
    await page.click('#tab-settings');
    await expect(page.locator('#view-settings')).toBeVisible();
    await page.click('[data-subview="settings-sub-preferences"]');

    // Verify theme dropdown exists and has pure colour options
    const themeSelect = page.locator('#user-setting-theme');
    await expect(themeSelect).toBeVisible();
    await expect(themeSelect.locator('option[value="red"]')).toHaveText('Red');
    await expect(themeSelect.locator('option[value="amber"]')).toHaveText('Amber');
    await expect(themeSelect.locator('option[value="purple"]')).toHaveText('Purple');
    await expect(themeSelect.locator('option[value="cyan"]')).toHaveText('Cyan');
    await expect(themeSelect.locator('option[value="emerald"]')).toHaveText('Emerald');
    await expect(themeSelect.locator('option[value="rose"]')).toHaveText('Rose');
    await expect(themeSelect.locator('option[value="oled"]')).toHaveText('Black');

    await themeSelect.selectOption('amber');
    await expect(page.locator('body')).toHaveAttribute('data-theme', 'amber');

    await themeSelect.selectOption('purple');
    await expect(page.locator('body')).toHaveAttribute('data-theme', 'purple');

    await themeSelect.selectOption('cyan');
    await expect(page.locator('body')).toHaveAttribute('data-theme', 'cyan');

    // Reload page to verify persistence
    await page.reload();
    await expect(page.locator('body')).toHaveAttribute('data-theme', 'cyan');
  });

  test('missing metadata filter and badge in history and timeline', async ({ page }) => {
    await page.goto('/');

    const testEmail = `missing_meta_ui_${Date.now()}@bingesync.local`;
    await page.fill('#dev-email', testEmail);
    await page.fill('#dev-name', 'Missing Meta UI User');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toContainText('Missing Meta UI User');

    // Log an item with missing metadata (no overview, no genres, default poster)
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'movie');
    await page.fill('#scrobble-title', 'Unmatched Film 123');
    await page.fill('#scrobble-year', '2025');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // Check timeline shows badge "Needs Match"
    await expect(page.locator('#timeline-container .badge-missing-metadata')).toBeVisible();

    // Go to History tab
    await page.click('#tab-history');
    await expect(page.locator('#view-history')).toBeVisible();
    await expect(page.locator('#history-list .badge-missing-metadata')).toBeVisible();

    // Click Missing Metadata filter pill
    await page.click('#pill-missing-metadata');
    await expect(page.locator('#pill-missing-metadata')).toHaveClass(/active/);
    await expect(page.locator('#history-list')).toContainText('Unmatched Film 123');
  });

  test('weekly viewing heatmap and binge wrapped modal flow', async ({ page }) => {
    await page.goto('/');

    const testEmail = `wrapped_${Date.now()}@bingesync.local`;
    await page.fill('#dev-email', testEmail);
    await page.fill('#dev-name', 'Wrapped User');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toContainText('Wrapped User');

    // Log a watch to have data
    await page.click('#btn-open-log-modal');
    await page.fill('#scrobble-title', 'Interstellar');
    await page.fill('#scrobble-year', '2014');
    await page.fill('#scrobble-duration', '169');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // Navigate to Stats view
    await page.click('#tab-stats');
    await expect(page.locator('#view-stats')).toBeVisible();

    // Verify weekly heatmap grid is rendered with cells
    await expect(page.locator('#weekly-heatmap-grid')).toBeVisible();
    const cellCount = await page.locator('#weekly-heatmap-grid .heatmap-cell').count();
    expect(cellCount).toBe(168); // 7 days * 24 hours

    // Open Binge Wrapped modal
    await page.click('#btn-open-wrapped');
    await expect(page.locator('#modal-wrapped')).toBeVisible();
    await expect(page.locator('#wrapped-metrics-grid')).toBeVisible();
    await expect(page.locator('#wrapped-year-badge')).toBeVisible();
    await expect(page.locator('#wrapped-longest-movie-title')).toContainText('Interstellar');

    // Test copy share card button
    await page.locator('#btn-copy-wrapped-summary').scrollIntoViewIfNeeded();
    await page.click('#btn-copy-wrapped-summary');
    await expect(page.locator('#btn-copy-wrapped-summary')).toContainText(/Copied/);

    // Close modal
    await page.click('#btn-close-wrapped-modal');
    await expect(page.locator('#modal-wrapped')).toBeHidden();
  });

  test('settings view: emby webhooks, outbound webhooks, and data export', async ({ page }) => {
    await page.goto('/');

    const testEmail = `settings_ext_${Date.now()}@bingesync.local`;
    await page.fill('#dev-email', testEmail);
    await page.fill('#dev-name', 'Settings Tester');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toContainText('Settings Tester');

    // Navigate to Settings
    await page.click('#tab-settings');
    await expect(page.locator('#view-settings')).toBeVisible();

    // 1. Verify Emby Webhook URL input and copy button exist
    await expect(page.locator('#emby-webhook-url-input')).toBeVisible();
    const embyUrl = await page.locator('#emby-webhook-url-input').inputValue();
    expect(embyUrl).toContain('/api/v1/webhooks/emby');

    await page.click('#btn-copy-emby-webhook');
    await expect(page.locator('#emby-copy-status')).toBeVisible();

    // 2. Configure Emby username and Outbound webhooks
    await page.click('[data-subview="settings-sub-services"]');
    await page.fill('#user-setting-emby-username', 'emby_tester');
    await page.selectOption('#user-setting-outbound-type', 'discord');
    await page.fill('#user-setting-outbound-url', 'https://discord.com/api/webhooks/123/abc');

    await page.click('#btn-save-user-settings');
    await expect(page.locator('#user-settings-status')).toContainText('Settings saved successfully');

    // 3. Test One-Click Data Export button
    await page.click('[data-subview="settings-sub-data"]');
    await expect(page.locator('#btn-export-user-data')).toBeVisible();
  });

  test('fix match supports TMDB ID and searching matching titles', async ({ page }) => {
    const userEmail = `user_tmdb_fix_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'TMDB Fixer');
    await page.click('#btn-dev-submit');

    await expect(page.locator('#btn-open-log-modal')).toBeVisible();

    // 1. Log a watch item
    await page.click('#btn-open-log-modal');
    await expect(page.locator('#modal-log-scrobble')).toBeVisible();
    await page.fill('#scrobble-title', 'Chernobyl');
    await page.selectOption('#scrobble-media-type', 'movie');
    await page.fill('#scrobble-year', '2019');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // 2. Open item detail modal
    const card = page.locator('.scrobble-card', { hasText: 'Chernobyl' }).first();
    await expect(card).toBeVisible();
    await card.click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();

    // 3. Click Fix Match
    await page.click('#btn-fix-match');
    await expect(page.locator('#detail-fix-match-box')).toBeVisible();

    // 4. Verify provider select and search button exist
    await expect(page.locator('#select-fix-match-provider')).toBeVisible();
    await expect(page.locator('#btn-search-fix-match')).toBeVisible();

    // 5. Test search with title
    await page.fill('#input-fix-match-tvdb-id', 'Chernobyl');
    await page.click('#btn-search-fix-match');
    await expect(page.locator('#fix-match-feedback')).toBeVisible();

    // 6. Select TMDB provider, fill TMDB ID, and apply
    await page.selectOption('#select-fix-match-provider', 'tmdb');
    await page.fill('#input-fix-match-tvdb-id', '87108');
    await page.click('#btn-submit-fix-match');

    // 7. Verify TMDB ID is updated on modal footer
    await expect(page.locator('#modal-item-detail')).toContainText('TMDB ID: 87108');

    // 8. Close modal
    await page.click('#btn-dismiss-detail-modal');
    await expect(page.locator('#modal-item-detail')).toBeHidden();
  });

  test('custom tagging, personal review notes, and history filtering', async ({ page }) => {
    await page.goto('/');

    const testEmail = `tags_notes_${Date.now()}@bingesync.local`;
    await page.fill('#dev-email', testEmail);
    await page.fill('#dev-name', 'Tag Tester');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toContainText('Tag Tester');

    // 1. Log a watch
    await page.click('#btn-open-log-modal');
    await page.fill('#scrobble-title', 'Inception');
    await page.fill('#scrobble-year', '2010');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // 2. Open item detail modal
    const card = page.locator('.scrobble-card', { hasText: 'Inception' }).first();
    await expect(card).toBeVisible();
    await card.click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();

    // 3. Add custom tags and personal review notes
    await expect(page.locator('#detail-tags-input')).toBeVisible();
    await expect(page.locator('#detail-notes-input')).toBeVisible();
    await page.fill('#detail-tags-input', 'Rewatch, Plane Movie, Watched with Daryl');
    await page.fill('#detail-notes-input', 'Mind-bending dream heist masterpiece.');
    await page.click('#btn-save-detail-user-data');
    await expect(page.locator('#detail-user-data-feedback')).toBeVisible();
    await expect(page.locator('#detail-user-data-feedback')).toContainText(/saved/i);

    // 4. Close modal and verify tags & notes snippet appear on card
    await page.click('#btn-dismiss-detail-modal');
    await expect(page.locator('#modal-item-detail')).toBeHidden();

    await expect(card.locator('.tag-chip-card', { hasText: 'Plane Movie' })).toBeVisible();
    await expect(card.locator('.scrobble-card-notes')).toContainText('Mind-bending dream heist masterpiece.');

    // 5. Test clicking tag chip filters history
    await card.locator('.tag-chip-card', { hasText: 'Plane Movie' }).click();
    await expect(page.locator('#view-history')).toBeVisible();
    await expect(page.locator('#history-person-badge')).toBeVisible();
    await expect(page.locator('#history-person-badge')).toContainText('Tag: Plane Movie');

    // 6. Verify Binge Wrapped modal top posters use uniform wrapped-card-poster
    await page.click('#tab-stats');
    await expect(page.locator('#view-stats')).toBeVisible();
    await page.click('#btn-open-wrapped');
    await expect(page.locator('#modal-wrapped')).toBeVisible();
    const wrappedCards = page.locator('.wrapped-top-card');
    await expect(wrappedCards.first()).toBeVisible();
    await expect(page.locator('.wrapped-card-poster').first()).toBeVisible();
    await page.click('#btn-close-wrapped-modal');
    await expect(page.locator('#modal-wrapped')).toBeHidden();
  });

  test('duplicate watch detector: scan for duplicate scrobbles and prune redundant watches', async ({ page }) => {
    const userEmail = `dup_user_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Dup Hunter');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toContainText('Dup Hunter');

    // 1. Log two watches for the exact same movie to create a duplicate pair
    await page.click('#btn-open-log-modal');
    await expect(page.locator('#modal-log-scrobble')).toBeVisible();
    await page.fill('#scrobble-title', 'Interstellar');
    await page.fill('#scrobble-year', '2014');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // Small delay to ensure second watch gets a distinct timestamp
    await page.waitForTimeout(600);

    await page.click('#btn-open-log-modal');
    await expect(page.locator('#modal-log-scrobble')).toBeVisible();
    await page.fill('#scrobble-title', 'Interstellar');
    await page.fill('#scrobble-year', '2014');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // 2. Open History tab and verify "Duplicates" button is visible
    await page.click('#tab-history');
    await expect(page.locator('#view-history')).toBeVisible();
    await expect(page.locator('#btn-open-duplicates-modal')).toBeVisible();

    // 3. Click "Duplicates" button to open modal
    await page.click('#btn-open-duplicates-modal');
    await expect(page.locator('#modal-duplicates')).toBeVisible();

    // 4. Verify duplicate detector finds Interstellar duplicate group
    const dupCard = page.locator('#dup-groups-list .dup-group-card', { hasText: 'Interstellar' });
    await expect(dupCard).toBeVisible({ timeout: 5000 });
    await expect(dupCard).toContainText('2 watches');
    await expect(page.locator('#btn-clean-keep-oldest')).toBeVisible();

    // 5. Click "Keep Oldest (Prune Redundant)"
    await page.click('#btn-clean-keep-oldest');
    await expect(page.locator('#dup-status-banner')).toBeVisible();
    await expect(page.locator('#dup-status-banner')).toContainText(/pruned/i);

    // 6. Verify duplicate list clears and shows empty state
    await expect(page.locator('#dup-empty-state')).toBeVisible({ timeout: 5000 });

    // 7. Close modal and check history only has 1 scrobble remaining
    await page.click('#btn-close-duplicates-modal');
    await expect(page.locator('#modal-duplicates')).toBeHidden();

    const interstellarCards = page.locator('#history-list .scrobble-card', { hasText: 'Interstellar' });
    await expect(interstellarCards).toHaveCount(1);
  });

  test('history view: filter movies and TV shows above a certain rating threshold', async ({ page }) => {
    const userEmail = `rater_filter_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Rating Filter Tester');
    await page.click('#btn-dev-submit');

    // 1. Log movie: Inception (rated 8)
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'movie');
    await page.fill('#scrobble-title', 'Inception');
    await page.fill('#scrobble-year', '2010');
    await page.selectOption('#scrobble-rating', '8');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // 2. Log movie: The Dark Knight (rated 10)
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'movie');
    await page.fill('#scrobble-title', 'The Dark Knight');
    await page.fill('#scrobble-year', '2008');
    await page.selectOption('#scrobble-rating', '10');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // 3. Log episode: Ozymandias (rated 9)
    await page.click('#btn-open-log-modal');
    await page.selectOption('#scrobble-media-type', 'episode');
    await page.fill('#scrobble-show', 'Breaking Bad');
    await page.fill('#scrobble-season', '5');
    await page.fill('#scrobble-episode', '14');
    await page.fill('#scrobble-title', 'Ozymandias');
    await page.selectOption('#scrobble-rating', '9');
    await page.click('#btn-submit-scrobble');
    await expect(page.locator('#modal-log-scrobble')).toBeHidden();

    // 4. Switch to History tab
    await page.click('#tab-history');
    await expect(page.locator('#view-history')).toBeVisible();
    await expect(page.locator('#history-rating-filter')).toBeVisible();

    // With All Ratings, all 3 items appear
    await expect(page.locator('#history-list .scrobble-card')).toHaveCount(3);

    // 5. Filter by 8+ Stars threshold -> all 3 appear (ratings: 10, 9, 8)
    await page.selectOption('#history-rating-filter', '8+');
    await expect(page.locator('#history-list .scrobble-card')).toHaveCount(3);

    // 6. Filter by 9+ Stars threshold -> only 2 appear (The Dark Knight: 10, Ozymandias: 9)
    await page.selectOption('#history-rating-filter', '9+');
    await expect(page.locator('#history-list .scrobble-card')).toHaveCount(2);
    await expect(page.locator('#history-list')).toContainText('The Dark Knight');
    await expect(page.locator('#history-list')).toContainText('Ozymandias');
    await expect(page.locator('#history-list')).not.toContainText('Inception');

    // 7. Filter by 10/10 Stars threshold -> only The Dark Knight appears
    await page.selectOption('#history-rating-filter', '10');
    await expect(page.locator('#history-list .scrobble-card')).toHaveCount(1);
    await expect(page.locator('#history-list')).toContainText('The Dark Knight');

    // 8. Combine rating threshold with media type filters:
    // Select 8+ stars threshold and switch to "Movies" pill -> only Inception & The Dark Knight
    await page.selectOption('#history-rating-filter', '8+');
    await page.click('#pill-movies');
    await expect(page.locator('#history-list .scrobble-card')).toHaveCount(2);
    await expect(page.locator('#history-list')).toContainText('The Dark Knight');
    await expect(page.locator('#history-list')).toContainText('Inception');
    await expect(page.locator('#history-list')).not.toContainText('Ozymandias');

    // Switch to "Episodes" pill with 8+ stars -> only Ozymandias
    await page.click('#pill-episodes');
    await expect(page.locator('#history-list .scrobble-card')).toHaveCount(1);
    await expect(page.locator('#history-list')).toContainText('Ozymandias');
    await expect(page.locator('#history-list')).not.toContainText('Inception');
    await expect(page.locator('#history-list')).not.toContainText('The Dark Knight');

    // 9. Reset rating filter to "all" and media type to "all"
    await page.selectOption('#history-rating-filter', 'all');
    await page.click('#pill-all');
    await expect(page.locator('#history-list .scrobble-card')).toHaveCount(3);
  });

  test('first-run setup wizard: step navigation, copyable redirect URIs, test connection, and review summary', async ({ page }) => {
    // 1. Visit app
    await page.goto('/');

    // 2. Open setup wizard
    await page.evaluate(() => {
      if (window.showSetupState) {
        window.showSetupState();
      }
    });

    await expect(page.locator('#state-setup')).toBeVisible();
    await expect(page.locator('.setup-header h2')).toContainText('Configure OpenID Connect');
    await expect(page.locator('#setup-step-1')).toBeVisible();

    // 3. Step 1: Check inputs and test connection feedback on empty URL
    await page.fill('#setup-issuer-url', '');
    await page.click('#btn-setup-test-oidc');
    await expect(page.locator('#setup-test-feedback')).toContainText('Please enter an OIDC Issuer URL first');

    // Fill valid issuer URL and advance to Step 2
    await page.fill('#setup-issuer-url', 'https://auth.example.com/application/o/bingesync/');
    await page.click('#btn-setup-step1-next');

    // 4. Step 2: Credentials and Copyable URIs
    await expect(page.locator('#setup-step-2')).toBeVisible();
    await expect(page.locator('#step-nav-2')).toHaveClass(/active/);
    await expect(page.locator('#step-nav-1')).toHaveClass(/completed/);

    // Verify copyable redirect URIs are populated
    const redirectVal = await page.inputValue('#setup-redirect-uri');
    expect(redirectVal).toContain('/api/v1/auth/callback');
    const logoutVal = await page.inputValue('#setup-logout-uri');
    expect(logoutVal).toContain('/api/v1/auth/backchannel-logout');

    // Advance to Step 3 with Client ID
    await page.fill('#setup-client-id', 'bingesync-client');
    await page.fill('#setup-client-secret', 'test-secret-123');
    await page.click('#btn-setup-step2-next');

    // 5. Step 3: Admin Group & Review Summary
    await expect(page.locator('#setup-step-3')).toBeVisible();
    await expect(page.locator('#step-nav-3')).toHaveClass(/active/);
    await page.fill('#setup-admin-group', 'bingesync-admins');

    await expect(page.locator('#review-issuer')).toContainText('auth.example.com');
    await expect(page.locator('#review-client-id')).toContainText('bingesync-client');
    await expect(page.locator('#review-redirect-uri')).toContainText('/api/v1/auth/callback');

    // Verify Step Navigation backwards works
    await page.click('#btn-setup-step3-back');
    await expect(page.locator('#setup-step-2')).toBeVisible();
    await page.click('#btn-setup-step2-back');
    await expect(page.locator('#setup-step-1')).toBeVisible();

    // Verify skip to dev login
    await page.click('#btn-skip-setup-to-dev');
    await expect(page.locator('#state-setup')).toBeHidden();
    await expect(page.locator('#state-auth')).toBeVisible();
  });

  test('user api keys: generate personal api key, view prefix in table, copy key, and revoke key', async ({ page }) => {
    const userEmail = `user_keys_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'API Key Tester');
    await page.click('#btn-dev-submit');

    // 1. Navigate to Settings view
    const keysLoaded = page.waitForResponse(resp => resp.url().includes('/api/v1/user/api-keys') && resp.status() === 200);
    await page.click('#tab-settings');
    await keysLoaded;

    await expect(page.locator('#view-settings')).toBeVisible();
    await page.click('[data-subview="settings-sub-api-keys"]');
    await expect(page.locator('#card-user-api-keys')).toBeVisible();
    await expect(page.locator('#user-api-keys-empty')).toBeVisible();

    // 2. Open generate modal
    await page.click('#btn-open-create-api-key-modal');
    await expect(page.locator('#modal-create-api-key')).toBeVisible();
    await expect(page.locator('#api-key-create-step')).toBeVisible();

    // 3. Fill key name and submit
    await page.fill('#input-api-key-name', 'Home Assistant Integration');
    const keyCreated = page.waitForResponse(resp => resp.url().includes('/api/v1/user/api-keys') && resp.status() === 200);
    await page.click('#btn-submit-create-api-key');
    await keyCreated;

    // 4. Verify generated key step displays live key with bs_live_ prefix
    await expect(page.locator('#api-key-success-step')).toBeVisible();
    const generatedKey = await page.inputValue('#input-generated-api-key');
    expect(generatedKey).toMatch(/^bs_live_[A-Za-z0-9_-]{43}$/);

    // 5. Click Done to close modal
    await page.click('#btn-done-api-key-modal');
    await expect(page.locator('#modal-create-api-key')).toBeHidden();

    // 6. Verify key appears in active keys table with prefix and name
    await expect(page.locator('#user-api-keys-table-wrap')).toBeVisible();
    await expect(page.locator('#user-api-keys-tbody')).toContainText('Home Assistant Integration');
    await expect(page.locator('#user-api-keys-tbody')).toContainText('bs_live_');
    await expect(page.locator('#user-api-keys-empty')).toBeHidden();

    // 7. Revoke key
    page.once('dialog', dialog => dialog.accept());
    const keyDeleted = page.waitForResponse(resp => resp.url().includes('/api/v1/user/api-keys') && resp.status() === 200);
    await page.click('.btn-revoke-api-key');
    await keyDeleted;

    // 8. Verify table is empty again
    await expect(page.locator('#user-api-keys-empty')).toBeVisible();
    await expect(page.locator('#user-api-keys-table-wrap')).toBeHidden();
  });

  test('settings menu: sub-navigation tabs switch between Webhooks, Connected Services, API Keys, Preferences, and Data', async ({ page }) => {
    const userEmail = `user_tabs_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Tabs User');
    await page.click('#btn-dev-submit');

    // 1. Navigate to Settings view
    await page.click('#tab-settings');
    await expect(page.locator('#view-settings')).toBeVisible();
    await expect(page.locator('#settings-subnav-pills')).toBeVisible();

    // Verify all 5 pills exist
    await expect(page.locator('#pill-settings-webhooks')).toBeVisible();
    await expect(page.locator('#pill-settings-services')).toBeVisible();
    await expect(page.locator('#pill-settings-api-keys')).toBeVisible();
    await expect(page.locator('#pill-settings-preferences')).toBeVisible();
    await expect(page.locator('#pill-settings-data')).toBeVisible();

    // 2. Default active tab is Webhooks
    await expect(page.locator('#pill-settings-webhooks')).toHaveClass(/active/);
    await expect(page.locator('#settings-sub-webhooks')).toBeVisible();
    await expect(page.locator('#settings-sub-services')).toBeHidden();
    await expect(page.locator('#settings-sub-api-keys')).toBeHidden();

    // 3. Switch to Connected Services
    await page.click('#pill-settings-services');
    await expect(page.locator('#pill-settings-services')).toHaveClass(/active/);
    await expect(page.locator('#pill-settings-webhooks')).not.toHaveClass(/active/);
    await expect(page.locator('#settings-sub-services')).toBeVisible();
    await expect(page.locator('#settings-sub-webhooks')).toBeHidden();

    // 4. Switch to API Keys
    await page.click('#pill-settings-api-keys');
    await expect(page.locator('#pill-settings-api-keys')).toHaveClass(/active/);
    await expect(page.locator('#settings-sub-api-keys')).toBeVisible();
    await expect(page.locator('#settings-sub-services')).toBeHidden();

    // 5. Switch to Preferences
    await page.click('#pill-settings-preferences');
    await expect(page.locator('#pill-settings-preferences')).toHaveClass(/active/);
    await expect(page.locator('#settings-sub-preferences')).toBeVisible();
    await expect(page.locator('#settings-sub-api-keys')).toBeHidden();

    // 6. Switch to Data & Export
    await page.click('#pill-settings-data');
    await expect(page.locator('#pill-settings-data')).toHaveClass(/active/);
    await expect(page.locator('#settings-sub-data')).toBeVisible();
    await expect(page.locator('#settings-sub-preferences')).toBeHidden();

    // 7. Switch back to Webhooks
    await page.click('#pill-settings-webhooks');
    await expect(page.locator('#pill-settings-webhooks')).toHaveClass(/active/);
    await expect(page.locator('#settings-sub-webhooks')).toBeVisible();
    await expect(page.locator('#settings-sub-data')).toBeHidden();
  });

  test('tautulli continuous sync: user settings user filter and background jobs task card', async ({ page }) => {
    const adminEmail = `admin_tautulli_sync_${Date.now()}@bingesync.local`;
    await page.goto('/');
    await page.fill('#dev-email', adminEmail);
    await page.fill('#dev-name', 'Sync Admin');
    await page.click('#btn-dev-submit');

    // 1. Open Settings -> Connected Services
    await page.click('#tab-settings');
    await expect(page.locator('#view-settings')).toBeVisible();
    await page.click('#pill-settings-services');
    await expect(page.locator('#settings-sub-services')).toBeVisible();

    // 2. Verify Tautulli section elements exist
    await expect(page.locator('#user-setting-tautulli-url')).toBeVisible();
    await expect(page.locator('#user-setting-tautulli-api-key')).toBeVisible();
    await expect(page.locator('#user-setting-tautulli-user')).toBeVisible();
    await expect(page.locator('#btn-fetch-tautulli-setting-users')).toBeVisible();
    await expect(page.locator('#user-setting-tautulli-sync-enabled')).toBeVisible();

    // 3. Fill in Tautulli settings
    await page.fill('#user-setting-tautulli-url', 'http://127.0.0.1:8181');
    await page.fill('#user-setting-tautulli-api-key', 'tautulli_secret_token');

    // Add an option to the select to simulate selected user
    await page.evaluate(() => {
      const select = document.getElementById('user-setting-tautulli-user');
      const opt = document.createElement('option');
      opt.value = 'sync_plex_user';
      opt.textContent = 'Sync Plex User (sync_plex_user)';
      select.appendChild(opt);
      select.value = 'sync_plex_user';
    });

    await page.check('#user-setting-tautulli-sync-enabled');

    // 4. Save settings
    await page.click('#btn-save-user-settings');
    await expect(page.locator('#user-settings-status')).toContainText('Settings saved successfully');

    // 5. Reload and verify settings persisted
    await page.reload();
    await page.click('#tab-settings');
    await page.click('#pill-settings-services');
    await expect(page.locator('#user-setting-tautulli-url')).toHaveValue('http://127.0.0.1:8181');
    await expect(page.locator('#user-setting-tautulli-user')).toHaveValue('sync_plex_user');
    await expect(page.locator('#user-setting-tautulli-sync-enabled')).toBeChecked();

    // 6. Switch to Admin Tasks and verify Tautulli Sync task card
    await page.click('#tab-admin');
    await page.click('#admin-subnav-pills button[data-subview="admin-sub-tasks"]');
    await expect(page.locator('#admin-sub-tasks')).toBeVisible();

    const tautulliCard = page.locator('.task-card', { hasText: 'Tautulli Continuous 1-Way Sync' });
    await expect(tautulliCard).toBeVisible();
    await expect(tautulliCard).toContainText('Every 1 hour');
    await expect(tautulliCard.locator('.btn-trigger-task')).toBeVisible();
    await expect(tautulliCard.locator('.btn-task-logs')).toBeVisible();

    // 7. Click Run Now on Tautulli task
    await tautulliCard.locator('.btn-trigger-task').click();
    await expect(tautulliCard).toBeVisible();

    // 8. Click View Logs and verify navigation to Logs view with tautulli filter
    await tautulliCard.locator('.btn-task-logs').click();
    await expect(page.locator('#admin-sub-logs')).toBeVisible();
    await expect(page.locator('#admin-logs-filter-search')).toHaveValue('tautulli_sync');
  });

  test('user settings: bidirectional media sync and plex server configuration', async ({ page }) => {
    const userEmail = `sync_bidi_${Date.now()}@bingesync.local`;

    // 1. Sign in
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'Sync Bidi Tester');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toHaveText('Sync Bidi Tester');
    await expect(page.locator('#view-timeline')).toBeVisible();

    // 2. Open Settings -> Connected Services
    await page.click('#tab-settings');
    await page.click('#pill-settings-services');

    // 3. Verify Plex and Bidirectional elements exist
    await expect(page.locator('#user-setting-plex-url')).toBeVisible();
    await expect(page.locator('#user-setting-plex-api-key')).toBeVisible();
    await expect(page.locator('#btn-test-plex-connection')).toBeVisible();
    await expect(page.locator('#btn-test-jellyfin-connection')).toBeVisible();
    await expect(page.locator('#user-setting-bidirectional-sync-enabled')).toBeVisible();
    await expect(page.locator('#btn-trigger-writeback-now')).toBeVisible();

    // 4. Test clicking connection test without URL shows validation message
    await page.click('#btn-test-plex-connection');
    await expect(page.locator('#plex-test-status')).toContainText('Please enter a Plex Server URL first');

    await page.click('#btn-test-jellyfin-connection');
    await expect(page.locator('#jellyfin-test-status')).toContainText('Please enter a Jellyfin Server URL first');

    // 5. Fill Plex settings and check Bidirectional Sync
    await page.fill('#user-setting-plex-url', 'http://192.168.1.100:32400');
    await page.fill('#user-setting-plex-api-key', 'plex_test_token_xyz987');
    await page.check('#user-setting-bidirectional-sync-enabled');

    // 6. Save Settings
    await page.click('#btn-save-user-settings');
    await expect(page.locator('#user-settings-status')).toContainText('Settings saved successfully');

    // 7. Reload and verify settings persisted
    await page.reload();
    await page.click('#tab-settings');
    await page.click('#pill-settings-services');
    await expect(page.locator('#user-setting-plex-url')).toHaveValue('http://192.168.1.100:32400');
    await expect(page.locator('#user-setting-bidirectional-sync-enabled')).toBeChecked();

    // 8. Test triggering manual writeback button
    await page.click('#btn-trigger-writeback-now');
    await expect(page.locator('#writeback-status')).toBeVisible();
  });

  test('scrobble source and watched-by user: badges, filtering in history & timeline, and detail modal', async ({ page, request }) => {
    const userEmail = `multisync_${Date.now()}@bingesync.local`;

    // 1. Sign in
    await page.goto('/');
    await page.fill('#dev-email', userEmail);
    await page.fill('#dev-name', 'MultiSync User');
    await page.click('#btn-dev-submit');
    await expect(page.locator('#user-display-name')).toHaveText('MultiSync User');

    // 2. Retrieve webhook URL and token from settings
    await page.click('#tab-settings');
    const webhookUrl = await page.locator('#webhook-url-input').inputValue();
    const webhookPath = new URL(webhookUrl).pathname;

    // Configure allowed usernames for multi-user sync: "sarah, daryl"
    await page.click('#pill-settings-services');
    await page.fill('#user-setting-plex-username', 'sarah, daryl');
    await page.fill('#user-setting-jellyfin-username', 'sarah, daryl');
    await page.click('#btn-save-user-settings');
    await expect(page.locator('#user-settings-status')).toContainText('Settings saved successfully');

    // 3. Post a Plex webhook for user "sarah" (Interstellar)
    const plexPayload = {
      event: 'media.scrobble',
      user: true,
      owner: false,
      Account: {
        id: 1234,
        title: 'sarah',
      },
      Metadata: {
        librarySectionType: 'movie',
        type: 'movie',
        title: 'Interstellar',
        year: 2014,
        duration: 10140000,
        viewOffset: 10140000,
      },
    };

    const plexResp = await request.post(webhookPath, {
      data: plexPayload,
      headers: { 'Content-Type': 'application/json' },
    });
    expect(plexResp.status()).toBe(201);

    // 4. Post a Jellyfin webhook for user "daryl" (Arrival)
    const jellyfinPath = webhookPath.replace('/plex/', '/jellyfin/');
    const jellyfinPayload = {
      NotificationType: 'PlaybackStop',
      PlayedToCompletion: true,
      ItemType: 'Movie',
      Name: 'Arrival',
      Year: 2016,
      RunTimeTicks: 69600000000,
      PlaybackPositionTicks: 69600000000,
      NotificationUsername: 'daryl',
    };

    const jellyfinResp = await request.post(jellyfinPath, {
      data: jellyfinPayload,
      headers: { 'Content-Type': 'application/json' },
    });
    expect(jellyfinResp.status()).toBe(201);

    // 5. Navigate to Timeline and verify badges
    await page.click('#tab-timeline');
    await page.click('#btn-refresh-timeline');

    // Verify Interstellar has Plex source tag and @sarah user tag
    const interstellarCard = page.locator('#timeline-container .scrobble-card', { hasText: 'Interstellar' });
    await expect(interstellarCard).toBeVisible();
    await expect(interstellarCard.locator('.source-tag-card')).toContainText('PLEX');
    await expect(interstellarCard.locator('.user-tag-card')).toContainText('sarah');

    // Verify Arrival has Jellyfin source tag and @daryl user tag
    const arrivalCard = page.locator('#timeline-container .scrobble-card', { hasText: 'Arrival' });
    await expect(arrivalCard).toBeVisible();
    await expect(arrivalCard.locator('.source-tag-card')).toContainText('JELLYFIN');
    await expect(arrivalCard.locator('.user-tag-card')).toContainText('daryl');

    // 6. Click Interstellar to inspect Detail Modal
    await interstellarCard.click();
    await expect(page.locator('#modal-item-detail')).toBeVisible();
    await expect(page.locator('#detail-source-display')).toContainText('plex');
    await expect(page.locator('#detail-user-display')).toContainText('sarah');
    await page.click('#btn-close-detail-modal');
    await expect(page.locator('#modal-item-detail')).toBeHidden();

    // 7. Navigate to History View and test filters
    await page.click('#tab-history');
    await expect(page.locator('#history-list')).toContainText('Interstellar');
    await expect(page.locator('#history-list')).toContainText('Arrival');

    // Filter by Source = Plex
    await page.selectOption('#history-source-filter', 'plex');
    await expect(page.locator('#history-list')).toContainText('Interstellar');
    await expect(page.locator('#history-list')).not.toContainText('Arrival');

    // Filter by Source = Jellyfin
    await page.selectOption('#history-source-filter', 'jellyfin');
    await expect(page.locator('#history-list')).not.toContainText('Interstellar');
    await expect(page.locator('#history-list')).toContainText('Arrival');

    // Reset Source to All
    await page.selectOption('#history-source-filter', 'all');
    await expect(page.locator('#history-list')).toContainText('Interstellar');
    await expect(page.locator('#history-list')).toContainText('Arrival');

    // Filter by Watched By user input = sarah
    await page.fill('#history-user-filter', 'sarah');
    await page.keyboard.press('Enter');
    await expect(page.locator('#history-list')).toContainText('Interstellar');
    await expect(page.locator('#history-list')).not.toContainText('Arrival');

    // Clear user filter
    await page.click('#btn-clear-history-user');
    await expect(page.locator('#history-list')).toContainText('Interstellar');
    await expect(page.locator('#history-list')).toContainText('Arrival');

    // 8. Click the user tag on Arrival card directly to filter
    const arrivalHistoryCard = page.locator('#history-list .scrobble-card', { hasText: 'Arrival' });
    await arrivalHistoryCard.locator('.user-tag-card').click();
    await expect(page.locator('#history-list')).toContainText('Arrival');
    await expect(page.locator('#history-list')).not.toContainText('Interstellar');
    await expect(page.locator('#history-user-badge')).toBeVisible();
    await expect(page.locator('#history-user-badge')).toContainText('daryl');
  });
});




