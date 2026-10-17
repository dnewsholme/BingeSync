import { test, expect } from '@playwright/test';

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
    await expect(page.locator('#modal-import-trakt')).toBeVisible();

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
    await expect(page.locator('#modal-import-trakt')).toBeHidden();

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
});

