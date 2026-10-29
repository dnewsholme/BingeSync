import { api, API_BASE } from './api.js';
import { initI18n, setLanguage, getLanguage, t, getMonthName, getMonthShortName } from './i18n.js';

export const MONTH_NAMES = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December'
];
export const MONTH_SHORT_NAMES = [
  'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
  'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'
];

export function updateMonthNames() {
  for (let i = 0; i < 12; i++) {
    MONTH_NAMES[i] = getMonthName(i);
    MONTH_SHORT_NAMES[i] = getMonthShortName(i);
  }
}

// Application State
const state = {
  user: null,
  authStatus: null,
  activeTab: 'view-timeline',
  timeline: {
    level: 'month', // 'years' | 'year' | 'month'
    selectedYear: String(new Date().getFullYear()),
    selectedMonth: String(new Date().getMonth() + 1).padStart(2, '0'),
    userManuallySelected: false,
    periodsData: null,
    limit: 60,
    offset: 0,
    search: '',
    searchAllDates: false,
    date: '',
    genre: '',
    person: '',
    rating: 'all',
    watch_status: 'all',
    days: [],
    hasMore: false,
    missingMetadata: false,
  },
  admin: {
    activeSubview: 'admin-sub-users',
  },
  history: {
    page: 1,
    limit: 12,
    mediaType: 'all',
    rating: 'all',
    search: '',
    person: '',
    tag: '',
    allResults: false,
    missingMetadata: false,
    totalPages: 1,
    selectMode: false,
    selectedIds: new Set(),
  },
  stats: {
    filter: 'all',
    timeRange: 'all',
    data: null,
  },
  cleanup: {
    items: [],
    filter: 'all',
    search: '',
    sort: 'recent',
    selectedItem: null,
    isLoading: false,
  },
  duplicates: {
    groups: [],
    selectedDeleteIds: new Set(),
    windowHours: 24,
    isLoading: false,
  },
  selectedItem: null,
  nowPlayingTimer: null,
};

// Helper: IMDb URL generator
function getImdbUrl(item) {
  if (item.guid) {
    const match = item.guid.match(/(?:imdb:\/\/)?(tt\d+)/i);
    if (match) {
      return `https://www.imdb.com/title/${match[1]}/`;
    }
  }
  const query = item.media_type === 'episode' && item.grandparent_title
    ? `${item.grandparent_title} ${item.title}`
    : `${item.title} ${item.year || ''}`;
  return `https://www.imdb.com/find/?q=${encodeURIComponent(query.trim())}`;
}

// Helper: TheTVDB URL generator
function getTvdbUrl(item) {
  if (item.tvdb_id) {
    const cleanId = String(item.tvdb_id).replace(/^(series-|movie-)/i, '').trim();
    if (cleanId) {
      if (item.media_type === 'movie') {
        return `https://thetvdb.com/dereferrer/movie/${cleanId}`;
      } else {
        return `https://thetvdb.com/dereferrer/series/${cleanId}`;
      }
    }
  }
  if (item.guid) {
    const match = item.guid.match(/(?:the)?tvdb:\/\/(?:series\/|movie\/)?(\d+)/i);
    if (match) {
      if (item.media_type === 'movie') {
        return `https://thetvdb.com/dereferrer/movie/${match[1]}`;
      } else {
        return `https://thetvdb.com/dereferrer/series/${match[1]}`;
      }
    }
  }
  const query = item.media_type === 'episode' && item.grandparent_title
    ? item.grandparent_title
    : (item.title || '');
  return `https://thetvdb.com/search?query=${encodeURIComponent(query.trim())}`;
}

// Helper: TMDB URL generator
function getTmdbUrl(item) {
  if (item.guid) {
    const match = item.guid.match(/tmdb:\/\/(\d+)/i);
    if (match) {
      return `https://www.themoviedb.org/${item.media_type === 'movie' ? 'movie' : 'tv'}/${match[1]}`;
    }
  }
  const query = item.media_type === 'episode' && item.grandparent_title
    ? item.grandparent_title
    : (item.title || '');
  return `https://www.themoviedb.org/search?query=${encodeURIComponent(query.trim())}`;
}

// Helper: Resolve image URL (resolves relative cached image paths to active API_BASE)
function resolveImageUrl(url) {
  if (!url) return null;
  if (url.startsWith('/library/') || url.startsWith('/photo/')) {
    return null;
  }
  if (url.startsWith('http://') || url.startsWith('https://') || url.startsWith('data:')) {
    return url;
  }
  const cleanUrl = url.startsWith('/') ? url : `/${url}`;
  if (cleanUrl.startsWith('/api/v1')) {
    return cleanUrl.replace('/api/v1', API_BASE);
  }
  return `${API_BASE}${cleanUrl}`;
}

// DOM Elements
const el = {
  // Navigation
  navbar: document.getElementById('navbar'),
  navLinks: document.getElementById('nav-links'),
  navActions: document.getElementById('nav-actions'),
  userBadge: document.getElementById('user-badge'),
  userAvatar: document.getElementById('user-avatar'),
  userDisplayName: document.getElementById('user-display-name'),
  btnLogout: document.getElementById('btn-logout'),
  navTabs: document.querySelectorAll('.nav-tab'),
  appViews: document.querySelectorAll('.app-view'),
  btnOpenLogModal: document.getElementById('btn-open-log-modal'),

  // States
  stateLoading: document.getElementById('state-loading'),
  stateAuth: document.getElementById('state-auth'),
  stateApp: document.getElementById('state-app'),

  // Auth Card
  btnOidcLogin: document.getElementById('btn-oidc-login'),
  authDivider: document.getElementById('auth-divider'),
  formDevLogin: document.getElementById('form-dev-login'),
  devEmail: document.getElementById('dev-email'),
  devName: document.getElementById('dev-name'),

  // Now Playing
  nowPlayingBanner: document.getElementById('now-playing-banner'),
  nowPlayingTitle: document.getElementById('now-playing-title'),
  nowPlayingDevice: document.getElementById('now-playing-device'),
  nowPlayingProgress: document.getElementById('now-playing-progress'),
  nowPlayingState: document.getElementById('now-playing-state'),

  // Timeline View
  timelineContainer: document.getElementById('timeline-container'),
  timelineHierarchyNav: document.getElementById('timeline-hierarchy-nav'),
  hierarchyBreadcrumbs: document.getElementById('hierarchy-breadcrumbs'),
  hierarchyControls: document.getElementById('hierarchy-controls'),
  timelineMonthChips: document.getElementById('timeline-month-chips'),
  btnRefreshTimeline: document.getElementById('btn-refresh-timeline'),
  timelineFilterBar: document.getElementById('timeline-filter-bar'),
  timelineFiltersGrid: document.getElementById('timeline-filters-grid'),
  timelineSearchWrap: document.getElementById('timeline-search-wrap'),
  timelineSearch: document.getElementById('timeline-search'),
  timelineSearchAllLabel: document.getElementById('timeline-search-all-label'),
  timelineSearchAllDates: document.getElementById('timeline-search-all-dates'),
  timelineDateFilter: document.getElementById('timeline-date-filter'),
  timelineGenreFilter: document.getElementById('timeline-genre-filter'),
  timelinePersonFilter: document.getElementById('timeline-person-filter'),
  timelineRatingFilter: document.getElementById('timeline-rating-filter'),
  timelineStatusFilter: document.getElementById('timeline-status-filter'),
  btnTimelineMissingMetadata: document.getElementById('btn-timeline-missing-metadata'),
  btnResetTimelineFilters: document.getElementById('btn-reset-timeline-filters'),
  timelineScrubber: document.getElementById('timeline-scrubber'),
  timelineScrubberList: document.getElementById('timeline-scrubber-list'),
  timelineLoadMoreWrap: document.getElementById('timeline-load-more-wrap'),
  btnTimelineLoadMore: document.getElementById('btn-timeline-load-more'),
  timelinePaginationInfo: document.getElementById('timeline-pagination-info'),

  // History View
  historySearch: document.getElementById('history-search'),
  historyRatingFilter: document.getElementById('history-rating-filter'),
  historyPersonBadge: document.getElementById('history-person-badge'),
  historySearchAllLabel: document.getElementById('history-search-all-label'),
  historySearchAllResults: document.getElementById('history-search-all-results'),
  historyList: document.getElementById('history-list'),
  historyPagination: document.getElementById('history-pagination'),
  paginationInfo: document.getElementById('pagination-info'),
  btnPagePrev: document.getElementById('btn-page-prev'),
  btnPageNext: document.getElementById('btn-page-next'),
  pills: document.querySelectorAll('#history-filter-pills .pill[data-type]'),
  pillMissingMetadata: document.getElementById('pill-missing-metadata'),
  btnToggleSelectMode: document.getElementById('btn-toggle-select-mode'),
  historyBulkBar: document.getElementById('history-bulk-bar'),
  bulkSelectAllCheckbox: document.getElementById('bulk-select-all-checkbox'),
  bulkSelectedCount: document.getElementById('bulk-selected-count'),
  btnDeleteSelected: document.getElementById('btn-delete-selected'),
  btnCancelSelectMode: document.getElementById('btn-cancel-select-mode'),

  // Stats View
  statScrobbles: document.getElementById('stat-scrobbles'),
  statScrobblesSub: document.getElementById('stat-scrobbles-sub'),
  statHours: document.getElementById('stat-hours'),
  statMinutes: document.getElementById('stat-minutes'),
  statMovies: document.getElementById('stat-movies'),
  statEpisodes: document.getElementById('stat-episodes'),
  statMovieTime: document.getElementById('stat-movie-time'),
  statEpisodeTime: document.getElementById('stat-episode-time'),
  statUniqueMovies: document.getElementById('stat-unique-movies'),
  statUniqueShows: document.getElementById('stat-unique-shows'),
  statStreak: document.getElementById('stat-streak'),
  statStreakSub: document.getElementById('stat-streak-sub'),
  statAvgDaily: document.getElementById('stat-avg-daily'),
  statAvgRating: document.getElementById('stat-avg-rating'),
  statTotalRated: document.getElementById('stat-total-rated'),
  statCompletionRate: document.getElementById('stat-completion-rate'),
  statPeakDay: document.getElementById('stat-peak-day'),
  statPeakCount: document.getElementById('stat-peak-count'),
  topShowsList: document.getElementById('top-shows-list'),
  topShowsSection: document.getElementById('top-shows-section'),
  topMoviesList: document.getElementById('top-movies-list'),
  topMoviesSection: document.getElementById('top-movies-section'),
  statsFilterPills: document.querySelectorAll('#stats-filter-pills .pill'),
  statsRangePills: document.querySelectorAll('#stats-range-pills .pill'),
  activityChart: document.getElementById('activity-chart'),
  activityChartTitle: document.getElementById('activity-chart-title'),
  activityChartSubtitle: document.getElementById('activity-chart-subtitle'),
  activityChartLegend: document.getElementById('activity-chart-legend'),
  genresChart: document.getElementById('genres-chart'),
  genresChartTitle: document.getElementById('genres-chart-title'),
  genresChartSubtitle: document.getElementById('genres-chart-subtitle'),
  dayOfWeekChart: document.getElementById('day-of-week-chart'),
  timeOfDayChart: document.getElementById('time-of-day-chart'),
  topActorsList: document.getElementById('top-actors-list'),
  topDirectorsList: document.getElementById('top-directors-list'),

  // Webhook View
  webhookUrlInput: document.getElementById('webhook-url-input'),
  btnCopyWebhook: document.getElementById('btn-copy-webhook'),
  copyStatus: document.getElementById('copy-status'),
  jellyfinWebhookUrlInput: document.getElementById('jellyfin-webhook-url-input'),
  btnCopyJellyfinWebhook: document.getElementById('btn-copy-jellyfin-webhook'),
  jellyfinCopyStatus: document.getElementById('jellyfin-copy-status'),
  embyWebhookUrlInput: document.getElementById('emby-webhook-url-input'),
  btnCopyEmbyWebhook: document.getElementById('btn-copy-emby-webhook'),
  embyCopyStatus: document.getElementById('emby-copy-status'),
  btnRegenToken: document.getElementById('btn-regen-token'),

  // Modals
  modalLogScrobble: document.getElementById('modal-log-scrobble'),
  btnCloseLogModal: document.getElementById('btn-close-log-modal'),
  btnCancelLogModal: document.getElementById('btn-cancel-log-modal'),
  formManualScrobble: document.getElementById('form-manual-scrobble'),
  scrobbleMediaType: document.getElementById('scrobble-media-type'),
  groupGrandparent: document.getElementById('group-grandparent'),
  groupEpisodeNums: document.getElementById('group-episode-nums'),
  labelScrobbleTitle: document.getElementById('label-scrobble-title'),
  scrobbleRating: document.getElementById('scrobble-rating'),

  btnOpenImportModal: document.getElementById('btn-open-import-modal'),
  btnOpenTautulliModal: document.getElementById('btn-open-tautulli-modal'),
  modalImportHistory: document.getElementById('modal-import-history'),
  btnImportTabTrakt: document.getElementById('btn-import-tab-trakt'),
  btnImportTabTautulli: document.getElementById('btn-import-tab-tautulli'),
  importPaneTrakt: document.getElementById('import-pane-trakt'),
  importPaneTautulli: document.getElementById('import-pane-tautulli'),
  btnCloseImportModal: document.getElementById('btn-close-import-modal'),
  btnCancelImportModal: document.getElementById('btn-cancel-import-modal'),
  formImportTrakt: document.getElementById('form-import-trakt'),
  importFileInput: document.getElementById('import-file-input'),
  importPasteInput: document.getElementById('import-paste-input'),
  importStatusBox: document.getElementById('import-status-box'),

  // Tautulli Modal components
  btnCancelTautulliModal: document.getElementById('btn-cancel-tautulli-modal'),
  formImportTautulli: document.getElementById('form-import-tautulli'),
  tautulliImportUsername: document.getElementById('tautulli-import-username'),
  btnFetchTautulliUsers: document.getElementById('btn-fetch-tautulli-users'),
  tautulliUsersSelect: document.getElementById('tautulli-users-select'),
  tautulliFileInput: document.getElementById('tautulli-file-input'),
  tautulliPasteInput: document.getElementById('tautulli-paste-input'),
  tautulliImportStatus: document.getElementById('tautulli-import-status'),
  btnSubmitTautulliImport: document.getElementById('btn-submit-tautulli-import'),
  linkGotoUserSettings: document.getElementById('link-goto-user-settings'),

  // User Settings & Themes
  formUserSettings: document.getElementById('form-user-settings'),
  btnSaveUserSettings: document.getElementById('btn-save-user-settings'),
  userSettingsStatus: document.getElementById('user-settings-status'),
  userSettingLanguage: document.getElementById('user-setting-language'),
  userSettingTheme: document.getElementById('user-setting-theme'),
  navLanguageSelect: document.getElementById('nav-language-select'),
  userSettingPlexUsername: document.getElementById('user-setting-plex-username'),
  userSettingJellyfinUsername: document.getElementById('user-setting-jellyfin-username'),
  userSettingEmbyUsername: document.getElementById('user-setting-emby-username'),
  userSettingJellyfinUrl: document.getElementById('user-setting-jellyfin-url'),
  userSettingJellyfinApiKey: document.getElementById('user-setting-jellyfin-api-key'),
  userSettingRadarrUrl: document.getElementById('user-setting-radarr-url'),
  userSettingRadarrApiKey: document.getElementById('user-setting-radarr-api-key'),
  userSettingSonarrUrl: document.getElementById('user-setting-sonarr-url'),
  userSettingSonarrApiKey: document.getElementById('user-setting-sonarr-api-key'),
  userSettingTautulliUrl: document.getElementById('user-setting-tautulli-url'),
  userSettingTautulliApiKey: document.getElementById('user-setting-tautulli-api-key'),
  userSettingOverseerrUrl: document.getElementById('user-setting-overseerr-url'),
  userSettingOverseerrApiKey: document.getElementById('user-setting-overseerr-api-key'),
  userSettingOutboundType: document.getElementById('user-setting-outbound-type'),
  userSettingOutboundUrl: document.getElementById('user-setting-outbound-url'),
  btnTestOutboundWebhook: document.getElementById('btn-test-outbound-webhook'),
  outboundWebhookTestStatus: document.getElementById('outbound-webhook-test-status'),
  btnExportUserData: document.getElementById('btn-export-user-data'),
  exportUserDataStatus: document.getElementById('export-user-data-status'),

  // Recommendations & Heatmap & Wrapped
  weeklyHeatmapGrid: document.getElementById('weekly-heatmap-grid'),
  btnOpenWrapped: document.getElementById('btn-open-wrapped'),
  modalWrapped: document.getElementById('modal-wrapped'),
  btnCloseWrappedModal: document.getElementById('btn-close-wrapped-modal'),
  btnDoneWrappedModal: document.getElementById('btn-done-wrapped-modal'),
  wrappedYearSelect: document.getElementById('wrapped-year-select'),
  wrappedYearBadge: document.getElementById('wrapped-year-badge'),
  wrappedValHours: document.getElementById('wrapped-val-hours'),
  wrappedValScrobbles: document.getElementById('wrapped-val-scrobbles'),
  wrappedValMovies: document.getElementById('wrapped-val-movies'),
  wrappedValEpisodes: document.getElementById('wrapped-val-episodes'),
  wrappedLongestMovieCard: document.getElementById('wrapped-longest-movie-card'),
  wrappedLongestMovieTitle: document.getElementById('wrapped-longest-movie-title'),
  wrappedLongestMovieDur: document.getElementById('wrapped-longest-movie-dur'),
  wrappedTopShowsList: document.getElementById('wrapped-top-shows-list'),
  wrappedTopMoviesList: document.getElementById('wrapped-top-movies-list'),
  btnCopyWrappedSummary: document.getElementById('btn-copy-wrapped-summary'),

  modalItemDetail: document.getElementById('modal-item-detail'),
  btnCloseDetailModal: document.getElementById('btn-close-detail-modal'),
  btnDismissDetailModal: document.getElementById('btn-dismiss-detail-modal'),
  btnDeleteDetailItem: document.getElementById('btn-delete-detail-item'),
  btnRefreshDetailMetadata: document.getElementById('btn-refresh-detail-metadata'),
  btnFixMatch: document.getElementById('btn-fix-match'),
  detailTitle: document.getElementById('detail-title'),
  detailBody: document.getElementById('detail-body'),
  detailImdbLink: document.getElementById('detail-imdb-link'),
  detailTvdbLink: document.getElementById('detail-tvdb-link'),
  detailTmdbLink: document.getElementById('detail-tmdb-link'),
  btnDeleteShowScrobbles: document.getElementById('btn-delete-show-scrobbles'),

  // Admin Management & Subviews
  tabAdmin: document.getElementById('tab-admin'),
  adminSubnavPills: document.getElementById('admin-subnav-pills'),
  adminUsersTbody: document.getElementById('admin-users-tbody'),
  btnRefreshAdminUsers: document.getElementById('btn-refresh-admin-users'),
  adminBackchannelUrlInput: document.getElementById('admin-backchannel-url-input'),
  btnCopyBackchannelUrl: document.getElementById('btn-copy-backchannel-url'),

  // Admin Settings
  formAdminSettings: document.getElementById('form-admin-settings'),
  btnSaveAdminSettings: document.getElementById('btn-save-admin-settings'),
  adminSettingsStatus: document.getElementById('admin-settings-status'),
  settingMetadataSource: document.getElementById('setting-metadata-source'),
  settingTvdbApiKey: document.getElementById('setting-tvdb-api-key'),
  settingTvdbPin: document.getElementById('setting-tvdb-pin'),
  settingTvdbBaseUrl: document.getElementById('setting-tvdb-base-url'),
  settingTmdbApiKey: document.getElementById('setting-tmdb-api-key'),
  settingTmdbBaseUrl: document.getElementById('setting-tmdb-base-url'),
  adminTvdbGroup: document.getElementById('admin-tvdb-settings-group'),
  adminTmdbGroup: document.getElementById('admin-tmdb-settings-group'),
  settingOidcEnabled: document.getElementById('setting-oidc-enabled'),
  settingOidcIssuerUrl: document.getElementById('setting-oidc-issuer-url'),
  settingOidcClientId: document.getElementById('setting-oidc-client-id'),
  settingOidcClientSecret: document.getElementById('setting-oidc-client-secret'),
  settingOidcRedirectUrl: document.getElementById('setting-oidc-redirect-url'),
  settingOidcAdminGroup: document.getElementById('setting-oidc-admin-group'),
  settingScrobbleThreshold: document.getElementById('setting-scrobble-threshold'),

  // Admin Background Tasks
  btnRefreshAdminTasks: document.getElementById('btn-refresh-admin-tasks'),
  btnForceRefreshAllMetadata: document.getElementById('btn-force-refresh-all-metadata'),
  adminTasksContainer: document.getElementById('admin-tasks-container'),

  // Admin Application Logs
  btnRefreshAdminLogs: document.getElementById('btn-refresh-admin-logs'),
  btnClearAdminLogs: document.getElementById('btn-clear-admin-logs'),
  adminLogsViewer: document.getElementById('admin-logs-viewer'),
  adminLogsFilterLevel: document.getElementById('admin-logs-filter-level'),
  adminLogsAutorefresh: document.getElementById('admin-logs-autorefresh'),

  // Admin Database Backups & Restore
  pillAdminBackups: document.getElementById('pill-admin-backups'),
  adminBackupsPath: document.getElementById('admin-backups-path'),
  adminBackupsTable: document.getElementById('admin-backups-table'),
  adminBackupsTbody: document.getElementById('admin-backups-tbody'),
  adminBackupsStatus: document.getElementById('admin-backups-status'),
  btnAdminCreateBackup: document.getElementById('btn-admin-create-backup'),
  btnRefreshAdminBackups: document.getElementById('btn-refresh-admin-backups'),

  // Restore Modal
  modalRestoreBackup: document.getElementById('modal-restore-backup'),
  restoreTargetFilename: document.getElementById('restore-target-filename'),
  restoreModalStatus: document.getElementById('restore-modal-status'),
  btnCloseRestoreModal: document.getElementById('btn-close-restore-modal'),
  btnCancelRestoreModal: document.getElementById('btn-cancel-restore-modal'),
  btnConfirmRestoreBackup: document.getElementById('btn-confirm-restore-backup'),

  // Library Cleanup (Radarr / Sonarr)
  tabCleanup: document.getElementById('tab-cleanup'),
  viewCleanup: document.getElementById('view-cleanup'),
  btnRefreshCleanup: document.getElementById('btn-refresh-cleanup'),
  cleanupTotalSize: document.getElementById('cleanup-total-size'),
  cleanupTotalCount: document.getElementById('cleanup-total-count'),
  cleanupRadarrCount: document.getElementById('cleanup-radarr-count'),
  cleanupRadarrSize: document.getElementById('cleanup-radarr-size'),
  cleanupSonarrCount: document.getElementById('cleanup-sonarr-count'),
  cleanupSonarrSize: document.getElementById('cleanup-sonarr-size'),
  cleanupSearchInput: document.getElementById('cleanup-search-input'),
  cleanupSortSelect: document.getElementById('cleanup-sort-select'),
  cleanupAlertBanner: document.getElementById('cleanup-alert-banner'),
  cleanupItemsGrid: document.getElementById('cleanup-items-grid'),
  modalCleanupAction: document.getElementById('modal-cleanup-action'),
  btnCloseCleanupModal: document.getElementById('btn-close-cleanup-modal'),
  btnCancelCleanupModal: document.getElementById('btn-cancel-cleanup-modal'),
  cleanupModalTitle: document.getElementById('cleanup-modal-title'),
  cleanupModalSubtitle: document.getElementById('cleanup-modal-subtitle'),
  cleanupModalPoster: document.getElementById('cleanup-modal-poster'),
  cleanupModalItemTitle: document.getElementById('cleanup-modal-item-title'),
  cleanupModalServiceBadge: document.getElementById('cleanup-modal-service-badge'),
  cleanupModalSize: document.getElementById('cleanup-modal-size'),
  cleanupModalWatched: document.getElementById('cleanup-modal-watched'),
  cleanupModalExtraInfo: document.getElementById('cleanup-modal-extra-info'),
  cleanupModalOptions: document.getElementById('cleanup-modal-options'),
  cleanupModalStatus: document.getElementById('cleanup-modal-status'),
  cleanupAlsoRemoveHistory: document.getElementById('cleanup-also-remove-history'),

  // Duplicate Watch Detector
  btnOpenDuplicatesModal: document.getElementById('btn-open-duplicates-modal'),
  btnCleanupFindDuplicates: document.getElementById('btn-cleanup-find-duplicates'),
  modalDuplicates: document.getElementById('modal-duplicates'),
  btnCloseDuplicatesModal: document.getElementById('btn-close-duplicates-modal'),
  btnCloseDuplicatesModalFooter: document.getElementById('btn-close-duplicates-modal-footer'),
  dupWindowSelect: document.getElementById('dup-window-select'),
  btnScanDuplicates: document.getElementById('btn-scan-duplicates'),
  dupBulkActions: document.getElementById('dup-bulk-actions'),
  btnCleanKeepOldest: document.getElementById('btn-clean-keep-oldest'),
  btnCleanKeepNewest: document.getElementById('btn-clean-keep-newest'),
  dupStatusBanner: document.getElementById('dup-status-banner'),
  dupResultsContainer: document.getElementById('dup-results-container'),
  dupLoading: document.getElementById('dup-loading'),
  dupEmptyState: document.getElementById('dup-empty-state'),
  dupGroupsList: document.getElementById('dup-groups-list'),
  dupSelectionSummary: document.getElementById('dup-selection-summary'),
  btnDeleteSelectedDuplicates: document.getElementById('btn-delete-selected-duplicates'),
  dupSelectedCount: document.getElementById('dup-selected-count'),
};

// Initialization
async function init() {
  initI18n();
  initTheme();
  initServiceWorker();
  updateMonthNames();

  document.querySelectorAll('.language-select').forEach((sel) => {
    sel.addEventListener('change', (e) => {
      setLanguage(e.target.value);
    });
  });

  window.addEventListener('languagechange', () => {
    updateMonthNames();
    if (state.activeTab === 'view-timeline') {
      renderTimelineHierarchyNav();
      if (state.timeline.level === 'years') {
        renderTimelineYearsView();
        renderTimelineScrubberYears();
      } else if (state.timeline.level === 'year') {
        renderTimelineMonthsView();
        renderTimelineScrubberMonths();
      } else {
        renderTimeline(state.timeline.days);
        renderTimelineScrubber(state.timeline.days);
        renderTimelinePagination();
      }
    } else if (state.activeTab === 'view-history') {
      if (state.history.currentItems) {
        renderHistory({
          items: state.history.currentItems,
          total: state.history.total || state.history.currentItems.length,
          page: state.history.page,
          total_pages: state.history.totalPages,
        });
      }
    } else if (state.activeTab === 'view-stats') {
      if (state.stats.data) {
        renderStats();
      }
    }
  });

  try {
    // 1. Fetch auth status
    state.authStatus = await api.getAuthStatus();

    if (state.authStatus.oidc_enabled) {
      el.btnOidcLogin.href = `${API_BASE}/auth/login`;
      el.btnOidcLogin.style.display = 'inline-flex';
    }
    if (state.authStatus.oidc_enabled && state.authStatus.dev_login_enabled) {
      el.authDivider.style.display = 'flex';
    }
    if (!state.authStatus.dev_login_enabled) {
      el.formDevLogin.style.display = 'none';
    }

    // 2. Check if currently logged in via /auth/me
    try {
      const me = await api.getMe();
      if (me && me.id) {
        onUserAuthenticated(me);
        return;
      }
    } catch (e) {
      // Unauthenticated
    }

    showAuthState();
  } catch (err) {
    console.error('Initialization error:', err);
    showAuthState();
  }
}

function showAuthState() {
  el.stateLoading.style.display = 'none';
  el.stateApp.style.display = 'none';
  el.stateAuth.style.display = 'flex';
  el.navLinks.style.display = 'none';
  el.navActions.style.display = 'none';
}

function onUserAuthenticated(user) {
  state.user = user;
  el.stateLoading.style.display = 'none';
  el.stateAuth.style.display = 'none';
  el.stateApp.style.display = 'block';

  el.navLinks.style.display = 'flex';
  el.navActions.style.display = 'flex';

  el.userDisplayName.textContent = user.display_name;
  el.userAvatar.textContent = (user.display_name || user.email || 'U').charAt(0).toUpperCase();

  // Populate webhook inputs
  if (user.webhook_url) {
    el.webhookUrlInput.value = user.webhook_url;
  }
  if (el.jellyfinWebhookUrlInput) {
    el.jellyfinWebhookUrlInput.value = user.jellyfin_webhook_url || (user.webhook_url ? user.webhook_url.replace('/webhooks/plex/', '/webhooks/jellyfin/') : '');
  }
  if (el.embyWebhookUrlInput) {
    el.embyWebhookUrlInput.value = user.emby_webhook_url || (user.webhook_url ? user.webhook_url.replace('/webhooks/plex/', '/webhooks/emby/') : '');
  }

  // Show Admin tab if user has admin role
  if (user.role === 'admin') {
    if (el.tabAdmin) el.tabAdmin.style.display = 'inline-flex';
  } else {
    if (el.tabAdmin) el.tabAdmin.style.display = 'none';
  }

  checkOverseerrStatus();

  // Load Initial Data
  loadTimeline();
  loadStats();
  loadNowPlaying();

  // Start now playing interval (every 10s)
  if (state.nowPlayingTimer) clearInterval(state.nowPlayingTimer);
  state.nowPlayingTimer = setInterval(loadNowPlaying, 10000);
}

// Navigation & Tab Switching
function switchTab(targetViewId) {
  state.activeTab = targetViewId;

  // Update querySelector for dynamically toggled tabs
  const navTabs = document.querySelectorAll('.nav-tab');
  const appViews = document.querySelectorAll('.app-view');

  navTabs.forEach(tab => {
    if (tab.dataset.target === targetViewId) {
      tab.classList.add('active');
    } else {
      tab.classList.remove('active');
    }
  });

  appViews.forEach(view => {
    if (view.id === targetViewId) {
      view.style.display = 'block';
    } else {
      view.style.display = 'none';
    }
  });

  if (targetViewId === 'view-timeline') loadTimeline();
  if (targetViewId === 'view-history') loadHistory();
  if (targetViewId === 'view-stats') loadStats();
  if (targetViewId === 'view-cleanup') loadCleanupItems();
  if (targetViewId === 'view-settings') {
    loadWebhookInfo();
    loadUserSettings();
  }
  if (targetViewId === 'view-admin') {
    switchAdminSubview(state.admin.activeSubview || 'admin-sub-users');
  } else {
    stopLogsAutoRefresh();
  }
}

el.navTabs.forEach(tab => {
  tab.addEventListener('click', () => switchTab(tab.dataset.target));
});

// View 1: Timeline
async function loadTimeline(append = false) {
  try {
    if (!append) {
      state.timeline.offset = 0;
      state.timeline.days = [];

      try {
        const periodsData = await api.getTimelinePeriods();
        state.timeline.periodsData = periodsData;

        // If user hasn't manually selected a period, default to latest active month
        if (!state.timeline.userManuallySelected && periodsData) {
          const currentYm = `${state.timeline.selectedYear}-${state.timeline.selectedMonth}`;
          const currentYearObj = periodsData.years?.find(y => y.year === state.timeline.selectedYear);
          const currentMonthHasData = currentYearObj?.months?.some(m => m.month === currentYm);
          if (!currentMonthHasData && periodsData.latest_year && periodsData.latest_month) {
            state.timeline.selectedYear = periodsData.latest_year;
            state.timeline.selectedMonth = periodsData.latest_month.slice(5, 7);
          }
        }
      } catch (e) {
        console.warn('Failed to load timeline periods:', e);
      }
    }

    renderTimelineHierarchyNav();

    let dateParam = '';
    if (!state.timeline.searchAllDates) {
      if (state.timeline.level === 'years') {
        renderTimelineYearsView();
        renderTimelineScrubberYears();
        if (el.timelineLoadMoreWrap) el.timelineLoadMoreWrap.style.display = 'none';
        return;
      }

      if (state.timeline.level === 'year') {
        renderTimelineMonthsView();
        renderTimelineScrubberMonths();
        if (el.timelineLoadMoreWrap) el.timelineLoadMoreWrap.style.display = 'none';
        return;
      }

      // Level === 'month': "the time line ui should only show dates for the currently selected month"
      const monthFilter = `${state.timeline.selectedYear}-${state.timeline.selectedMonth}`;
      dateParam = state.timeline.date || monthFilter;
    } else {
      dateParam = state.timeline.date || '';
    }

    const days = await api.getTimeline({
      limit: state.timeline.limit,
      offset: state.timeline.offset,
      search: state.timeline.search,
      date: dateParam,
      all: state.timeline.searchAllDates,
      all_dates: state.timeline.searchAllDates,
      genre: state.timeline.genre,
      person: state.timeline.person,
      rating: state.timeline.rating,
      watch_status: state.timeline.watch_status,
      missing_metadata: state.timeline.missingMetadata,
    });

    if (append) {
      if (days && days.length > 0) {
        days.forEach(newDay => {
          const existingDay = state.timeline.days.find(d => d.date === newDay.date);
          if (existingDay) {
            existingDay.items.push(...newDay.items);
            existingDay.total_count += newDay.total_count;
            existingDay.total_minutes += newDay.total_minutes;
          } else {
            state.timeline.days.push(newDay);
          }
        });
        state.timeline.hasMore = state.timeline.searchAllDates ? false : (days.length >= state.timeline.limit);
      } else {
        state.timeline.hasMore = false;
      }
    } else {
      state.timeline.days = days || [];
      state.timeline.hasMore = state.timeline.searchAllDates ? false : Boolean(days && days.length >= state.timeline.limit);
    }

    renderTimeline(state.timeline.days);
    renderTimelineScrubber(state.timeline.days);
    renderTimelinePagination();
  } catch (err) {
    console.error('Error loading timeline:', err);
    if (!append) {
      el.timelineContainer.innerHTML = `<div class="empty-state"><p>Failed to load timeline. Please try again.</p></div>`;
    }
  }
}

function renderTimelineHierarchyNav() {
  if (!el.hierarchyBreadcrumbs || !el.hierarchyControls) return;

  if (state.timeline.searchAllDates) {
    const totalItems = state.timeline.days.reduce((acc, d) => acc + (d.items?.length || 0), 0);
    const searchLabel = state.timeline.search ? `"${state.timeline.search}"` : 'All Dates';
    el.hierarchyBreadcrumbs.innerHTML = `
      <span class="crumb-current">${t('timeline.all_history_search')}: ${escapeHtml(searchLabel)}</span>
      <span class="crumb-separator">·</span>
      <span style="font-size:0.8rem;color:var(--text-dim);">${totalItems} ${totalItems === 1 ? t('timeline.item') : t('timeline.items')} (${state.timeline.days.length} ${t('stats.days')})</span>
    `;
    el.hierarchyControls.innerHTML = `
      <button class="btn btn-outline btn-xs" id="btn-exit-search-all-dates" title="Exit all-history search and return to monthly timeline">
        ${t('timeline.exit_search_all')}
      </button>
    `;
    const btnExit = document.getElementById('btn-exit-search-all-dates');
    if (btnExit) {
      btnExit.addEventListener('click', () => {
        state.timeline.searchAllDates = false;
        if (el.timelineSearchAllDates) el.timelineSearchAllDates.checked = false;
        if (el.timelineSearchAllLabel) el.timelineSearchAllLabel.classList.remove('active');
        loadTimeline();
      });
    }
    if (el.timelineMonthChips) el.timelineMonthChips.innerHTML = '';
    return;
  }

  const level = state.timeline.level;
  const year = state.timeline.selectedYear;
  const monthIdx = parseInt(state.timeline.selectedMonth, 10) - 1;
  const monthName = MONTH_NAMES[monthIdx] || 'Month';

  // 1. Breadcrumbs
  if (level === 'years') {
    el.hierarchyBreadcrumbs.innerHTML = `
      <span class="crumb-current">${t('timeline.all_years')}</span>
    `;
  } else if (level === 'year') {
    el.hierarchyBreadcrumbs.innerHTML = `
      <button class="crumb-btn" id="btn-crumb-years" title="Dig back to all years">${t('timeline.all_years')}</button>
      <span class="crumb-separator">/</span>
      <span class="crumb-current">${year}</span>
    `;
  } else {
    // level === 'month'
    el.hierarchyBreadcrumbs.innerHTML = `
      <button class="crumb-btn" id="btn-crumb-years" title="Dig back to all years">${t('timeline.all_years')}</button>
      <span class="crumb-separator">/</span>
      <button class="crumb-btn" id="btn-crumb-year" title="${t('timeline.view_year_months', { year })}">${year}</button>
      <span class="crumb-separator">/</span>
      <span class="crumb-current">${monthName}</span>
    `;
  }

  // 2. Controls & Steppers
  if (level === 'years') {
    el.hierarchyControls.innerHTML = `
      <span class="hierarchy-active-label">${t('timeline.years_overview')}</span>
    `;
  } else if (level === 'year') {
    el.hierarchyControls.innerHTML = `
      <button class="btn btn-outline btn-xs btn-drill-up" id="btn-timeline-up" title="Dig back further to all years">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="18 15 12 9 6 15"/></svg>
        ${t('timeline.all_years')}
      </button>
      <div class="hierarchy-nav-stepper">
        <button class="btn btn-icon btn-xs" id="btn-timeline-prev-year" title="Previous Year">‹</button>
        <span class="hierarchy-active-label">${year}</span>
        <button class="btn btn-icon btn-xs" id="btn-timeline-next-year" title="Next Year">›</button>
      </div>
    `;
  } else {
    // level === 'month'
    el.hierarchyControls.innerHTML = `
      <button class="btn btn-outline btn-xs btn-drill-up" id="btn-timeline-up" title="${t('timeline.view_year_months', { year })}">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="18 15 12 9 6 15"/></svg>
        ${t('timeline.year_months', { year })}
      </button>
      <div class="hierarchy-nav-stepper">
        <button class="btn btn-icon btn-xs" id="btn-timeline-prev-month" title="Previous Month">‹</button>
        <span class="hierarchy-active-label">${monthName} ${year}</span>
        <button class="btn btn-icon btn-xs" id="btn-timeline-next-month" title="Next Month">›</button>
      </div>
    `;
  }

  // Attach breadcrumb & control listeners
  const crumbYears = el.hierarchyBreadcrumbs.querySelector('#btn-crumb-years');
  if (crumbYears) {
    crumbYears.addEventListener('click', () => {
      state.timeline.level = 'years';
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  }

  const crumbYear = el.hierarchyBreadcrumbs.querySelector('#btn-crumb-year');
  if (crumbYear) {
    crumbYear.addEventListener('click', () => {
      state.timeline.level = 'year';
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  }

  const btnUp = el.hierarchyControls.querySelector('#btn-timeline-up');
  if (btnUp) {
    btnUp.addEventListener('click', () => {
      if (state.timeline.level === 'month') {
        state.timeline.level = 'year';
      } else if (state.timeline.level === 'year') {
        state.timeline.level = 'years';
      }
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  }

  const prevMonthBtn = el.hierarchyControls.querySelector('#btn-timeline-prev-month');
  if (prevMonthBtn) {
    prevMonthBtn.addEventListener('click', () => {
      let m = parseInt(state.timeline.selectedMonth, 10) - 1;
      let y = parseInt(state.timeline.selectedYear, 10);
      if (m < 1) {
        m = 12;
        y -= 1;
      }
      state.timeline.selectedYear = String(y);
      state.timeline.selectedMonth = String(m).padStart(2, '0');
      state.timeline.date = '';
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  }

  const nextMonthBtn = el.hierarchyControls.querySelector('#btn-timeline-next-month');
  if (nextMonthBtn) {
    nextMonthBtn.addEventListener('click', () => {
      let m = parseInt(state.timeline.selectedMonth, 10) + 1;
      let y = parseInt(state.timeline.selectedYear, 10);
      if (m > 12) {
        m = 1;
        y += 1;
      }
      state.timeline.selectedYear = String(y);
      state.timeline.selectedMonth = String(m).padStart(2, '0');
      state.timeline.date = '';
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  }

  const prevYearBtn = el.hierarchyControls.querySelector('#btn-timeline-prev-year');
  if (prevYearBtn) {
    prevYearBtn.addEventListener('click', () => {
      state.timeline.selectedYear = String(parseInt(state.timeline.selectedYear, 10) - 1);
      state.timeline.date = '';
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  }

  const nextYearBtn = el.hierarchyControls.querySelector('#btn-timeline-next-year');
  if (nextYearBtn) {
    nextYearBtn.addEventListener('click', () => {
      state.timeline.selectedYear = String(parseInt(state.timeline.selectedYear, 10) + 1);
      state.timeline.date = '';
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  }

  // 3. Month Chips for currently selected year
  if (el.timelineMonthChips) {
    if (level === 'years') {
      el.timelineMonthChips.style.display = 'none';
    } else {
      el.timelineMonthChips.style.display = 'flex';
      const yearData = state.timeline.periodsData?.years?.find(y => y.year === year);
      const monthMap = new Map();
      if (yearData && yearData.months) {
        yearData.months.forEach(m => monthMap.set(m.month_num, m.count));
      }

      el.timelineMonthChips.innerHTML = MONTH_SHORT_NAMES.map((shortName, idx) => {
        const monthNumStr = String(idx + 1).padStart(2, '0');
        const count = monthMap.get(monthNumStr) || 0;
        const isActive = level === 'month' && state.timeline.selectedMonth === monthNumStr;
        const badge = count > 0 ? `<span class="month-chip-badge">${count}</span>` : '';
        const classes = ['month-chip'];
        if (isActive) classes.push('active');
        if (count > 0) classes.push('has-data');
        return `
          <button class="${classes.join(' ')}" data-month-num="${monthNumStr}" title="${MONTH_NAMES[idx]} ${year}: ${count} watches">
            ${shortName} ${badge}
          </button>
        `;
      }).join('');

      el.timelineMonthChips.querySelectorAll('.month-chip').forEach(btn => {
        btn.addEventListener('click', () => {
          state.timeline.selectedMonth = btn.dataset.monthNum;
          state.timeline.level = 'month';
          state.timeline.date = '';
          state.timeline.userManuallySelected = true;
          loadTimeline();
        });
      });
    }
  }
}

function renderTimelineMonthsView() {
  const year = state.timeline.selectedYear;
  const yearData = state.timeline.periodsData?.years?.find(y => y.year === year);
  const totalCount = yearData ? yearData.count : 0;
  const totalMinutes = yearData ? yearData.minutes : 0;
  const totalHours = Math.round(totalMinutes / 60);

  const monthMap = new Map();
  if (yearData && yearData.months) {
    yearData.months.forEach(m => monthMap.set(m.month_num, m));
  }

  let html = `
    <div class="timeline-day-header" style="margin-bottom: 16px;">
      <div style="display:flex;align-items:center;gap:12px;">
        <div class="timeline-date-title">${year} — ${t('timeline.months_overview')}</div>
      </div>
      <div class="timeline-day-stats">
        <span class="stat-chip">${totalCount} ${totalCount === 1 ? t('timeline.item') : t('timeline.items')} in ${year}</span>
        <span class="stat-chip">${totalHours}h</span>
      </div>
    </div>
    <div class="timeline-months-grid">
  `;

  for (let i = 0; i < 12; i++) {
    const monthNum = String(i + 1).padStart(2, '0');
    const mData = monthMap.get(monthNum);
    const count = mData ? mData.count : 0;
    const minutes = mData ? mData.minutes : 0;
    const hours = Math.floor(minutes / 60);
    const remMins = minutes % 60;
    const timeStr = count > 0 ? (hours > 0 ? `${hours}h ${remMins}m ${t('timeline.hours_watched')}` : `${remMins}m ${t('timeline.hours_watched')}`) : t('stats.no_activity');
    const posters = mData?.poster_urls || [];
    const validPosters = posters.map(url => resolveImageUrl(url)).filter(Boolean);
    const posterImgs = validPosters.map(url => `<img src="${escapeHtml(url)}" class="month-card-poster" alt="Poster" loading="lazy" onerror="this.style.display='none'" />`).join('');

    html += `
      <div class="timeline-month-card ${count > 0 ? 'has-data' : 'empty'}" data-month-num="${monthNum}">
        <div class="month-card-header">
          <span class="month-card-title">${MONTH_NAMES[i]}</span>
          <span class="month-card-badge">${count} ${count === 1 ? t('timeline.item') : t('timeline.items')}</span>
        </div>
        <div class="month-card-stats">${timeStr}</div>
        <div class="month-card-posters">
          ${posterImgs || '<div style="display:flex;align-items:center;justify-content:center;width:100%;color:var(--text-dim);font-size:0.75rem;">No posters</div>'}
        </div>
        <button class="btn btn-outline btn-xs" style="width: 100%; margin-top: 4px;">${t('timeline.view_dates')} →</button>
      </div>
    `;
  }

  html += `</div>`;
  el.timelineContainer.innerHTML = html;

  el.timelineContainer.querySelectorAll('.timeline-month-card').forEach(card => {
    card.addEventListener('click', () => {
      state.timeline.selectedMonth = card.dataset.monthNum;
      state.timeline.level = 'month';
      state.timeline.date = '';
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  });
}

function renderTimelineYearsView() {
  const years = state.timeline.periodsData?.years || [];
  const currentYearStr = state.timeline.periodsData?.current_year || String(new Date().getFullYear());

  let displayYears = [...years];
  if (!displayYears.some(y => y.year === currentYearStr)) {
    displayYears.unshift({
      year: currentYearStr,
      count: 0,
      minutes: 0,
      months: [],
      poster_urls: [],
    });
  }

  const totalLifetimeWatches = displayYears.reduce((sum, y) => sum + y.count, 0);

  let html = `
    <div class="timeline-day-header" style="margin-bottom: 16px;">
      <div style="display:flex;align-items:center;gap:12px;">
        <div class="timeline-date-title">${t('timeline.watch_history_by_year')}</div>
      </div>
      <div class="timeline-day-stats">
        <span class="stat-chip">${totalLifetimeWatches} ${t('stats.total_scrobbles')}</span>
        <span class="stat-chip">${displayYears.length} ${t('stats.days')}</span>
      </div>
    </div>
    <div class="timeline-years-grid">
  `;

  for (const y of displayYears) {
    const hours = Math.round(y.minutes / 60);
    const activeMonths = y.months ? y.months.length : 0;
    const posters = y.poster_urls || [];
    const validPosters = posters.map(url => resolveImageUrl(url)).filter(Boolean);
    const posterImgs = validPosters.map(url => `<img src="${escapeHtml(url)}" class="year-card-poster" alt="Poster" loading="lazy" onerror="this.style.display='none'" />`).join('');

    html += `
      <div class="timeline-year-card" data-year="${y.year}">
        <div class="year-card-top">
          <div class="year-card-number">${y.year}</div>
          <span class="year-card-badge">${y.count} ${y.count === 1 ? t('timeline.item') : t('timeline.items')}</span>
        </div>
        <div class="year-card-stats">${hours} ${t('timeline.hours_watched')} • ${activeMonths} ${activeMonths === 1 ? t('timeline.active_month') : t('timeline.active_months')}</div>
        <div class="year-card-posters">
          ${posterImgs || '<div style="display:flex;align-items:center;justify-content:center;width:100%;color:var(--text-dim);font-size:0.75rem;">No posters</div>'}
        </div>
        <button class="btn btn-outline btn-sm" style="width: 100%; margin-top: 6px;">${t('timeline.explore_year')} →</button>
      </div>
    `;
  }

  html += `</div>`;
  el.timelineContainer.innerHTML = html;

  el.timelineContainer.querySelectorAll('.timeline-year-card').forEach(card => {
    card.addEventListener('click', () => {
      state.timeline.selectedYear = card.dataset.year;
      state.timeline.level = 'year';
      state.timeline.date = '';
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  });
}

function renderTimelineScrubberMonths() {
  if (!el.timelineScrubber || !el.timelineScrubberList) return;
  const year = state.timeline.selectedYear;
  const yearData = state.timeline.periodsData?.years?.find(y => y.year === year);
  if (!yearData || !yearData.months || yearData.months.length === 0) {
    el.timelineScrubber.style.display = 'none';
    el.timelineScrubberList.innerHTML = '';
    return;
  }

  el.timelineScrubber.style.display = 'block';
  const header = el.timelineScrubber.querySelector('.scrubber-header');
  if (header) header.textContent = `${year} Months`;

  el.timelineScrubberList.innerHTML = yearData.months.map(m => {
    return `<button class="scrubber-item" data-month-num="${m.month_num}" title="${m.month_name} (${m.count} watches)">${m.month_name} (${m.count})</button>`;
  }).join('');

  el.timelineScrubberList.querySelectorAll('.scrubber-item').forEach(btn => {
    btn.addEventListener('click', () => {
      state.timeline.selectedMonth = btn.dataset.monthNum;
      state.timeline.level = 'month';
      state.timeline.date = '';
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  });
}

function renderTimelineScrubberYears() {
  if (!el.timelineScrubber || !el.timelineScrubberList) return;
  const years = state.timeline.periodsData?.years || [];
  if (years.length === 0) {
    el.timelineScrubber.style.display = 'none';
    el.timelineScrubberList.innerHTML = '';
    return;
  }

  el.timelineScrubber.style.display = 'block';
  const header = el.timelineScrubber.querySelector('.scrubber-header');
  if (header) header.textContent = 'Years';

  el.timelineScrubberList.innerHTML = years.map(y => {
    return `<button class="scrubber-item" data-year="${y.year}" title="${y.year} (${y.count} watches)">${y.year} (${y.count})</button>`;
  }).join('');

  el.timelineScrubberList.querySelectorAll('.scrubber-item').forEach(btn => {
    btn.addEventListener('click', () => {
      state.timeline.selectedYear = btn.dataset.year;
      state.timeline.level = 'year';
      state.timeline.date = '';
      state.timeline.userManuallySelected = true;
      loadTimeline();
    });
  });
}

function renderTimeline(days) {
  if (!days || days.length === 0) {
    const hasFilters = Boolean(
      state.timeline.search ||
      state.timeline.genre ||
      state.timeline.person ||
      (state.timeline.rating && state.timeline.rating !== 'all') ||
      (state.timeline.watch_status && state.timeline.watch_status !== 'all')
    );
    const monthIdx = parseInt(state.timeline.selectedMonth, 10) - 1;
    const monthName = MONTH_NAMES[monthIdx] || 'Month';
    const totalLifetimeWatches = state.timeline.periodsData?.years?.reduce((sum, y) => sum + y.count, 0) || 0;

    let msg = '';
    let actionBtn = '';
    if (state.timeline.search && !state.timeline.searchAllDates) {
      msg = `No watch history matches "${state.timeline.search}" in ${monthName} ${state.timeline.selectedYear}.`;
      actionBtn = `<button class="btn btn-primary btn-sm" id="btn-empty-search-all" style="margin-top:12px;">Search Across All History (All Dates)</button>`;
    } else if (hasFilters) {
      msg = 'No watch history matches your timeline filters.';
    } else if (totalLifetimeWatches === 0) {
      msg = 'No watch history yet. Connect your Plex Webhook or log a scrobble to see your timeline!';
    } else {
      msg = `No watch history logged for ${monthName} ${state.timeline.selectedYear}.`;
      actionBtn = `<button class="btn btn-outline btn-xs" id="btn-empty-view-year" style="margin-top:12px;">View ${state.timeline.selectedYear} Months</button>`;
    }

    el.timelineContainer.innerHTML = `
      <div class="empty-state">
        <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
        <p>${msg}</p>
        ${actionBtn}
      </div>`;

    const emptyBtn = el.timelineContainer.querySelector('#btn-empty-view-year');
    if (emptyBtn) {
      emptyBtn.addEventListener('click', () => {
        state.timeline.level = 'year';
        state.timeline.userManuallySelected = true;
        loadTimeline();
      });
    }
    const searchAllBtn = el.timelineContainer.querySelector('#btn-empty-search-all');
    if (searchAllBtn) {
      searchAllBtn.addEventListener('click', () => {
        state.timeline.searchAllDates = true;
        if (el.timelineSearchAllDates) el.timelineSearchAllDates.checked = true;
        if (el.timelineSearchAllLabel) el.timelineSearchAllLabel.classList.add('active');
        loadTimeline();
      });
    }
    return;
  }

  el.timelineContainer.innerHTML = days.map(day => `
    <div class="timeline-day" data-date="${day.date}">
      <div class="timeline-day-header">
        <div style="display:flex;align-items:center;gap:12px;">
          <div class="timeline-date-title">${formatDisplayDate(day.date)}</div>
          <button class="day-header-delete-btn" data-date="${day.date}" title="${t('timeline.confirm_delete_day', { date: day.date })}">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
            ${t('timeline.delete_day')}
          </button>
        </div>
        <div class="timeline-day-stats">
          <span class="stat-chip">${day.total_count} ${day.total_count === 1 ? t('timeline.item') : t('timeline.items')}</span>
          <span class="stat-chip">${t('timeline.min_watched', { mins: day.total_minutes })}</span>
        </div>
      </div>
      <div class="scrobble-grid">
        ${day.items.map(item => renderScrobbleCard(item, false)).join('')}
      </div>
    </div>
  `).join('');

  // Add delete day button listeners
  el.timelineContainer.querySelectorAll('.day-header-delete-btn').forEach(btn => {
    btn.addEventListener('click', async (e) => {
      e.stopPropagation();
      const dateStr = btn.dataset.date;
      if (confirm(t('timeline.confirm_delete_day', { date: dateStr }))) {
        try {
          await api.deleteByDate(dateStr);
          loadTimeline();
          loadStats();
          loadHistory();
        } catch (err) {
          alert(`Failed to delete scrobbles on ${dateStr}: ${err.message}`);
        }
      }
    });
  });

  // Add card click listeners
  attachScrobbleCardListeners(el.timelineContainer, days.flatMap(d => d.items));
}

function renderTimelineScrubber(days) {
  if (!el.timelineScrubber || !el.timelineScrubberList) return;
  if (!days || days.length === 0) {
    el.timelineScrubber.style.display = 'none';
    el.timelineScrubberList.innerHTML = '';
    return;
  }

  el.timelineScrubber.style.display = 'block';
  const header = el.timelineScrubber.querySelector('.scrubber-header');
  const monthIdx = parseInt(state.timeline.selectedMonth, 10) - 1;
  if (header) {
    header.textContent = state.timeline.searchAllDates
      ? 'All Dates'
      : `${MONTH_SHORT_NAMES[monthIdx] || 'Month'} Dates`;
  }

  const dateMap = new Map();
  days.forEach(d => {
    if (!dateMap.has(d.date)) {
      dateMap.set(d.date, d.total_count);
    }
  });

  el.timelineScrubberList.innerHTML = Array.from(dateMap.entries()).map(([dateStr, count]) => {
    const formatted = formatDisplayDate(dateStr);
    return `<button class="scrubber-item" data-date="${dateStr}" title="Jump to ${formatted} (${count} watches)">${formatted}</button>`;
  }).join('');

  el.timelineScrubberList.querySelectorAll('.scrubber-item').forEach(btn => {
    btn.addEventListener('click', () => {
      const date = btn.dataset.date;
      const targetDay = el.timelineContainer.querySelector(`.timeline-day[data-date="${date}"]`);
      if (targetDay) {
        targetDay.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }
    });
  });
}

function renderTimelinePagination() {
  if (!el.timelineLoadMoreWrap) return;
  if (state.timeline.hasMore) {
    el.timelineLoadMoreWrap.style.display = 'flex';
    const totalItems = state.timeline.days.reduce((acc, d) => acc + d.items.length, 0);
    if (el.timelinePaginationInfo) {
      el.timelinePaginationInfo.textContent = `Showing ${totalItems} scrobbles across ${state.timeline.days.length} days`;
    }
  } else {
    el.timelineLoadMoreWrap.style.display = 'none';
  }
}

if (el.btnTimelineLoadMore) {
  el.btnTimelineLoadMore.addEventListener('click', async () => {
    el.btnTimelineLoadMore.disabled = true;
    el.btnTimelineLoadMore.textContent = 'Loading...';
    state.timeline.offset += state.timeline.limit;
    await loadTimeline(true);
    el.btnTimelineLoadMore.disabled = false;
    el.btnTimelineLoadMore.textContent = 'Load More Watches';
  });
}

function hasActiveTimelineFilters() {
  return Boolean(
    state.timeline.search ||
    state.timeline.date ||
    state.timeline.genre ||
    state.timeline.person ||
    (state.timeline.rating && state.timeline.rating !== 'all') ||
    (state.timeline.watch_status && state.timeline.watch_status !== 'all') ||
    state.timeline.missingMetadata
  );
}

function showTimelineFilters() {
  if (el.timelineFiltersGrid) {
    el.timelineFiltersGrid.style.display = 'grid';
  }
}

function hideTimelineFilters() {
  if (el.timelineFiltersGrid && !hasActiveTimelineFilters()) {
    el.timelineFiltersGrid.style.display = 'none';
  }
}

// Expandable Search Interaction & Hidden Filter UI
if (el.timelineSearchWrap && el.timelineSearch) {
  el.timelineSearchWrap.addEventListener('click', () => {
    el.timelineSearch.focus();
    el.timelineSearchWrap.classList.add('expanded');
    showTimelineFilters();
  });

  el.timelineSearch.addEventListener('focus', () => {
    el.timelineSearchWrap.classList.add('expanded');
    showTimelineFilters();
  });
}

document.addEventListener('click', (e) => {
  if (!el.timelineFilterBar) return;
  if (!el.timelineFilterBar.contains(e.target)) {
    if (!hasActiveTimelineFilters() && !el.timelineSearch?.value.trim()) {
      el.timelineSearchWrap?.classList.remove('expanded');
      el.timelineSearchWrap?.classList.remove('has-value');
      hideTimelineFilters();
    }
  }
});

// Timeline Filter Event Listeners
let timelineSearchTimer = null;
if (el.timelineSearch) {
  el.timelineSearch.addEventListener('input', (e) => {
    if (e.target.value.trim()) {
      el.timelineSearchWrap?.classList.add('has-value');
    } else {
      el.timelineSearchWrap?.classList.remove('has-value');
    }
    showTimelineFilters();
    clearTimeout(timelineSearchTimer);
    timelineSearchTimer = setTimeout(() => {
      state.timeline.search = e.target.value.trim();
      loadTimeline();
    }, 300);
  });
}

if (el.timelineDateFilter) {
  el.timelineDateFilter.addEventListener('change', (e) => {
    const val = e.target.value;
    state.timeline.date = val;
    if (val) {
      const parts = val.split('-');
      if (parts.length >= 2) {
        state.timeline.selectedYear = parts[0];
        state.timeline.selectedMonth = parts[1];
        state.timeline.level = 'month';
        state.timeline.userManuallySelected = true;
      }
    }
    loadTimeline();
  });
}

if (el.timelineGenreFilter) {
  let genreTimer = null;
  el.timelineGenreFilter.addEventListener('input', (e) => {
    clearTimeout(genreTimer);
    genreTimer = setTimeout(() => {
      state.timeline.genre = e.target.value.trim();
      loadTimeline();
    }, 300);
  });
}

if (el.timelinePersonFilter) {
  let personTimer = null;
  el.timelinePersonFilter.addEventListener('input', (e) => {
    clearTimeout(personTimer);
    personTimer = setTimeout(() => {
      state.timeline.person = e.target.value.trim();
      loadTimeline();
    }, 300);
  });
}

if (el.timelineRatingFilter) {
  el.timelineRatingFilter.addEventListener('change', (e) => {
    state.timeline.rating = e.target.value;
    loadTimeline();
  });
}

if (el.timelineStatusFilter) {
  el.timelineStatusFilter.addEventListener('change', (e) => {
    state.timeline.watch_status = e.target.value;
    loadTimeline();
  });
}

if (el.timelineSearchAllDates) {
  el.timelineSearchAllDates.addEventListener('change', (e) => {
    state.timeline.searchAllDates = e.target.checked;
    if (el.timelineSearchAllLabel) {
      el.timelineSearchAllLabel.classList.toggle('active', e.target.checked);
    }
    loadTimeline();
  });
}

if (el.btnResetTimelineFilters) {
  el.btnResetTimelineFilters.addEventListener('click', () => {
    state.timeline.search = '';
    state.timeline.searchAllDates = false;
    state.timeline.date = '';
    state.timeline.genre = '';
    state.timeline.person = '';
    state.timeline.rating = 'all';
    state.timeline.watch_status = 'all';
    state.timeline.level = 'month';
    state.timeline.userManuallySelected = false;

    if (el.timelineSearch) el.timelineSearch.value = '';
    if (el.timelineSearchWrap) {
      el.timelineSearchWrap.classList.remove('expanded');
      el.timelineSearchWrap.classList.remove('has-value');
    }
    if (el.timelineSearchAllDates) el.timelineSearchAllDates.checked = false;
    if (el.timelineSearchAllLabel) el.timelineSearchAllLabel.classList.remove('active');
    if (el.timelineDateFilter) el.timelineDateFilter.value = '';
    if (el.timelineGenreFilter) el.timelineGenreFilter.value = '';
    if (el.timelinePersonFilter) el.timelinePersonFilter.value = '';
    if (el.timelineRatingFilter) el.timelineRatingFilter.value = 'all';
    if (el.timelineStatusFilter) el.timelineStatusFilter.value = 'all';
    state.timeline.missingMetadata = false;
    if (el.btnTimelineMissingMetadata) el.btnTimelineMissingMetadata.classList.remove('active');

    hideTimelineFilters();
    loadTimeline();
  });
}

if (el.btnTimelineMissingMetadata) {
  el.btnTimelineMissingMetadata.addEventListener('click', () => {
    state.timeline.missingMetadata = !state.timeline.missingMetadata;
    el.btnTimelineMissingMetadata.classList.toggle('active', state.timeline.missingMetadata);
    loadTimeline();
  });
}

function renderScrobbleCard(item, isHistory = false) {
  const isEpisode = item.media_type === 'episode';
  const badgeClass = isEpisode ? 'badge-episode' : 'badge-movie';
  const badgeText = isEpisode
    ? `S${String(item.season_number || 1).padStart(2, '0')}E${String(item.episode_number || 1).padStart(2, '0')}`
    : t('history.movie');

  const subTitle = isEpisode && item.grandparent_title
    ? `${item.grandparent_title}`
    : (item.year ? `${item.year}` : '');

  const durationMin = item.duration_ms ? `${Math.round(item.duration_ms / 60000)}m` : '';

  const resolvedThumb = resolveImageUrl(item.thumb_url);
  const posterHtml = resolvedThumb
    ? `<img src="${escapeHtml(resolvedThumb)}" alt="${escapeHtml(item.title)}" class="scrobble-poster" onerror="this.outerHTML='<div class=\\'scrobble-poster\\'>🎬</div>'"/>`
    : `<div class="scrobble-poster">${isEpisode ? '📺' : '🎬'}</div>`;

  const ratingHtml = item.user_rating
    ? `<span class="scrobble-rating" title="User Rating: ${item.user_rating}/10">★ ${item.user_rating}/10</span>`
    : '';

  const isSelected = isHistory && state.history.selectedIds.has(item.id);
  const cardClasses = ['scrobble-card'];
  if (isHistory && state.history.selectMode) cardClasses.push('select-mode');
  if (isSelected) cardClasses.push('selected');

  const checkboxHtml = isHistory && state.history.selectMode
    ? `<input type="checkbox" class="scrobble-checkbox" data-id="${item.id}" ${isSelected ? 'checked' : ''} onclick="event.stopPropagation()" />`
    : '';

  const imdbUrl = getImdbUrl(item);
  const tvdbUrl = getTvdbUrl(item);

  const isInProgress = item.watch_status === 'in_progress';
  const progressBadgeHtml = isInProgress
    ? `<span class="scrobble-badge badge-in-progress" title="${t('history.in_progress')}: ${item.progress_percent || 0}%">⏳ ${item.progress_percent || 0}%</span>`
    : '';

  const hasMissingMetadata = !item.thumb_url || !item.overview || !item.genres;
  const missingBadgeHtml = hasMissingMetadata
    ? `<span class="scrobble-badge badge-missing-metadata" title="Missing metadata or poster — click card to Fix Match">⚠️ Needs Match</span>`
    : '';

  const progressBarHtml = isInProgress && (item.progress_percent != null)
    ? `<div class="scrobble-progress-bar"><div class="scrobble-progress-fill" style="width: ${Math.min(100, Math.max(0, item.progress_percent))}%;"></div></div>`
    : '';

  return `
    <div class="${cardClasses.join(' ')}" data-id="${item.id}">
      ${checkboxHtml}
      ${posterHtml}
      <div class="scrobble-header-badges">
        <span class="scrobble-badge ${badgeClass}">${badgeText}</span>
        ${progressBadgeHtml}
        ${ratingHtml}
        ${missingBadgeHtml}
        <div class="card-external-links">
          <a href="${imdbUrl}" target="_blank" rel="noopener noreferrer" class="card-imdb-link" title="Open on IMDb" onclick="event.stopPropagation()">IMDb ↗</a>
          <a href="${tvdbUrl}" target="_blank" rel="noopener noreferrer" class="card-tvdb-link" title="View on TheTVDB" onclick="event.stopPropagation()">TVDB ↗</a>
        </div>
      </div>
      <div class="scrobble-card-title">${escapeHtml(item.title)}</div>
      ${subTitle ? `<div class="scrobble-card-sub">${escapeHtml(subTitle)}</div>` : ''}
      ${(item.director || item.directors) ? `<div class="scrobble-card-director" onclick="event.stopPropagation()"><span style="font-size:0.72rem;color:var(--text-dim);">Dir:</span> <button type="button" class="person-tag-card" data-person="${escapeHtml((item.director || item.directors).split(',')[0].trim())}" title="Show history with ${escapeHtml((item.director || item.directors).split(',')[0].trim())}">${escapeHtml((item.director || item.directors).split(',')[0].trim())}</button></div>` : ''}
      ${item.cinematographer ? `<div class="scrobble-card-cinematographer" onclick="event.stopPropagation()"><span style="font-size:0.72rem;color:var(--text-dim);">Cinematography:</span> <button type="button" class="person-tag-card" data-person="${escapeHtml(item.cinematographer.split(',')[0].trim())}" title="Show history with ${escapeHtml(item.cinematographer.split(',')[0].trim())}">${escapeHtml(item.cinematographer.split(',')[0].trim())}</button></div>` : ''}
      <div class="scrobble-card-meta">
        <span>${formatTime(item.watched_at)}</span>
        <span>${durationMin}</span>
      </div>
      ${item.tags ? `
        <div class="scrobble-card-tags" onclick="event.stopPropagation()">
          ${item.tags.split(',').map(t => t.trim()).filter(Boolean).map(t => `
            <button type="button" class="tag-chip-card" data-tag="${escapeHtml(t)}" title="Filter by tag: ${escapeHtml(t)}">🏷️ ${escapeHtml(t)}</button>
          `).join('')}
        </div>
      ` : ''}
      ${item.notes ? `
        <div class="scrobble-card-notes" title="${escapeHtml(item.notes)}">
          <span style="opacity:0.8;">📝</span> <em>${escapeHtml(item.notes.length > 55 ? item.notes.substring(0, 52) + '...' : item.notes)}</em>
        </div>
      ` : ''}
      ${progressBarHtml}
    </div>
  `;
}

function attachScrobbleCardListeners(container, items, isHistory = false) {
  const itemMap = new Map(items.map(i => [i.id, i]));
  container.querySelectorAll('.scrobble-card').forEach(card => {
    // Click on person tag on card
    card.querySelectorAll('.person-tag-card').forEach(pt => {
      pt.addEventListener('click', (e) => {
        e.stopPropagation();
        const p = pt.dataset.person;
        if (p) filterHistoryByPerson(p);
      });
    });

    // Click on personal tag chip on card
    card.querySelectorAll('.tag-chip-card').forEach(tc => {
      tc.addEventListener('click', (e) => {
        e.stopPropagation();
        const tag = tc.dataset.tag;
        if (tag) filterHistoryByTag(tag);
      });
    });

    card.addEventListener('click', (e) => {
      // If clicking directly on checkbox, let checkbox change handler handle it
      if (e.target.classList.contains('scrobble-checkbox')) return;

      if (isHistory && state.history.selectMode) {
        // Toggle selection in select mode
        const id = card.dataset.id;
        const cb = card.querySelector('.scrobble-checkbox');
        if (state.history.selectedIds.has(id)) {
          state.history.selectedIds.delete(id);
          card.classList.remove('selected');
          if (cb) cb.checked = false;
        } else {
          state.history.selectedIds.add(id);
          card.classList.add('selected');
          if (cb) cb.checked = true;
        }
        updateBulkBarState();
        return;
      }

      const item = itemMap.get(card.dataset.id);
      if (item) openDetailModal(item);
    });

    const cb = card.querySelector('.scrobble-checkbox');
    if (cb) {
      cb.addEventListener('change', (e) => {
        const id = card.dataset.id;
        if (e.target.checked) {
          state.history.selectedIds.add(id);
          card.classList.add('selected');
        } else {
          state.history.selectedIds.delete(id);
          card.classList.remove('selected');
        }
        updateBulkBarState();
      });
    }
  });
}

function updateHistoryPersonBadge() {
  if (!el.historyPersonBadge) return;
  if (state.history.person) {
    el.historyPersonBadge.style.display = 'inline-flex';
    el.historyPersonBadge.innerHTML = `
      <span>Cast / Director: <strong>${escapeHtml(state.history.person)}</strong></span>
      <button type="button" id="btn-clear-history-person" title="Clear person filter">✕</button>
    `;
    const btn = el.historyPersonBadge.querySelector('#btn-clear-history-person');
    if (btn) {
      btn.onclick = () => {
        state.history.person = '';
        updateHistoryPersonBadge();
        state.history.page = 1;
        loadHistory();
      };
    }
  } else if (state.history.tag) {
    el.historyPersonBadge.style.display = 'inline-flex';
    el.historyPersonBadge.innerHTML = `
      <span>Tag: <strong>${escapeHtml(state.history.tag)}</strong></span>
      <button type="button" id="btn-clear-history-tag" title="Clear tag filter">✕</button>
    `;
    const btn = el.historyPersonBadge.querySelector('#btn-clear-history-tag');
    if (btn) {
      btn.onclick = () => {
        state.history.tag = '';
        updateHistoryPersonBadge();
        state.history.page = 1;
        loadHistory();
      };
    }
  } else {
    el.historyPersonBadge.style.display = 'none';
    el.historyPersonBadge.innerHTML = '';
  }
}

function filterHistoryByPerson(personName) {
  if (el.modalItemDetail) el.modalItemDetail.style.display = 'none';
  state.history.person = personName;
  state.history.tag = '';
  state.history.page = 1;
  state.history.search = '';
  if (el.historySearch) el.historySearch.value = '';

  const tabHistory = document.getElementById('tab-history');
  if (tabHistory) tabHistory.click();

  updateHistoryPersonBadge();
  loadHistory();
}

function filterHistoryByTag(tag) {
  if (el.modalItemDetail) el.modalItemDetail.style.display = 'none';
  state.history.tag = tag;
  state.history.person = '';
  state.history.page = 1;
  state.history.search = '';
  if (el.historySearch) el.historySearch.value = `tag:${tag}`;

  const tabHistory = document.getElementById('tab-history');
  if (tabHistory) tabHistory.click();

  updateHistoryPersonBadge();
  loadHistory();
}

// View 2: History
async function loadHistory() {
  try {
    updateHistoryPersonBadge();
    const res = await api.getHistory({
      page: state.history.page,
      limit: state.history.limit,
      media_type: state.history.mediaType,
      rating: state.history.rating,
      search: state.history.search,
      person: state.history.person,
      tag: state.history.tag,
      all: state.history.allResults,
      all_dates: state.history.allResults,
      missing_metadata: state.history.missingMetadata,
    });

    state.history.totalPages = res.total_pages || 1;
    state.history.total = res.total || 0;
    state.history.currentItems = res.items || [];
    renderHistory(res);
  } catch (err) {
    console.error('Error loading history:', err);
    el.historyList.innerHTML = `<div class="empty-state"><p>${t('history.empty_filtered')}</p></div>`;
  }
}

function renderHistory(res) {
  if (!res.items || res.items.length === 0) {
    el.historyList.innerHTML = `
      <div class="empty-state">
        <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
        <p>${t('history.empty_filtered')}</p>
      </div>`;
    el.historyPagination.style.display = 'none';
    return;
  }

  el.historyList.innerHTML = `
    <div class="scrobble-grid">
      ${res.items.map(item => renderScrobbleCard(item, true)).join('')}
    </div>
  `;

  attachScrobbleCardListeners(el.historyList, res.items, true);
  updateBulkBarState();

  // Pagination bar
  el.historyPagination.style.display = 'flex';
  if (state.history.allResults) {
    el.paginationInfo.textContent = t('history.showing_all', { total: res.total });
    el.btnPagePrev.disabled = true;
    el.btnPageNext.disabled = true;
  } else {
    el.paginationInfo.textContent = t('history.page_info', { page: res.page, pages: res.total_pages, total: res.total });
    el.btnPagePrev.disabled = res.page <= 1;
    el.btnPageNext.disabled = res.page >= res.total_pages;
  }
}

// Multi-Select Handlers
function updateBulkBarState() {
  if (!el.historyBulkBar) return;
  const count = state.history.selectedIds.size;
  if (el.bulkSelectedCount) el.bulkSelectedCount.textContent = t('history.selected_count', { count });
  if (el.btnDeleteSelected) el.btnDeleteSelected.disabled = count === 0;

  // Check if all visible items are selected
  if (el.bulkSelectAllCheckbox && state.history.currentItems) {
    const allVisibleSelected = state.history.currentItems.length > 0 &&
      state.history.currentItems.every(i => state.history.selectedIds.has(i.id));
    el.bulkSelectAllCheckbox.checked = allVisibleSelected;
  }
}

if (el.btnToggleSelectMode) {
  el.btnToggleSelectMode.addEventListener('click', () => {
    state.history.selectMode = !state.history.selectMode;
    state.history.selectedIds.clear();

    if (state.history.selectMode) {
      el.btnToggleSelectMode.classList.add('active');
      el.historyBulkBar.style.display = 'block';
    } else {
      el.btnToggleSelectMode.classList.remove('active');
      el.historyBulkBar.style.display = 'none';
    }
    loadHistory();
  });
}

if (el.btnCancelSelectMode) {
  el.btnCancelSelectMode.addEventListener('click', () => {
    state.history.selectMode = false;
    state.history.selectedIds.clear();
    if (el.btnToggleSelectMode) el.btnToggleSelectMode.classList.remove('active');
    if (el.historyBulkBar) el.historyBulkBar.style.display = 'none';
    loadHistory();
  });
}

if (el.bulkSelectAllCheckbox) {
  el.bulkSelectAllCheckbox.addEventListener('change', (e) => {
    const check = e.target.checked;
    if (state.history.currentItems) {
      state.history.currentItems.forEach(i => {
        if (check) {
          state.history.selectedIds.add(i.id);
        } else {
          state.history.selectedIds.delete(i.id);
        }
      });
    }
    loadHistory();
  });
}

if (el.btnDeleteSelected) {
  el.btnDeleteSelected.addEventListener('click', async () => {
    const count = state.history.selectedIds.size;
    if (count === 0) return;

    if (confirm(`Are you sure you want to delete ${count} selected scrobbles? This cannot be undone.`)) {
      try {
        const ids = Array.from(state.history.selectedIds);
        await api.deleteBatch(ids);
        state.history.selectedIds.clear();
        state.history.selectMode = false;
        if (el.btnToggleSelectMode) el.btnToggleSelectMode.classList.remove('active');
        if (el.historyBulkBar) el.historyBulkBar.style.display = 'none';
        loadHistory();
        loadTimeline();
        loadStats();
      } catch (err) {
        alert(`Failed to delete selected scrobbles: ${err.message}`);
      }
    }
  });
}

// History Filters & Search
let searchDebounceTimer = null;
el.historySearch.addEventListener('input', (e) => {
  clearTimeout(searchDebounceTimer);
  searchDebounceTimer = setTimeout(() => {
    state.history.search = e.target.value.trim();
    state.history.page = 1;
    loadHistory();
  }, 300);
});

if (el.historySearchAllResults) {
  el.historySearchAllResults.addEventListener('change', (e) => {
    state.history.allResults = e.target.checked;
    if (el.historySearchAllLabel) {
      el.historySearchAllLabel.classList.toggle('active', e.target.checked);
    }
    state.history.page = 1;
    loadHistory();
  });
}

el.pills.forEach(pill => {
  pill.addEventListener('click', () => {
    el.pills.forEach(p => p.classList.remove('active'));
    pill.classList.add('active');
    state.history.mediaType = pill.dataset.type;
    state.history.page = 1;
    loadHistory();
  });
});

if (el.historyRatingFilter) {
  el.historyRatingFilter.addEventListener('change', (e) => {
    state.history.rating = e.target.value;
    if (e.target.value !== 'all') {
      el.historyRatingFilter.classList.add('active');
    } else {
      el.historyRatingFilter.classList.remove('active');
    }
    state.history.page = 1;
    loadHistory();
  });
}

if (el.pillMissingMetadata) {
  el.pillMissingMetadata.addEventListener('click', () => {
    state.history.missingMetadata = !state.history.missingMetadata;
    el.pillMissingMetadata.classList.toggle('active', state.history.missingMetadata);
    state.history.page = 1;
    loadHistory();
  });
}

el.btnPagePrev.addEventListener('click', () => {
  if (state.history.page > 1) {
    state.history.page--;
    loadHistory();
  }
});

el.btnPageNext.addEventListener('click', () => {
  if (state.history.page < state.history.totalPages) {
    state.history.page++;
    loadHistory();
  }
});

// View 3: Stats
async function loadStats() {
  try {
    const params = {
      media_type: state.stats.filter || 'all',
      time_range: state.stats.timeRange || 'all',
    };
    const s = await api.getStats(params);
    state.stats.data = s;
    renderStats();
  } catch (err) {
    console.error('Error loading stats:', err);
  }
}

function renderStats() {
  const s = state.stats.data;
  if (!s) return;

  const filter = state.stats.filter || 'all';
  const timeRange = state.stats.timeRange || 'all';
  const isAll = filter === 'all';
  const isMovie = filter === 'movie';
  const isTv = filter === 'episode';

  // 1. Total Scrobbles Card
  if (isMovie) {
    el.statScrobbles.textContent = (s.movies_watched || 0).toLocaleString();
    if (el.statScrobblesSub) el.statScrobblesSub.textContent = t('stats.total_movies_watched', 'total movie watches');
  } else if (isTv) {
    el.statScrobbles.textContent = (s.episodes_watched || 0).toLocaleString();
    if (el.statScrobblesSub) el.statScrobblesSub.textContent = t('stats.total_episodes_watched', 'total episode watches');
  } else {
    el.statScrobbles.textContent = (s.total_scrobbles || 0).toLocaleString();
    if (el.statScrobblesSub) {
      el.statScrobblesSub.textContent = `${(s.movies_watched || 0).toLocaleString()} ${t('stats.tab_movies').toLowerCase()} · ${(s.episodes_watched || 0).toLocaleString()} ${t('stats.tab_shows').toLowerCase()}`;
    }
  }

  // 2. Time Watched Card
  const totalMins = isMovie
    ? (s.movie_minutes || 0)
    : isTv
    ? (s.episode_minutes || 0)
    : (s.total_minutes || 0);

  const days = Math.floor(totalMins / 1440);
  const remainingAfterDays = totalMins % 1440;
  const hours = Math.floor(remainingAfterDays / 60);
  const mins = remainingAfterDays % 60;

  if (days > 0) {
    el.statHours.textContent = mins > 0 ? `${days}d ${hours}h ${mins}m` : `${days}d ${hours}h`;
  } else {
    el.statHours.textContent = mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
  }
  el.statMinutes.textContent = t('stats.total_mins_watched', { mins: totalMins.toLocaleString() });

  // 3. Movies Card
  el.statMovies.textContent = (s.movies_watched || 0).toLocaleString();
  if (el.statUniqueMovies) {
    el.statUniqueMovies.textContent = t('stats.unique_movies', { count: (s.unique_movies || 0).toLocaleString() });
  }
  if (el.statMovieTime) {
    const mH = Math.floor((s.movie_minutes || 0) / 60);
    el.statMovieTime.textContent = t('stats.movie_runtime', { hours: mH.toLocaleString() });
  }

  // 4. TV Episodes Card
  el.statEpisodes.textContent = (s.episodes_watched || 0).toLocaleString();
  if (el.statUniqueShows) {
    el.statUniqueShows.textContent = t('stats.unique_shows', { count: (s.unique_shows || 0).toLocaleString() });
  }
  if (el.statEpisodeTime) {
    const eH = Math.floor((s.episode_minutes || 0) / 60);
    el.statEpisodeTime.textContent = t('stats.episode_runtime', { hours: eH.toLocaleString() });
  }

  // 5. Watching Streak Card
  el.statStreak.textContent = `${s.current_streak_days || 0}d 🔥`;
  if (el.statStreakSub) {
    el.statStreakSub.textContent = t('stats.longest_streak', { days: s.longest_streak_days || 0 });
  }

  // 6. Daily Pace Card
  if (el.statAvgDaily) {
    el.statAvgDaily.textContent = `${s.avg_daily_minutes || 0}m`;
  }

  // 7. Average Rating Card
  const avgRating = s.average_rating ?? s.avg_rating;
  if (el.statAvgRating) {
    el.statAvgRating.textContent = avgRating ? `${avgRating.toFixed(1)} ★` : '—';
  }
  if (el.statTotalRated) {
    el.statTotalRated.textContent = s.total_rated
      ? t('stats.items_rated', { count: s.total_rated.toLocaleString() })
      : t('stats.no_ratings');
  }

  // 8. Completion Rate Card
  const compRate = s.completion_rate_percent ?? s.completion_rate ?? 0;
  if (el.statCompletionRate) {
    el.statCompletionRate.textContent = `${Math.round(compRate)}%`;
  }

  // 9. Peak Binge Day Card
  if (el.statPeakCount) {
    el.statPeakCount.textContent = (s.peak_day_count || 0) > 0 ? t('stats.items_count', { count: s.peak_day_count }) : '—';
  }
  if (el.statPeakDay) {
    el.statPeakDay.textContent = s.peak_day ? t('stats.on_date', { date: s.peak_day }) : t('stats.no_activity');
  }

  // Toggle Top Shows section
  if (el.topShowsSection) {
    el.topShowsSection.style.display = isMovie ? 'none' : 'block';
  }

  // Render Top Shows
  if (!isMovie && el.topShowsList) {
    if (!s.top_shows || s.top_shows.length === 0) {
      el.topShowsList.innerHTML = `<div class="empty-state" style="padding:15px;"><p>No TV show history recorded yet.</p></div>`;
    } else {
      el.topShowsList.innerHTML = s.top_shows.map((show, idx) => `
        <div class="top-show-item">
          <span class="top-show-rank">#${idx + 1}</span>
          <div class="top-show-thumb">
            ${resolveImageUrl(show.thumb_url)
              ? `<img src="${escapeHtml(resolveImageUrl(show.thumb_url))}" alt="${escapeHtml(show.show_title)}" onerror="this.outerHTML='📺'"/>`
              : '📺'}
          </div>
          <div class="top-show-info">
            <div class="top-show-name" title="${escapeHtml(show.show_title)}">${escapeHtml(show.show_title)}</div>
            <div class="top-show-count">${show.count} ${show.count === 1 ? 'episode' : 'episodes'} watched</div>
          </div>
          <span class="top-show-badge">${show.count} ${show.count === 1 ? 'ep' : 'eps'}</span>
        </div>
      `).join('');
    }
  }

  // Toggle Top Movies section
  if (el.topMoviesSection) {
    el.topMoviesSection.style.display = isTv ? 'none' : 'block';
  }

  // Render Top Movies
  if (!isTv && el.topMoviesList) {
    if (!s.top_movies || s.top_movies.length === 0) {
      el.topMoviesList.innerHTML = `<div class="empty-state" style="padding:15px;"><p>No movie history recorded yet.</p></div>`;
    } else {
      el.topMoviesList.innerHTML = s.top_movies.map((movie, idx) => `
        <div class="top-show-item">
          <span class="top-show-rank">#${idx + 1}</span>
          <div class="top-show-thumb">
            ${resolveImageUrl(movie.thumb_url)
              ? `<img src="${escapeHtml(resolveImageUrl(movie.thumb_url))}" alt="${escapeHtml(movie.title)}" onerror="this.outerHTML='🎬'"/>`
              : '🎬'}
          </div>
          <div class="top-show-info">
            <div class="top-show-name" title="${escapeHtml(movie.title)}${movie.year ? ` (${movie.year})` : ''}">${escapeHtml(movie.title)}${movie.year ? ` (${movie.year})` : ''}</div>
            <div class="top-show-count">${movie.count} ${movie.count === 1 ? 'play' : 'plays'}</div>
          </div>
          <span class="top-show-badge">${movie.count} ${movie.count === 1 ? 'play' : 'plays'}</span>
        </div>
      `).join('');
    }
  }

  // Render Daily Activity Chart
  if (el.activityChart) {
    if (el.activityChartTitle) {
      el.activityChartTitle.textContent = isMovie
        ? 'Daily Movie Activity'
        : isTv
        ? 'Daily TV Episode Activity'
        : 'Daily Watch Activity';
    }
    if (el.activityChartSubtitle) {
      const rangeMap = {
        all: 'Recent activity (14 days)',
        '7d': 'Past 7 days',
        '30d': 'Past 30 days',
        '90d': 'Past 90 days',
        year: 'Past 12 months',
      };
      el.activityChartSubtitle.textContent = rangeMap[timeRange] || 'Activity breakdown';
    }
    if (el.activityChartLegend) {
      if (isMovie) {
        el.activityChartLegend.innerHTML = `<span class="legend-item"><span class="legend-dot dot-movie"></span>Movies</span>`;
      } else if (isTv) {
        el.activityChartLegend.innerHTML = `<span class="legend-item"><span class="legend-dot dot-episode"></span>TV Episodes</span>`;
      } else {
        el.activityChartLegend.innerHTML = `
          <span class="legend-item"><span class="legend-dot dot-movie"></span>Movies</span>
          <span class="legend-item"><span class="legend-dot dot-episode"></span>TV Episodes</span>
        `;
      }
    }

    if (!s.daily_activity || s.daily_activity.length === 0) {
      el.activityChart.innerHTML = `<div class="empty-state" style="padding:20px;width:100%;"><p>No recent activity data.</p></div>`;
    } else {
      const maxTotal = Math.max(...s.daily_activity.map(a => a.total), 1);
      el.activityChart.innerHTML = s.daily_activity.map(item => {
        const totalPct = (item.total / maxTotal) * 100;
        const dateParts = item.date.split('-');
        const shortDate = dateParts.length === 3 ? `${dateParts[1]}/${dateParts[2]}` : item.date;
        const barHeight = Math.max(totalPct, 6);

        let barInner = '';
        if (isMovie) {
          barInner = `<div class="bar-segment-movie" style="height: 100%; border-radius: 4px 4px 0 0;"></div>`;
        } else if (isTv) {
          barInner = `<div class="bar-segment-episode" style="height: 100%; border-radius: 4px 4px 0 0;"></div>`;
        } else {
          barInner = `
            <div class="bar-segment-movie" style="height: ${item.total > 0 ? (item.movies / item.total) * 100 : 0}%;"></div>
            <div class="bar-segment-episode" style="height: ${item.total > 0 ? (item.episodes / item.total) * 100 : 0}%;"></div>
          `;
        }

        const tooltip = isMovie
          ? `${item.date}: ${item.movies} ${item.movies === 1 ? 'movie' : 'movies'}`
          : isTv
          ? `${item.date}: ${item.episodes} ${item.episodes === 1 ? 'episode' : 'episodes'}`
          : `${item.date}: ${item.movies} movies, ${item.episodes} episodes (${item.total} total)`;

        return `
          <div class="activity-bar-group" title="${tooltip}">
            <div class="activity-bar-stacked" style="height: ${item.total > 0 ? barHeight : 4}%;">
              ${item.total > 0 ? barInner : ''}
            </div>
            <span class="activity-bar-label">${shortDate}</span>
          </div>
        `;
      }).join('');
    }
  }

  // Render Day of Week Chart
  if (el.dayOfWeekChart) {
    const dowData = s.day_of_week_activity || s.day_of_week || [];
    if (!dowData || dowData.length === 0) {
      el.dayOfWeekChart.innerHTML = `<div class="empty-state" style="padding:15px;width:100%;"><p>No activity data.</p></div>`;
    } else {
      const maxDow = Math.max(...dowData.map(d => d.count), 1);
      el.dayOfWeekChart.innerHTML = dowData.map(d => {
        const pct = Math.max(Math.round((d.count / maxDow) * 100), d.count > 0 ? 8 : 2);
        const isPeak = d.count === maxDow && maxDow > 0;
        return `
          <div class="dow-bar-group" title="${d.day}: ${d.count} ${d.count === 1 ? 'watch' : 'watches'}">
            <div class="dow-bar-wrap">
              <div class="dow-bar-fill ${isPeak ? 'peak' : ''}" style="height: ${pct}%;"></div>
            </div>
            <span class="dow-bar-label">${d.day}</span>
          </div>
        `;
      }).join('');
    }
  }

  // Render Time of Day Chart
  if (el.timeOfDayChart) {
    const todData = s.time_of_day_activity || s.time_of_day || [];
    if (!todData || todData.length === 0) {
      el.timeOfDayChart.innerHTML = `<div class="empty-state" style="padding:15px;"><p>No activity data.</p></div>`;
    } else {
      const icons = {
        Morning: '🌅',
        Afternoon: '☀️',
        Evening: '🌆',
        Night: '🌙',
      };
      el.timeOfDayChart.innerHTML = todData.map(t => {
        const icon = icons[t.period] || '🕒';
        const label = t.label || t.period;
        return `
          <div class="tod-item">
            <div class="tod-header">
              <div class="tod-period-name">
                <span>${icon}</span>
                <span>${escapeHtml(label)}</span>
              </div>
              <div class="tod-period-stat">${t.count} watches (${t.percentage}%)</div>
            </div>
            <div class="tod-track">
              <div class="tod-fill ${t.period.toLowerCase()}" style="width: ${t.percentage}%;"></div>
            </div>
          </div>
        `;
      }).join('');
    }
  }

  // Render Top Genres Breakdown
  if (el.genresChart) {
    if (el.genresChartTitle) {
      el.genresChartTitle.textContent = isMovie
        ? 'Top Movie Genres'
        : isTv
        ? 'Top TV Genres'
        : 'Top Genres';
    }
    if (el.genresChartSubtitle) {
      el.genresChartSubtitle.textContent = isMovie
        ? 'Most watched film categories'
        : isTv
        ? 'Most watched television categories'
        : 'Most watched media categories';
    }

    if (!s.top_genres || s.top_genres.length === 0) {
      el.genresChart.innerHTML = `<div class="empty-state" style="padding:20px;"><p>No genre data available yet.</p></div>`;
    } else {
      const maxGenre = Math.max(...s.top_genres.map(g => g.count), 1);
      el.genresChart.innerHTML = s.top_genres.map(g => {
        const pct = Math.round((g.count / maxGenre) * 100);
        return `
          <div class="genre-bar-item">
            <div class="genre-bar-header">
              <span>${escapeHtml(g.genre)}</span>
              <span style="color:var(--text-dim);">${g.count} watches</span>
            </div>
            <div class="genre-bar-track">
              <div class="genre-bar-fill" style="width: ${pct}%;"></div>
            </div>
          </div>
        `;
      }).join('');
    }
  }

  // Render Top Actors
  if (el.topActorsList) {
    if (!s.top_actors || s.top_actors.length === 0) {
      el.topActorsList.innerHTML = `<div class="empty-state" style="padding:15px;"><p>No actor data recorded yet.</p></div>`;
    } else {
      el.topActorsList.innerHTML = s.top_actors.map((actor, idx) => `
        <div class="top-person-row">
          <div class="top-person-left">
            <span class="top-person-rank">#${idx + 1}</span>
            <span class="top-person-name">${escapeHtml(actor.name)}</span>
          </div>
          <span class="top-person-count">${actor.count} ${actor.count === 1 ? 'watch' : 'watches'}</span>
        </div>
      `).join('');
    }
  }

  // Render Top Directors
  if (el.topDirectorsList) {
    if (!s.top_directors || s.top_directors.length === 0) {
      el.topDirectorsList.innerHTML = `<div class="empty-state" style="padding:15px;"><p>No director data recorded yet.</p></div>`;
    } else {
      el.topDirectorsList.innerHTML = s.top_directors.map((director, idx) => `
        <div class="top-person-row">
          <div class="top-person-left">
            <span class="top-person-rank">#${idx + 1}</span>
            <span class="top-person-name">${escapeHtml(director.name)}</span>
          </div>
          <span class="top-person-count">${director.count} ${director.count === 1 ? 'watch' : 'watches'}</span>
        </div>
      `).join('');
    }
  }

  // Render Weekly Viewing Heatmap (7x24)
  renderWeeklyHeatmap(s.weekly_heatmap);
}

// Stats media type filter pills event listeners (All, Movies, TV Shows)
if (el.statsFilterPills) {
  el.statsFilterPills.forEach(pill => {
    pill.addEventListener('click', () => {
      el.statsFilterPills.forEach(p => p.classList.remove('active'));
      pill.classList.add('active');
      state.stats.filter = pill.dataset.type;
      loadStats();
    });
  });
}

// Stats time range pills event listeners (All Time, 7d, 30d, 90d, Year)
if (el.statsRangePills) {
  el.statsRangePills.forEach(pill => {
    pill.addEventListener('click', () => {
      el.statsRangePills.forEach(p => p.classList.remove('active'));
      pill.classList.add('active');
      state.stats.timeRange = pill.dataset.range;
      loadStats();
    });
  });
}

// Now Playing
async function loadNowPlaying() {
  if (!state.user) return;
  try {
    const res = await api.getNowPlaying();
    if (res && res.now_playing) {
      const p = res.now_playing;
      el.nowPlayingBanner.style.display = 'flex';
      el.nowPlayingTitle.textContent = p.media_type === 'episode' && p.grandparent_title
        ? `${p.grandparent_title} - ${p.title}`
        : p.title;

      el.nowPlayingDevice.textContent = p.player_title ? t('now_playing.on_device', { device: p.player_title }) : '';
      el.nowPlayingState.textContent = p.state === 'paused' ? t('now_playing.paused') : t('now_playing.badge');

      if (p.duration_ms > 0) {
        const pct = Math.min(100, Math.round((p.view_offset_ms / p.duration_ms) * 100));
        el.nowPlayingProgress.style.width = `${pct}%`;
      } else {
        el.nowPlayingProgress.style.width = '50%';
      }
    } else {
      el.nowPlayingBanner.style.display = 'none';
    }
  } catch (err) {
    el.nowPlayingBanner.style.display = 'none';
  }
}

// View 4: Webhook Info
async function loadWebhookInfo() {
  try {
    const info = await api.getWebhookInfo();
    el.webhookUrlInput.value = info.webhook_url;
    if (el.jellyfinWebhookUrlInput) {
      el.jellyfinWebhookUrlInput.value = info.jellyfin_webhook_url || info.webhook_url.replace('/webhooks/plex/', '/webhooks/jellyfin/');
    }
    if (el.embyWebhookUrlInput) {
      el.embyWebhookUrlInput.value = info.emby_webhook_url || info.webhook_url.replace('/webhooks/plex/', '/webhooks/emby/');
    }
  } catch (err) {
    console.error('Error loading webhook info:', err);
  }

  // Load Metadata integration status (TheTVDB or TMDB)
  const tvdbBadge = document.getElementById('tvdb-status-badge');
  const tvdbText = document.getElementById('tvdb-status-text');
  const tvdbDot = document.getElementById('tvdb-pulse-dot');
  const cardTitle = document.getElementById('settings-metadata-card-title');
  const cardDesc = document.getElementById('settings-metadata-card-desc');
  const cardAttribution = document.getElementById('settings-metadata-attribution-note');

  if (tvdbBadge && tvdbText) {
    try {
      const integrations = await api.getIntegrationsStatus();
      const isTmdb = integrations && integrations.metadata_source === 'tmdb';
      if (isTmdb) {
        if (cardTitle) cardTitle.textContent = 'The Movie Database (TMDB) Metadata Integration';
        if (cardDesc) cardDesc.innerHTML = 'BingeSync automatically enriches your scrobble timeline in the background with genres, cast/actor details, director credits, and plot summaries retrieved from <a href="https://www.themoviedb.org/" target="_blank" rel="noopener noreferrer" class="tmdb-inline-link">The Movie Database (TMDB)</a>.';
        if (cardAttribution) cardAttribution.innerHTML = 'Media metadata, plot outlines, genres, and cast details are provided by <a href="https://www.themoviedb.org/" target="_blank" rel="noopener noreferrer" class="tmdb-inline-link"><strong>The Movie Database (TMDB)</strong></a>. This product uses the TMDB API but is not endorsed or certified by TMDB.';

        if (integrations.tmdb_configured) {
          tvdbText.textContent = 'The Movie Database (TMDB) Connected & Active (Background Enrichment Enabled)';
          tvdbBadge.className = 'tvdb-status-badge tvdb-status-active';
          if (tvdbDot) tvdbDot.className = 'pulse-dot-sm pulse-active';
        } else {
          tvdbText.textContent = 'TMDB API Key Not Configured (Set TMDB_API_KEY to enable)';
          tvdbBadge.className = 'tvdb-status-badge tvdb-status-inactive';
          if (tvdbDot) tvdbDot.className = 'pulse-dot-sm pulse-inactive';
        }
      } else {
        if (cardTitle) cardTitle.textContent = 'TheTVDB Metadata Integration';
        if (cardDesc) cardDesc.innerHTML = 'BingeSync automatically enriches your scrobble timeline in the background with genres, cast/actor details, director credits, and plot summaries retrieved from <a href="https://thetvdb.com/" target="_blank" rel="noopener noreferrer" class="tvdb-inline-link">TheTVDB</a>.';
        if (cardAttribution) cardAttribution.innerHTML = 'Media metadata, plot outlines, genres, and cast details are provided by <a href="https://thetvdb.com/" target="_blank" rel="noopener noreferrer" class="tvdb-inline-link"><strong>TheTVDB</strong></a>. This product uses TheTVDB API but is not certified or endorsed by TheTVDB.';

        if (integrations && integrations.tvdb_configured) {
          tvdbText.textContent = 'TheTVDB Connected & Active (Background Enrichment Enabled)';
          tvdbBadge.className = 'tvdb-status-badge tvdb-status-active';
          if (tvdbDot) tvdbDot.className = 'pulse-dot-sm pulse-active';
        } else {
          tvdbText.textContent = 'TheTVDB API Key Not Configured (Set TVDB_API_KEY to enable)';
          tvdbBadge.className = 'tvdb-status-badge tvdb-status-inactive';
          if (tvdbDot) tvdbDot.className = 'pulse-dot-sm pulse-inactive';
        }
      }
    } catch (e) {
      tvdbText.textContent = 'Metadata Integration Active';
    }
  }
}

el.btnCopyWebhook.addEventListener('click', async () => {
  try {
    await navigator.clipboard.writeText(el.webhookUrlInput.value);
    el.copyStatus.style.display = 'block';
    setTimeout(() => {
      el.copyStatus.style.display = 'none';
    }, 2500);
  } catch (err) {
    el.webhookUrlInput.select();
    document.execCommand('copy');
    el.copyStatus.style.display = 'block';
    setTimeout(() => {
      el.copyStatus.style.display = 'none';
    }, 2500);
  }
});

if (el.btnCopyJellyfinWebhook && el.jellyfinWebhookUrlInput) {
  el.btnCopyJellyfinWebhook.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(el.jellyfinWebhookUrlInput.value);
      if (el.jellyfinCopyStatus) {
        el.jellyfinCopyStatus.style.display = 'block';
        setTimeout(() => {
          if (el.jellyfinCopyStatus) el.jellyfinCopyStatus.style.display = 'none';
        }, 2500);
      }
    } catch (err) {
      el.jellyfinWebhookUrlInput.select();
      document.execCommand('copy');
      if (el.jellyfinCopyStatus) {
        el.jellyfinCopyStatus.style.display = 'block';
        setTimeout(() => {
          if (el.jellyfinCopyStatus) el.jellyfinCopyStatus.style.display = 'none';
        }, 2500);
      }
    }
  });
}

if (el.btnCopyEmbyWebhook && el.embyWebhookUrlInput) {
  el.btnCopyEmbyWebhook.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(el.embyWebhookUrlInput.value);
      if (el.embyCopyStatus) {
        el.embyCopyStatus.style.display = 'block';
        setTimeout(() => {
          if (el.embyCopyStatus) el.embyCopyStatus.style.display = 'none';
        }, 2500);
      }
    } catch (err) {
      el.embyWebhookUrlInput.select();
      document.execCommand('copy');
      if (el.embyCopyStatus) {
        el.embyCopyStatus.style.display = 'block';
        setTimeout(() => {
          if (el.embyCopyStatus) el.embyCopyStatus.style.display = 'none';
        }, 2500);
      }
    }
  });
}

el.btnRegenToken.addEventListener('click', async () => {
  if (confirm('Are you sure you want to regenerate your webhook token? You will need to update the webhook URLs in Plex, Jellyfin, and Emby.')) {
    try {
      const res = await api.regenerateWebhookToken();
      el.webhookUrlInput.value = res.webhook_url;
      if (el.jellyfinWebhookUrlInput) {
        el.jellyfinWebhookUrlInput.value = res.jellyfin_webhook_url || res.webhook_url.replace('/webhooks/plex/', '/webhooks/jellyfin/');
      }
      if (el.embyWebhookUrlInput) {
        el.embyWebhookUrlInput.value = res.emby_webhook_url || res.webhook_url.replace('/webhooks/plex/', '/webhooks/emby/');
      }
      alert('Webhook token regenerated successfully!');
    } catch (err) {
      alert(`Error regenerating token: ${err.message}`);
    }
  }
});

// User Settings (Connected Services: Plex, Jellyfin, Emby, Radarr, Sonarr, Tautulli, Overseerr, Outbound Webhooks)
async function loadUserSettings() {
  if (!el.formUserSettings) return;
  try {
    const s = await api.getUserSettings();
    if (el.userSettingLanguage) el.userSettingLanguage.value = getLanguage();
    if (el.userSettingPlexUsername) el.userSettingPlexUsername.value = s.plex_username || '';
    if (el.userSettingJellyfinUsername) el.userSettingJellyfinUsername.value = s.jellyfin_username || '';
    if (el.userSettingEmbyUsername) el.userSettingEmbyUsername.value = s.emby_username || '';

    if (el.userSettingJellyfinUrl) el.userSettingJellyfinUrl.value = s.jellyfin_url || '';
    applySecretField(el.userSettingJellyfinApiKey, s.jellyfin_api_key, s.jellyfin_api_key_masked);

    if (el.userSettingRadarrUrl) el.userSettingRadarrUrl.value = s.radarr_url || '';
    applySecretField(el.userSettingRadarrApiKey, s.radarr_api_key, s.radarr_api_key_masked);

    if (el.userSettingSonarrUrl) el.userSettingSonarrUrl.value = s.sonarr_url || '';
    applySecretField(el.userSettingSonarrApiKey, s.sonarr_api_key, s.sonarr_api_key_masked);

    if (el.userSettingTautulliUrl) el.userSettingTautulliUrl.value = s.tautulli_url || '';
    applySecretField(el.userSettingTautulliApiKey, s.tautulli_api_key, s.tautulli_api_key_masked);

    if (el.userSettingOverseerrUrl) el.userSettingOverseerrUrl.value = s.overseerr_url || '';
    applySecretField(el.userSettingOverseerrApiKey, s.overseerr_api_key, s.overseerr_api_key_masked);

    if (el.userSettingOutboundType) el.userSettingOutboundType.value = s.outbound_webhook_type || '';
    if (el.userSettingOutboundUrl) el.userSettingOutboundUrl.value = s.outbound_webhook_url || '';
  } catch (err) {
    console.error('Failed to load user settings:', err);
    if (el.userSettingsStatus) {
      el.userSettingsStatus.style.color = 'var(--accent-red)';
      el.userSettingsStatus.textContent = 'Failed to load settings: ' + err.message;
    }
  }
}

if (el.formUserSettings) {
  el.formUserSettings.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (el.btnSaveUserSettings) {
      el.btnSaveUserSettings.disabled = true;
      el.btnSaveUserSettings.textContent = 'Saving...';
    }
    if (el.userSettingsStatus) el.userSettingsStatus.textContent = '';

    try {
      const payload = {
        plex_username: el.userSettingPlexUsername ? el.userSettingPlexUsername.value.trim() : null,
        jellyfin_username: el.userSettingJellyfinUsername ? el.userSettingJellyfinUsername.value.trim() : null,
        emby_username: el.userSettingEmbyUsername ? el.userSettingEmbyUsername.value.trim() : null,
        jellyfin_url: el.userSettingJellyfinUrl ? el.userSettingJellyfinUrl.value.trim() : null,
        jellyfin_api_key: getSecretValue(el.userSettingJellyfinApiKey),
        radarr_url: el.userSettingRadarrUrl ? el.userSettingRadarrUrl.value.trim() : null,
        radarr_api_key: getSecretValue(el.userSettingRadarrApiKey),
        sonarr_url: el.userSettingSonarrUrl ? el.userSettingSonarrUrl.value.trim() : null,
        sonarr_api_key: getSecretValue(el.userSettingSonarrApiKey),
        tautulli_url: el.userSettingTautulliUrl ? el.userSettingTautulliUrl.value.trim() : null,
        tautulli_api_key: getSecretValue(el.userSettingTautulliApiKey),
        overseerr_url: el.userSettingOverseerrUrl ? el.userSettingOverseerrUrl.value.trim() : null,
        overseerr_api_key: getSecretValue(el.userSettingOverseerrApiKey),
        outbound_webhook_type: el.userSettingOutboundType ? el.userSettingOutboundType.value : null,
        outbound_webhook_url: el.userSettingOutboundUrl ? el.userSettingOutboundUrl.value.trim() : null,
      };

      await api.updateUserSettings(payload);
      await loadUserSettings();
      checkOverseerrStatus();
      if (el.userSettingsStatus) {
        el.userSettingsStatus.style.color = '#34d399';
        el.userSettingsStatus.textContent = 'Settings saved successfully!';
        setTimeout(() => {
          if (el.userSettingsStatus) el.userSettingsStatus.textContent = '';
        }, 4000);
      }
    } catch (err) {
      console.error('Failed to save user settings:', err);
      if (el.userSettingsStatus) {
        el.userSettingsStatus.style.color = 'var(--accent-red)';
        el.userSettingsStatus.textContent = 'Error: ' + err.message;
      }
    } finally {
      if (el.btnSaveUserSettings) {
        el.btnSaveUserSettings.disabled = false;
        el.btnSaveUserSettings.textContent = 'Save Settings';
      }
    }
  });
}

if (el.btnTestOutboundWebhook) {
  el.btnTestOutboundWebhook.addEventListener('click', async () => {
    const url = el.userSettingOutboundUrl ? el.userSettingOutboundUrl.value.trim() : '';
    const type = el.userSettingOutboundType ? el.userSettingOutboundType.value : '';
    if (!url) {
      if (el.outboundWebhookTestStatus) {
        el.outboundWebhookTestStatus.style.display = 'block';
        el.outboundWebhookTestStatus.style.color = 'var(--accent-red)';
        el.outboundWebhookTestStatus.textContent = 'Please enter a webhook URL first.';
      }
      return;
    }

    el.btnTestOutboundWebhook.disabled = true;
    if (el.outboundWebhookTestStatus) {
      el.outboundWebhookTestStatus.style.display = 'block';
      el.outboundWebhookTestStatus.style.color = 'var(--text-muted)';
      el.outboundWebhookTestStatus.textContent = 'Sending test notification...';
    }

    try {
      await api.testOutboundWebhook({ webhook_url: url, webhook_type: type || 'generic' });
      if (el.outboundWebhookTestStatus) {
        el.outboundWebhookTestStatus.style.color = '#34d399';
        el.outboundWebhookTestStatus.textContent = 'Test notification sent successfully!';
        setTimeout(() => {
          if (el.outboundWebhookTestStatus) el.outboundWebhookTestStatus.style.display = 'none';
        }, 5000);
      }
    } catch (err) {
      if (el.outboundWebhookTestStatus) {
        el.outboundWebhookTestStatus.style.color = 'var(--accent-red)';
        el.outboundWebhookTestStatus.textContent = 'Failed: ' + err.message;
      }
    } finally {
      el.btnTestOutboundWebhook.disabled = false;
    }
  });
}

if (el.btnExportUserData) {
  el.btnExportUserData.addEventListener('click', async () => {
    el.btnExportUserData.disabled = true;
    if (el.exportUserDataStatus) {
      el.exportUserDataStatus.textContent = 'Preparing your export archive...';
      el.exportUserDataStatus.style.color = 'var(--text-muted)';
    }
    try {
      await api.downloadUserData();
      if (el.exportUserDataStatus) {
        el.exportUserDataStatus.textContent = 'Export downloaded successfully!';
        el.exportUserDataStatus.style.color = '#34d399';
        setTimeout(() => {
          if (el.exportUserDataStatus) el.exportUserDataStatus.textContent = '';
        }, 4000);
      }
    } catch (err) {
      console.error('Data export error:', err);
      if (el.exportUserDataStatus) {
        el.exportUserDataStatus.textContent = 'Failed to download export: ' + err.message;
        el.exportUserDataStatus.style.color = 'var(--accent-red)';
      }
    } finally {
      el.btnExportUserData.disabled = false;
    }
  });
}

if (el.linkGotoUserSettings) {
  el.linkGotoUserSettings.addEventListener('click', (e) => {
    e.preventDefault();
    const modalImport = document.getElementById('modal-import-history');
    if (modalImport) modalImport.style.display = 'none';
    switchTab('view-settings');
  });
}

// Dev Login Submission
el.formDevLogin.addEventListener('submit', async (e) => {
  e.preventDefault();
  const email = el.devEmail.value.trim();
  const name = el.devName.value.trim();
  try {
    const res = await api.devLogin(email, name);
    onUserAuthenticated(res.user);
  } catch (err) {
    alert(`Login failed: ${err.message}`);
  }
});

// Logout
el.btnLogout.addEventListener('click', async () => {
  await api.logout();
  state.user = null;
  if (state.nowPlayingTimer) clearInterval(state.nowPlayingTimer);
  showAuthState();
});

// Refresh Timeline Button
el.btnRefreshTimeline.addEventListener('click', () => {
  loadTimeline();
  loadNowPlaying();
});

// Modal: Manual Scrobble
el.btnOpenLogModal.addEventListener('click', () => {
  el.formManualScrobble.reset();
  updateModalMediaTypeFields();
  el.modalLogScrobble.style.display = 'flex';
});

el.btnCloseLogModal.addEventListener('click', () => {
  el.modalLogScrobble.style.display = 'none';
});

el.btnCancelLogModal.addEventListener('click', () => {
  el.modalLogScrobble.style.display = 'none';
});

el.scrobbleMediaType.addEventListener('change', updateModalMediaTypeFields);

function updateModalMediaTypeFields() {
  const isEp = el.scrobbleMediaType.value === 'episode';
  el.groupGrandparent.style.display = isEp ? 'block' : 'none';
  el.groupEpisodeNums.style.display = isEp ? 'flex' : 'none';
  el.labelScrobbleTitle.textContent = isEp ? 'Episode Title' : 'Movie Title';
}

el.formManualScrobble.addEventListener('submit', async (e) => {
  e.preventDefault();
  const isEp = el.scrobbleMediaType.value === 'episode';
  const durationMin = parseInt(document.getElementById('scrobble-duration').value, 10);
  const year = parseInt(document.getElementById('scrobble-year').value, 10);
  const ratingVal = el.scrobbleRating ? el.scrobbleRating.value : '';
  const user_rating = ratingVal ? parseInt(ratingVal, 10) : null;

  const payload = {
    title: document.getElementById('scrobble-title').value.trim(),
    media_type: el.scrobbleMediaType.value,
    grandparent_title: isEp ? document.getElementById('scrobble-show').value.trim() : null,
    parent_title: isEp ? `Season ${document.getElementById('scrobble-season').value || 1}` : null,
    season_number: isEp ? parseInt(document.getElementById('scrobble-season').value, 10) || 1 : null,
    episode_number: isEp ? parseInt(document.getElementById('scrobble-episode').value, 10) || 1 : null,
    year: isNaN(year) ? null : year,
    duration_ms: isNaN(durationMin) ? null : durationMin * 60000,
    user_rating: user_rating,
    overview: document.getElementById('scrobble-overview').value.trim() || null,
  };

  const directorVal = document.getElementById('scrobble-director')?.value?.trim();
  const cinematographerVal = document.getElementById('scrobble-cinematographer')?.value?.trim();
  if (directorVal) {
    payload.director = directorVal;
    payload.directors = directorVal;
  }
  if (cinematographerVal) {
    payload.cinematographer = cinematographerVal;
  }

  try {
    await api.createScrobble(payload);
    el.modalLogScrobble.style.display = 'none';
    loadTimeline();
    loadStats();
    loadHistory();
  } catch (err) {
    alert(`Failed to save scrobble: ${err.message}`);
  }
});

// Modal: Unified Watch History Import (Trakt.tv & Tautulli)
function switchImportTab(source) {
  if (source === 'tautulli') {
    if (el.btnImportTabTrakt) el.btnImportTabTrakt.classList.remove('active');
    if (el.btnImportTabTautulli) el.btnImportTabTautulli.classList.add('active');
    if (el.importPaneTrakt) el.importPaneTrakt.style.display = 'none';
    if (el.importPaneTautulli) el.importPaneTautulli.style.display = 'block';
    if (el.tautulliImportStatus) el.tautulliImportStatus.style.display = 'none';
    if (el.tautulliImportUsername && !el.tautulliImportUsername.value) {
      el.tautulliImportUsername.value = state.user?.preferred_username || state.user?.display_name || (state.user?.email ? state.user.email.split('@')[0] : '');
    }
  } else {
    if (el.btnImportTabTrakt) el.btnImportTabTrakt.classList.add('active');
    if (el.btnImportTabTautulli) el.btnImportTabTautulli.classList.remove('active');
    if (el.importPaneTrakt) el.importPaneTrakt.style.display = 'block';
    if (el.importPaneTautulli) el.importPaneTautulli.style.display = 'none';
    if (el.importStatusBox) el.importStatusBox.style.display = 'none';
  }
}

if (el.btnImportTabTrakt) {
  el.btnImportTabTrakt.addEventListener('click', () => switchImportTab('trakt'));
}

if (el.btnImportTabTautulli) {
  el.btnImportTabTautulli.addEventListener('click', () => switchImportTab('tautulli'));
}

if (el.btnOpenImportModal) {
  el.btnOpenImportModal.addEventListener('click', () => {
    switchImportTab('trakt');
    if (el.formImportTrakt) el.formImportTrakt.reset();
    if (el.formImportTautulli) el.formImportTautulli.reset();
    if (el.importStatusBox) {
      el.importStatusBox.style.display = 'none';
      el.importStatusBox.className = 'import-status-box';
    }
    if (el.tautulliImportStatus) {
      el.tautulliImportStatus.style.display = 'none';
      el.tautulliImportStatus.className = 'import-status-box';
    }
    if (el.tautulliImportUsername) {
      el.tautulliImportUsername.value = state.user?.preferred_username || state.user?.display_name || (state.user?.email ? state.user.email.split('@')[0] : '');
    }
    if (el.modalImportHistory) el.modalImportHistory.style.display = 'flex';
  });
}

if (el.btnOpenTautulliModal) {
  el.btnOpenTautulliModal.addEventListener('click', () => {
    switchImportTab('tautulli');
    if (el.modalImportHistory) el.modalImportHistory.style.display = 'flex';
  });
}

if (el.btnCloseImportModal) {
  el.btnCloseImportModal.addEventListener('click', () => {
    if (el.modalImportHistory) el.modalImportHistory.style.display = 'none';
  });
}

if (el.btnCancelImportModal) {
  el.btnCancelImportModal.addEventListener('click', () => {
    if (el.modalImportHistory) el.modalImportHistory.style.display = 'none';
  });
}

if (el.btnCancelTautulliModal) {
  el.btnCancelTautulliModal.addEventListener('click', () => {
    if (el.modalImportHistory) el.modalImportHistory.style.display = 'none';
  });
}

if (el.formImportTrakt) {
  el.formImportTrakt.addEventListener('submit', async (e) => {
    e.preventDefault();
    el.importStatusBox.style.display = 'block';
    el.importStatusBox.className = 'import-status-box';
    el.importStatusBox.textContent = 'Parsing and importing Trakt history...';

    let content = el.importPasteInput ? el.importPasteInput.value.trim() : '';
    const file = el.importFileInput && el.importFileInput.files && el.importFileInput.files[0];
    let isZip = false;

    if (file) {
      const isZipFile = file.name.toLowerCase().endsWith('.zip') || file.type.includes('zip');
      try {
        if (isZipFile) {
          content = await file.arrayBuffer();
          isZip = true;
        } else {
          content = await file.text();
        }
      } catch (err) {
        el.importStatusBox.className = 'import-status-box error';
        el.importStatusBox.textContent = `Failed to read file: ${err.message}`;
        return;
      }
    }

    if (!content || (content instanceof ArrayBuffer && content.byteLength === 0)) {
      el.importStatusBox.className = 'import-status-box error';
      el.importStatusBox.textContent = 'Please choose a file or paste your Trakt data.';
      return;
    }

    try {
      const res = await api.importTrakt(content, isZip);
      el.importStatusBox.className = 'import-status-box success';
      el.importStatusBox.innerHTML = `
        <strong>Import Complete!</strong><br>
        ${escapeHtml(res.message)}<br>
        <small style="color:var(--text-dim);">Imported: ${res.imported} | Updated ratings: ${res.updated} | Skipped: ${res.skipped}</small>
      `;
      loadTimeline();
      loadStats();
      loadHistory();
    } catch (err) {
      el.importStatusBox.className = 'import-status-box error';
      el.importStatusBox.textContent = `Import failed: ${err.message}`;
    }
  });
}

// Fetch users from Tautulli API
if (el.btnFetchTautulliUsers) {
  el.btnFetchTautulliUsers.addEventListener('click', async () => {
    el.btnFetchTautulliUsers.disabled = true;
    el.btnFetchTautulliUsers.textContent = 'Fetching...';

    try {
      const res = await api.getTautulliUsers();
      if (res && res.users && res.users.length > 0) {
        el.tautulliUsersSelect.innerHTML = '<option value="">-- Select a Tautulli user --</option>';
        const currentTarget = (el.tautulliImportUsername.value || '').toLowerCase();
        let matched = false;

        res.users.forEach(u => {
          const opt = document.createElement('option');
          opt.value = u.username;
          opt.textContent = `${u.friendly_name || u.username} (${u.username})`;
          if (!matched && (u.username.toLowerCase() === currentTarget || (u.friendly_name && u.friendly_name.toLowerCase() === currentTarget))) {
            opt.selected = true;
            matched = true;
            el.tautulliImportUsername.value = u.username;
          }
          el.tautulliUsersSelect.appendChild(opt);
        });

        el.tautulliUsersSelect.style.display = 'block';
      } else {
        alert('No users returned by Tautulli server.');
      }
    } catch (err) {
      alert(`Failed to fetch Tautulli users: ${err.message}`);
    } finally {
      el.btnFetchTautulliUsers.disabled = false;
      el.btnFetchTautulliUsers.textContent = 'Fetch Users';
    }
  });
}

if (el.tautulliUsersSelect) {
  el.tautulliUsersSelect.addEventListener('change', (e) => {
    if (e.target.value && el.tautulliImportUsername) {
      el.tautulliImportUsername.value = e.target.value;
    }
  });
}

let isTautulliSubmitting = false;
if (el.formImportTautulli) {
  el.formImportTautulli.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (isTautulliSubmitting) return;
    isTautulliSubmitting = true;

    if (el.tautulliImportStatus) {
      el.tautulliImportStatus.style.display = 'block';
      el.tautulliImportStatus.className = 'import-status-box';
      el.tautulliImportStatus.textContent = 'Importing Tautulli history...';
    }
    if (el.btnSubmitTautulliImport) el.btnSubmitTautulliImport.disabled = true;

    const username = el.tautulliImportUsername ? el.tautulliImportUsername.value.trim() : '';
    const file = el.tautulliFileInput && el.tautulliFileInput.files && el.tautulliFileInput.files[0];
    let content = el.tautulliPasteInput ? el.tautulliPasteInput.value.trim() : '';

    try {
      if (file) {
        content = await file.text();
      }

      const payload = {
        username: username || null,
        content: content || null,
      };

      const res = await api.importTautulli(payload);
      if (el.tautulliImportStatus) {
        el.tautulliImportStatus.className = 'import-status-box success';
        el.tautulliImportStatus.innerHTML = `
          <strong>Import Complete!</strong><br>
          ${escapeHtml(res.message || 'Imported Tautulli history.')}<br>
          <small style="color:var(--text-dim);">Imported: ${res.imported} | Skipped duplicates: ${res.skipped}</small>
        `;
      }
      loadTimeline();
      loadStats();
      loadHistory();
    } catch (err) {
      if (el.tautulliImportStatus) {
        el.tautulliImportStatus.className = 'import-status-box error';
        el.tautulliImportStatus.textContent = `Import failed: ${err.message}`;
      }
    } finally {
      isTautulliSubmitting = false;
      if (el.btnSubmitTautulliImport) el.btnSubmitTautulliImport.disabled = false;
    }
  });
}

// Modal: Detail View
function formatPersonLinks(rawString, role = 'person') {
  if (!rawString || !rawString.trim()) return '';
  const list = rawString
    .split(',')
    .map(s => s.trim())
    .filter(Boolean);
  if (list.length === 0) return '';
  return list
    .map(name => `
      <button type="button" class="person-tag" data-person="${escapeHtml(name)}" data-role="${escapeHtml(role)}" title="Show other watches featuring ${escapeHtml(name)}">
        <span class="person-tag-name">${escapeHtml(name)}</span>
        <span class="person-tag-icon" title="View in History tab">↗</span>
      </button>
    `)
    .join(' ');
}

async function loadPersonWatches(personName, currentItemId) {
  const container = document.getElementById('detail-person-watches-container');
  if (!container) return;
  container.style.display = 'block';
  container.innerHTML = `
    <div class="watches-section-header">
      <div class="watches-heading">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"></path>
          <circle cx="12" cy="7" r="4"></circle>
        </svg>
        <span>Watches featuring "${escapeHtml(personName)}"</span>
      </div>
      <span class="watches-badge-loading">Searching history...</span>
    </div>
  `;
  try {
    const res = await api.getHistory({ person: personName, limit: 50, all: true, all_dates: true });
    const items = res && res.items ? res.items : [];
    const totalCount = items.length;

    let contentHtml = '';
    if (items.length === 0) {
      contentHtml = `<div class="watches-empty-state"><span>No other history items found featuring "${escapeHtml(personName)}".</span></div>`;
    } else {
      contentHtml = `
        <div class="watches-table-wrap">
          <table class="watches-table">
            <thead>
              <tr>
                <th>Title</th>
                <th>Type</th>
                <th>Watched At</th>
                <th>Rating</th>
                <th>Status</th>
              </tr>
            </thead>
            <tbody>
              ${items.map(it => {
                const isCurrent = it.id === currentItemId;
                const dateObj = new Date(it.watched_at);
                const dateStr = !isNaN(dateObj) ? dateObj.toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' }) : it.watched_at;
                const ratingStr = it.user_rating ? `<span class="watch-star">★ ${it.user_rating}/10</span>` : '&mdash;';
                const statusStr = it.watch_status ? escapeHtml(it.watch_status) : 'completed';
                const typeBadge = it.media_type === 'episode' ? 'badge-episode' : 'badge-movie';
                const typeText = it.media_type === 'episode' ? (it.grandparent_title || 'Episode') : 'Movie';

                return `
                  <tr class="other-watch-row person-history-item ${isCurrent ? 'current-item' : ''}" data-id="${escapeHtml(it.id)}" style="cursor:pointer;" title="Click to view details">
                    <td>
                      <div style="display:flex;align-items:center;gap:8px;">
                        <span style="font-weight:600;color:#fff;">${escapeHtml(it.title)}</span>
                        ${isCurrent ? '<span class="status-pill status-pill-active" style="font-size:0.7rem;padding:1px 6px;">Current</span>' : ''}
                      </div>
                      ${it.grandparent_title ? `<div style="font-size:0.75rem;color:var(--text-dim);">${escapeHtml(it.grandparent_title)}</div>` : ''}
                    </td>
                    <td><span class="scrobble-badge ${typeBadge}" style="font-size:0.7rem;">${escapeHtml(typeText)}</span></td>
                    <td><div class="other-watch-date">${escapeHtml(dateStr)}</div></td>
                    <td>${ratingStr}</td>
                    <td><span class="watch-status-tag ${escapeHtml(statusStr)}">${escapeHtml(statusStr)}</span></td>
                  </tr>
                `;
              }).join('')}
            </tbody>
          </table>
        </div>
      `;
    }

    container.innerHTML = `
      <div class="watches-section-header" style="justify-content:space-between;flex-wrap:wrap;gap:8px;">
        <div class="watches-heading">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"></path>
            <circle cx="12" cy="7" r="4"></circle>
          </svg>
          <span>Watches featuring "${escapeHtml(personName)}"</span>
          <span class="watches-count-pill">${totalCount} item${totalCount === 1 ? '' : 's'}</span>
        </div>
        <div style="display:flex;gap:6px;">
          <button type="button" class="btn btn-outline btn-xs btn-open-history-filtered" data-person="${escapeHtml(personName)}" title="Filter full History tab by this person">
            View in History Tab ↗
          </button>
          <button type="button" class="btn btn-ghost btn-xs btn-close-person-watches" title="Close this list">✕</button>
        </div>
      </div>
      ${contentHtml}
    `;

    // Click on row to open that item's detail
    container.querySelectorAll('.person-history-item').forEach(row => {
      row.addEventListener('click', () => {
        const id = row.dataset.id;
        const target = items.find(i => i.id === id);
        if (target) {
          openDetailModal(target);
        }
      });
    });

    const btnClose = container.querySelector('.btn-close-person-watches');
    if (btnClose) {
      btnClose.addEventListener('click', () => {
        container.style.display = 'none';
      });
    }

    const btnOpenHistory = container.querySelector('.btn-open-history-filtered');
    if (btnOpenHistory) {
      btnOpenHistory.addEventListener('click', () => {
        filterHistoryByPerson(personName);
      });
    }
  } catch (err) {
    container.innerHTML = `<div class="watches-empty-state"><span style="color:var(--accent-red);">Error loading watches: ${escapeHtml(err.message)}</span></div>`;
  }
}

async function openDetailModal(item) {
  state.selectedItem = item;
  el.detailTitle.textContent = item.title;

  const isEpisode = item.media_type === 'episode';
  const showTitle = isEpisode && item.grandparent_title ? item.grandparent_title : null;
  const sub = showTitle
    ? `<div style="color:var(--text-muted);font-weight:600;margin-bottom:12px;">${escapeHtml(showTitle)} &bull; S${item.season_number || 1}E${item.episode_number || 1}</div>`
    : '';

  const durationStr = item.duration_ms ? `${Math.round(item.duration_ms / 60000)} minutes` : 'Unknown duration';
  const yearStr = item.year ? `${item.year}` : 'Unknown year';
  const watchedStr = new Date(item.watched_at).toLocaleString();
  const ratingBadge = `<div id="detail-rating-display" style="font-size:0.85rem;color:var(--text-dim);">${
    item.user_rating
      ? `Rating: <strong style="color:#fbbf24;">★ ${item.user_rating}/10</strong>`
      : `Rating: <span style="color:var(--text-dim);">Unrated</span>`
  }</div>`;

  const genresBadge = item.genres
    ? `<div style="font-size:0.85rem;color:var(--text-dim);">Genres: <strong style="color:#fff;">${escapeHtml(item.genres)}</strong></div>`
    : '';
  const directorVal = item.director || item.directors;
  const directorBadge = directorVal
    ? `<div class="detail-person-row" style="font-size:0.85rem;color:var(--text-dim);display:flex;align-items:center;flex-wrap:wrap;gap:6px;margin-top:2px;"><span>Director:</span> <div class="person-tag-list">${formatPersonLinks(directorVal, 'director')}</div></div>`
    : '';
  const cinematographerBadge = item.cinematographer
    ? `<div class="detail-person-row" style="font-size:0.85rem;color:var(--text-dim);display:flex;align-items:center;flex-wrap:wrap;gap:6px;margin-top:2px;"><span>Cinematographer:</span> <div class="person-tag-list">${formatPersonLinks(item.cinematographer, 'cinematographer')}</div></div>`
    : '';
  const actorsBadge = item.actors
    ? `<div class="detail-person-row" style="font-size:0.85rem;color:var(--text-dim);display:flex;align-items:center;flex-wrap:wrap;gap:6px;margin-top:2px;"><span>Cast:</span> <div class="person-tag-list">${formatPersonLinks(item.actors, 'actor')}</div></div>`
    : '';

  const resolvedThumb = resolveImageUrl(item.thumb_url);
  const posterImg = resolvedThumb
    ? `<img src="${escapeHtml(resolvedThumb)}" alt="${escapeHtml(item.title)}" style="width:105px;height:155px;object-fit:cover;border-radius:8px;flex-shrink:0;box-shadow:0 4px 12px rgba(0,0,0,0.5);border:1px solid rgba(255,255,255,0.08);margin-bottom:12px;"/>`
    : '';

  const statusBadge = item.watch_status === 'in_progress'
    ? `<div style="font-size:0.85rem;color:var(--text-dim);">Status: <strong style="color:#fbbf24;">In Progress (${item.progress_percent || 0}%)</strong></div>`
    : `<div style="font-size:0.85rem;color:var(--text-dim);">Status: <strong style="color:#34d399;">Completed</strong></div>`;

  el.detailBody.innerHTML = `
    <div style="display:flex;gap:18px;align-items:flex-start;margin-bottom:16px;">
      ${posterImg}
      <div style="flex:1;min-width:0;">
        ${sub}
        <div style="display:flex;gap:12px;margin-bottom:12px;flex-wrap:wrap;">
          <div style="font-size:0.85rem;color:var(--text-dim);">Release: <strong style="color:#fff;">${yearStr}</strong></div>
          <div style="font-size:0.85rem;color:var(--text-dim);">Duration: <strong style="color:#fff;">${durationStr}</strong></div>
          <div style="font-size:0.85rem;color:var(--text-dim);">Watched: <strong style="color:#fff;">${watchedStr}</strong></div>
          ${statusBadge}
          ${ratingBadge}
          ${genresBadge}
          ${directorBadge}
          ${cinematographerBadge}
          ${actorsBadge}
        </div>
        ${item.overview ? `<p style="font-size:0.92rem;line-height:1.6;color:var(--text-main);">${escapeHtml(item.overview)}</p>` : '<p style="color:var(--text-dim);font-style:italic;">No plot overview available.</p>'}
      </div>
    </div>
    <div class="detail-rating-widget" style="margin:14px 0;padding:12px 14px;background:rgba(255,255,255,0.03);border:1px solid rgba(255,255,255,0.08);border-radius:8px;display:flex;align-items:center;justify-content:space-between;flex-wrap:wrap;gap:10px;">
      <div style="display:flex;align-items:center;gap:10px;">
        <span style="font-size:0.88rem;font-weight:600;color:var(--text-main);">Your Rating:</span>
        <select id="detail-rating-select" class="form-input" style="width:145px;padding:5px 8px;font-size:0.85rem;background:#0d1525;border:1px solid var(--border-color);border-radius:6px;color:#fff;">
          <option value="">No rating</option>
          <option value="10">★ 10 (Masterpiece)</option>
          <option value="9">★ 9 (Great)</option>
          <option value="8">★ 8 (Very Good)</option>
          <option value="7">★ 7 (Good)</option>
          <option value="6">★ 6 (Fair)</option>
          <option value="5">★ 5 (Average)</option>
          <option value="4">★ 4 (Below Avg)</option>
          <option value="3">★ 3 (Bad)</option>
          <option value="2">★ 2 (Terrible)</option>
          <option value="1">★ 1 (Appalling)</option>
        </select>
        <button type="button" class="btn btn-primary btn-sm" id="btn-save-detail-rating" style="padding:5px 12px;font-size:0.82rem;">Save Rating</button>
      </div>
      <span id="detail-rating-feedback" style="font-size:0.82rem;font-weight:500;display:none;"></span>
    </div>

    <!-- Personal Tags & Notes Widget -->
    <div class="detail-user-data-widget" style="margin:14px 0;padding:12px 14px;background:rgba(255,255,255,0.03);border:1px solid rgba(255,255,255,0.08);border-radius:8px;">
      <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:8px;">
        <span style="font-size:0.88rem;font-weight:600;color:var(--text-main);display:flex;align-items:center;gap:6px;">
          🏷️ Tags & Review Notes
        </span>
        <button type="button" class="btn btn-primary btn-sm" id="btn-save-detail-user-data" style="padding:4px 12px;font-size:0.82rem;">Save Notes & Tags</button>
      </div>
      <div style="margin-bottom:8px;">
        <label for="detail-tags-input" style="font-size:0.75rem;color:var(--text-dim);display:block;margin-bottom:4px;font-weight:600;text-transform:uppercase;">Personal Tags (comma-separated)</label>
        <input type="text" id="detail-tags-input" class="form-control" placeholder="e.g. Rewatch, Plane Movie, Watched with Daryl" value="${escapeHtml(item.tags || '')}" style="font-size:0.85rem;padding:6px 10px;" />
        <div id="detail-tags-preview" style="display:flex;flex-wrap:wrap;gap:5px;margin-top:6px;">
          ${(item.tags || '').split(',').map(t => t.trim()).filter(Boolean).map(t => `<button type="button" class="tag-chip-card" data-tag="${escapeHtml(t)}" title="Filter by tag: ${escapeHtml(t)}">🏷️ ${escapeHtml(t)}</button>`).join('')}
        </div>
      </div>
      <div>
        <label for="detail-notes-input" style="font-size:0.75rem;color:var(--text-dim);display:block;margin-bottom:4px;font-weight:600;text-transform:uppercase;">Review Notes</label>
        <textarea id="detail-notes-input" class="form-control" rows="2" placeholder="Write thoughts, review notes, or memorable moments..." style="font-size:0.85rem;padding:6px 10px;resize:vertical;">${escapeHtml(item.notes || '')}</textarea>
      </div>
      <span id="detail-user-data-feedback" style="font-size:0.82rem;font-weight:500;display:none;margin-top:6px;"></span>
    </div>
    <div id="detail-fix-match-box" class="fix-match-box" style="display:none;margin:14px 0;padding:12px 14px;background:rgba(59,130,246,0.06);border:1px solid rgba(59,130,246,0.25);border-radius:8px;">
      <div style="font-weight:600;font-size:0.88rem;color:#93c5fd;margin-bottom:6px;">${t('modal.fix_match_title')}</div>
      <p style="font-size:0.82rem;color:var(--text-dim);margin-bottom:10px;">${t('modal.fix_match_desc')}</p>
      <div style="display:flex;gap:8px;align-items:center;flex-wrap:wrap;">
        <select id="select-fix-match-provider" class="form-control" style="width:auto;min-width:130px;padding:6px 8px;font-size:0.82rem;">
          <option value="auto">Auto (${state.adminSettings?.metadata_source === 'tmdb' ? 'TMDB' : 'TheTVDB'})</option>
          <option value="tmdb">The Movie Database (TMDB)</option>
          <option value="tvdb">TheTVDB</option>
        </select>
        <input type="text" id="input-fix-match-tvdb-id" class="form-control font-mono" placeholder="${t('modal.fix_match_placeholder')}" value="${escapeHtml(item.tmdb_id || item.tvdb_id || (item.media_type === 'episode' ? (item.grandparent_title || item.title) : item.title))}" style="flex:1;min-width:180px;padding:6px 10px;font-size:0.85rem;" />
        <button type="button" class="btn btn-secondary btn-sm" id="btn-search-fix-match" style="padding:6px 12px;font-size:0.82rem;">🔍 ${t('modal.fix_match_search') || 'Search'}</button>
        <button type="button" class="btn btn-primary btn-sm" id="btn-submit-fix-match" style="padding:6px 12px;font-size:0.82rem;">${t('modal.fix_match_apply')}</button>
        <button type="button" class="btn btn-outline btn-sm" id="btn-cancel-fix-match" style="padding:6px 10px;font-size:0.82rem;">${t('modal.cancel')}</button>
      </div>
      <div id="fix-match-feedback" style="margin-top:8px;font-size:0.82rem;display:none;"></div>
      <div id="fix-match-results" class="fix-match-results" style="display:none;margin-top:10px;max-height:280px;overflow-y:auto;flex-direction:column;gap:8px;"></div>
    </div>
    <div id="detail-arr-container"></div>
    <div id="detail-person-watches-container" class="person-watches-section" style="display:none;"></div>
    <div id="detail-watches-container" class="detail-watches-section"></div>
    <div style="display:flex;align-items:center;justify-content:space-between;font-size:0.78rem;font-family:monospace;color:var(--text-dim);background:#080c14;padding:8px 12px;border-radius:6px;margin-top:14px;flex-wrap:wrap;gap:8px;">
      <span>Scrobble ID: ${item.id}</span>
      ${item.tvdb_id ? `<span style="color:#60a5fa;">TheTVDB ID: ${escapeHtml(item.tvdb_id)}</span>` : ''}
      ${item.tmdb_id ? `<span style="color:#34d399;">TMDB ID: ${escapeHtml(item.tmdb_id)}</span>` : ''}
      <span class="detail-tvdb-attribution">Metadata & cast via <a href="https://thetvdb.com" target="_blank" rel="noopener noreferrer">TheTVDB</a></span>
    </div>
  `;

  const ratingSelect = document.getElementById('detail-rating-select');
  if (ratingSelect) {
    ratingSelect.value = item.user_rating ? String(item.user_rating) : '';
  }

  const btnSaveRating = document.getElementById('btn-save-detail-rating');
  if (btnSaveRating) {
    btnSaveRating.onclick = async () => {
      const val = ratingSelect ? ratingSelect.value : '';
      const numRating = val ? parseInt(val, 10) : null;
      const feedback = document.getElementById('detail-rating-feedback');
      btnSaveRating.disabled = true;
      try {
        await api.rateScrobble(item.id, numRating);
        item.user_rating = numRating;
        const disp = document.getElementById('detail-rating-display');
        if (disp) {
          disp.innerHTML = numRating
            ? `Rating: <strong style="color:#fbbf24;">★ ${numRating}/10</strong>`
            : `Rating: <span style="color:var(--text-dim);">Unrated</span>`;
        }
        if (feedback) {
          feedback.textContent = numRating ? `Rating saved (★ ${numRating}/10)` : 'Rating cleared';
          feedback.style.color = '#34d399';
          feedback.style.display = 'inline';
        }
        loadTimeline();
        loadHistory();
        loadStats();
      } catch (err) {
        if (feedback) {
          feedback.textContent = `Error: ${err.message}`;
          feedback.style.color = '#ef4444';
          feedback.style.display = 'inline';
        }
      } finally {
        btnSaveRating.disabled = false;
      }
    };
  }

  // Tags and notes click / save handlers
  const prevTags = document.getElementById('detail-tags-preview');
  if (prevTags) {
    prevTags.querySelectorAll('.tag-chip-card').forEach(tc => {
      tc.addEventListener('click', (e) => {
        e.stopPropagation();
        const tag = tc.dataset.tag;
        if (tag) filterHistoryByTag(tag);
      });
    });
  }

  const btnSaveUserData = document.getElementById('btn-save-detail-user-data');
  if (btnSaveUserData) {
    btnSaveUserData.onclick = async () => {
      const tagsVal = document.getElementById('detail-tags-input')?.value?.trim() || null;
      const notesVal = document.getElementById('detail-notes-input')?.value?.trim() || null;
      const feedback = document.getElementById('detail-user-data-feedback');
      btnSaveUserData.disabled = true;
      try {
        await api.updateScrobbleUserData(item.id, { tags: tagsVal, notes: notesVal });
        item.tags = tagsVal;
        item.notes = notesVal;
        if (feedback) {
          feedback.textContent = 'Tags & notes saved successfully';
          feedback.style.color = '#34d399';
          feedback.style.display = 'block';
        }
        const prevContainer = document.getElementById('detail-tags-preview');
        if (prevContainer) {
          prevContainer.innerHTML = (tagsVal || '').split(',').map(t => t.trim()).filter(Boolean).map(t => `<button type="button" class="tag-chip-card" data-tag="${escapeHtml(t)}" title="Filter by tag: ${escapeHtml(t)}">🏷️ ${escapeHtml(t)}</button>`).join('');
          prevContainer.querySelectorAll('.tag-chip-card').forEach(tc => {
            tc.addEventListener('click', (e) => {
              e.stopPropagation();
              const tag = tc.dataset.tag;
              if (tag) filterHistoryByTag(tag);
            });
          });
        }
        loadTimeline();
        loadHistory();
      } catch (err) {
        if (feedback) {
          feedback.textContent = `Error: ${err.message}`;
          feedback.style.color = '#ef4444';
          feedback.style.display = 'block';
        }
      } finally {
        btnSaveUserData.disabled = false;
      }
    };
  }

  const btnCancelFixMatch = document.getElementById('btn-cancel-fix-match');
  if (btnCancelFixMatch) {
    btnCancelFixMatch.onclick = () => {
      const box = document.getElementById('detail-fix-match-box');
      if (box) box.style.display = 'none';
    };
  }

  const btnSearchFixMatch = document.getElementById('btn-search-fix-match');
  const inpFixMatch = document.getElementById('input-fix-match-tvdb-id');
  const selProvider = document.getElementById('select-fix-match-provider');
  const resultsContainer = document.getElementById('fix-match-results');
  const fixFeedback = document.getElementById('fix-match-feedback');

  const applyFixMatchAction = async (targetId, prov) => {
    const btnSubmit = document.getElementById('btn-submit-fix-match');
    if (!targetId) {
      if (fixFeedback) {
        fixFeedback.textContent = 'Please enter a valid title or ID';
        fixFeedback.style.color = '#ef4444';
        fixFeedback.style.display = 'block';
      }
      return;
    }

    if (btnSubmit) btnSubmit.disabled = true;
    if (fixFeedback) {
      fixFeedback.textContent = `Fixing match and downloading metadata (${prov || 'auto'})...`;
      fixFeedback.style.color = '#60a5fa';
      fixFeedback.style.display = 'block';
    }

    try {
      const res = await api.fixMatch(item.id, { id: targetId, provider: prov });
      if (fixFeedback) {
        fixFeedback.textContent = 'Match updated successfully!';
        fixFeedback.style.color = '#34d399';
      }
      if (res && res.scrobble) {
        state.selectedItem = res.scrobble;
        await openDetailModal(res.scrobble);
      }
      loadTimeline();
      loadHistory();
    } catch (err) {
      if (fixFeedback) {
        fixFeedback.textContent = `Error: ${err.message}`;
        fixFeedback.style.color = '#ef4444';
      }
    } finally {
      if (btnSubmit) btnSubmit.disabled = false;
    }
  };

  const doSearchAction = async () => {
    const query = inpFixMatch ? inpFixMatch.value.trim() : '';
    const prov = selProvider ? selProvider.value : 'auto';
    if (!query) {
      if (fixFeedback) {
        fixFeedback.textContent = 'Please enter a title or ID to search';
        fixFeedback.style.color = '#ef4444';
        fixFeedback.style.display = 'block';
      }
      return;
    }

    if (btnSearchFixMatch) btnSearchFixMatch.disabled = true;
    if (fixFeedback) {
      fixFeedback.textContent = `Searching for "${query}"...`;
      fixFeedback.style.color = '#60a5fa';
      fixFeedback.style.display = 'block';
    }
    if (resultsContainer) {
      resultsContainer.style.display = 'none';
      resultsContainer.innerHTML = '';
    }

    try {
      const res = await api.searchMatch(item.id, query, prov);
      const candidates = (res && res.results) || [];
      if (btnSearchFixMatch) btnSearchFixMatch.disabled = false;

      if (candidates.length === 0) {
        if (fixFeedback) {
          fixFeedback.textContent = `No matching titles found for "${query}". You can also enter an ID directly and click "${t('modal.fix_match_apply')}".`;
          fixFeedback.style.color = '#f59e0b';
          fixFeedback.style.display = 'block';
        }
        return;
      }

      if (fixFeedback) {
        fixFeedback.textContent = `Found ${candidates.length} candidate(s):`;
        fixFeedback.style.color = '#34d399';
        fixFeedback.style.display = 'block';
      }

      if (resultsContainer) {
        resultsContainer.innerHTML = candidates.map(c => {
          const poster = c.image_url
            ? `<img src="${escapeHtml(c.image_url)}" alt="${escapeHtml(c.name || '')}" style="width:38px;height:54px;object-fit:cover;border-radius:4px;flex-shrink:0;" onerror="this.outerHTML='<div style=\\\'width:38px;height:54px;background:rgba(255,255,255,0.06);border-radius:4px;display:flex;align-items:center;justify-content:center;font-size:1.1rem;flex-shrink:0;\\\'>🎬</div>'" />`
            : `<div style="width:38px;height:54px;background:rgba(255,255,255,0.06);border-radius:4px;display:flex;align-items:center;justify-content:center;font-size:1.1rem;flex-shrink:0;">🎬</div>`;
          const provBadge = c.provider === 'tmdb'
            ? `<span class="badge" style="background:rgba(16,185,129,0.18);color:#34d399;font-size:0.72rem;padding:2px 6px;border-radius:4px;">TMDB</span>`
            : `<span class="badge" style="background:rgba(59,130,246,0.18);color:#60a5fa;font-size:0.72rem;padding:2px 6px;border-radius:4px;">TheTVDB</span>`;
          const yearStr = c.year ? `(${c.year})` : '';
          const typeStr = c.entity_type ? `<span style="font-size:0.72rem;color:var(--text-dim);text-transform:capitalize;">${escapeHtml(c.entity_type)}</span>` : '';
          const overviewText = c.overview
            ? escapeHtml(c.overview.slice(0, 110)) + (c.overview.length > 110 ? '...' : '')
            : '<span style="color:var(--text-dim);font-style:italic;">No overview available</span>';

          return `
            <div class="fix-match-candidate-row" style="display:flex;align-items:center;gap:10px;padding:8px 10px;background:rgba(0,0,0,0.35);border:1px solid rgba(255,255,255,0.08);border-radius:6px;">
              ${poster}
              <div style="flex:1;min-width:0;">
                <div style="display:flex;align-items:center;gap:6px;flex-wrap:wrap;">
                  <strong style="font-size:0.86rem;color:var(--text-main);">${escapeHtml(c.name || 'Untitled')}</strong>
                  <span style="font-size:0.8rem;color:var(--text-muted);">${yearStr}</span>
                  ${provBadge}
                  ${typeStr}
                  <span class="font-mono" style="font-size:0.75rem;color:var(--text-dim);">#${escapeHtml(c.id)}</span>
                </div>
                <div style="font-size:0.77rem;color:var(--text-muted);margin-top:2px;overflow:hidden;text-overflow:ellipsis;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;">
                  ${overviewText}
                </div>
              </div>
              <button type="button" class="btn btn-primary btn-sm btn-select-candidate-match" data-id="${escapeHtml(c.id)}" data-provider="${escapeHtml(c.provider || prov)}" style="padding:4px 10px;font-size:0.78rem;white-space:nowrap;flex-shrink:0;">
                ${t('modal.fix_match_select') || 'Match'}
              </button>
            </div>
          `;
        }).join('');

        resultsContainer.style.display = 'flex';

        resultsContainer.querySelectorAll('.btn-select-candidate-match').forEach(btn => {
          btn.addEventListener('click', async (e) => {
            e.stopPropagation();
            const tid = btn.dataset.id;
            const tprov = btn.dataset.provider;
            await applyFixMatchAction(tid, tprov);
          });
        });
      }
    } catch (err) {
      if (btnSearchFixMatch) btnSearchFixMatch.disabled = false;
      if (fixFeedback) {
        fixFeedback.textContent = `Search error: ${err.message}`;
        fixFeedback.style.color = '#ef4444';
        fixFeedback.style.display = 'block';
      }
    }
  };

  if (btnSearchFixMatch) {
    btnSearchFixMatch.onclick = doSearchAction;
  }

  if (inpFixMatch) {
    inpFixMatch.onkeydown = (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        const val = inpFixMatch.value.trim();
        if (/^\d+$/.test(val) || /^(tmdb|tvdb|series|movie)-\d+$/i.test(val)) {
          applyFixMatchAction(val, selProvider ? selProvider.value : 'auto');
        } else {
          doSearchAction();
        }
      }
    };
  }

  const btnSubmitFixMatch = document.getElementById('btn-submit-fix-match');
  if (btnSubmitFixMatch) {
    btnSubmitFixMatch.onclick = async () => {
      const val = inpFixMatch ? inpFixMatch.value.trim() : '';
      const prov = selProvider ? selProvider.value : 'auto';
      if (/^\d+$/.test(val) || /^(tmdb|tvdb|series|movie)-\d+$/i.test(val)) {
        await applyFixMatchAction(val, prov);
      } else {
        await doSearchAction();
      }
    };
  }

  // IMDb button setup
  if (el.detailImdbLink) {
    el.detailImdbLink.href = getImdbUrl(item);
    el.detailImdbLink.style.display = 'inline-flex';
  }

  // TVDB button setup
  if (el.detailTvdbLink) {
    el.detailTvdbLink.href = getTvdbUrl(item);
    el.detailTvdbLink.style.display = 'inline-flex';
  }

  // TMDB button setup
  if (el.detailTmdbLink) {
    el.detailTmdbLink.href = getTmdbUrl(item);
    el.detailTmdbLink.style.display = 'inline-flex';
  }

  // Delete show scrobbles button setup
  if (el.btnDeleteShowScrobbles) {
    if (showTitle) {
      el.btnDeleteShowScrobbles.style.display = 'inline-block';
      el.btnDeleteShowScrobbles.textContent = `Delete Show (${showTitle})`;
      el.btnDeleteShowScrobbles.onclick = async () => {
        if (confirm(`Delete ALL scrobbles for "${showTitle}"? This cannot be undone.`)) {
          try {
            await api.deleteByShow(showTitle);
            el.modalItemDetail.style.display = 'none';
            loadTimeline();
            loadHistory();
            loadStats();
          } catch (err) {
            alert(`Failed to delete scrobbles for show: ${err.message}`);
          }
        }
      };
    } else {
      el.btnDeleteShowScrobbles.style.display = 'none';
    }
  }

  // Person tag click listeners
  el.detailBody.querySelectorAll('.person-tag').forEach(tag => {
    tag.addEventListener('click', (e) => {
      e.stopPropagation();
      const personName = tag.dataset.person;
      if (!personName) return;

      if (e.target.closest('.person-tag-icon') || e.shiftKey) {
        filterHistoryByPerson(personName);
        return;
      }

      loadPersonWatches(personName, item.id);
    });
  });

  el.modalItemDetail.style.display = 'flex';

  // Check Radarr / Sonarr Library integration
  checkLibraryIntegrations(item);

  // Load all other watches of this same item (exact episode for TV shows)
  loadItemWatches(item);
}

async function checkLibraryIntegrations(item) {
  const container = document.getElementById('detail-arr-container');
  if (!container) return;

  try {
    const res = await api.checkMediaInLibrary({
      title: item.title,
      media_type: item.media_type,
      year: item.year,
      grandparent_title: item.grandparent_title,
    });

    if (res && res.found) {
      const isRadarr = res.service === 'radarr';
      const showTitle = res.title || item.grandparent_title || item.title;

      if (isRadarr) {
        container.innerHTML = `
          <div class="arr-cleanup-card">
            <div class="arr-info">
              <span class="arr-badge arr-badge-radarr">Found in Radarr</span>
              <span class="arr-title">${escapeHtml(res.title || item.title)}</span>
              <span class="arr-sub">${res.has_file ? 'Media files on disk & monitored' : 'Monitored in library'}</span>
            </div>
            <button class="btn btn-danger btn-sm" id="btn-arr-cleanup">
              Clean Up & Delete Files
            </button>
          </div>
        `;

        const cleanupBtn = document.getElementById('btn-arr-cleanup');
        if (cleanupBtn) {
          cleanupBtn.addEventListener('click', async () => {
            if (confirm(`Remove "${res.title || item.title}" from Radarr and delete all media files from disk?`)) {
              try {
                await api.deleteFromRadarr(res.movie_id, true);
                container.innerHTML = `
                  <div style="font-size:0.85rem;color:#10b981;padding:8px 0;">
                    ✓ Successfully removed from Radarr and deleted disk files.
                  </div>
                `;
              } catch (err) {
                alert(`Failed to clean up from Radarr: ${err.message}`);
              }
            }
          });
        }
      } else {
        const hasSeason = item.season_number != null;
        const seasonNum = hasSeason ? item.season_number : 1;

        container.innerHTML = `
          <div class="arr-cleanup-card arr-cleanup-card-sonarr">
            <div class="arr-info">
              <div style="display:flex;align-items:center;gap:8px;">
                <span class="arr-badge arr-badge-sonarr">Found in Sonarr</span>
                <span class="arr-title">${escapeHtml(showTitle)}</span>
              </div>
              <span class="arr-sub">Sonarr cleanup options:</span>
            </div>
            <div class="arr-sonarr-actions">
              <div class="arr-select-group">
                <select id="sonarr-action-select" class="form-control form-control-sm arr-select">
                  <option value="delete_series">Delete show and files</option>
                  <option value="delete_season"${hasSeason ? ' selected' : ''}>Delete and unmonitor season ${hasSeason ? seasonNum + ' ' : ''}files</option>
                  <option value="delete_all_files_unmonitor">Delete all files and unmonitor all seasons</option>
                </select>
                <button class="btn btn-danger btn-sm" id="btn-arr-cleanup">
                  Clean Up
                </button>
              </div>
            </div>
          </div>
        `;

        const cleanupBtn = document.getElementById('btn-arr-cleanup');
        const selectAction = document.getElementById('sonarr-action-select');

        if (cleanupBtn && selectAction) {
          cleanupBtn.addEventListener('click', async () => {
            const action = selectAction.value;
            let confirmMsg = '';
            let targetSeason = seasonNum;

            if (action === 'delete_series') {
              confirmMsg = `Remove "${showTitle}" from Sonarr and delete all series media files from disk?`;
            } else if (action === 'delete_season') {
              if (!hasSeason) {
                const inputSeason = prompt(`Enter season number to delete and unmonitor:`, '1');
                if (!inputSeason) return;
                targetSeason = parseInt(inputSeason, 10);
                if (isNaN(targetSeason)) {
                  alert('Invalid season number');
                  return;
                }
              }
              confirmMsg = `Delete all episode files for Season ${targetSeason} of "${showTitle}" from disk and unmonitor Season ${targetSeason} in Sonarr?`;
            } else if (action === 'delete_all_files_unmonitor') {
              confirmMsg = `Delete ALL episode files across ALL seasons of "${showTitle}" from disk and unmonitor all seasons in Sonarr?`;
            }

            if (!confirm(confirmMsg)) return;

            try {
              cleanupBtn.disabled = true;
              cleanupBtn.textContent = 'Processing...';

              const deleteRes = await api.deleteFromSonarr(res.series_id, {
                action: action,
                season_number: action === 'delete_season' ? targetSeason : undefined,
                delete_files: true,
              });

              const msg = deleteRes && deleteRes.message ? deleteRes.message : 'Successfully executed Sonarr cleanup.';
              container.innerHTML = `
                <div style="font-size:0.85rem;color:#10b981;padding:8px 0;font-weight:500;">
                  ✓ ${escapeHtml(msg)}
                </div>
              `;
            } catch (err) {
              cleanupBtn.disabled = false;
              cleanupBtn.textContent = 'Clean Up';
              alert(`Failed to clean up from Sonarr: ${err.message}`);
            }
          });
        }
      }
    }
  } catch (err) {
    // Non-fatal if integrations are not configured
  }
}

async function loadItemWatches(item) {
  const container = document.getElementById('detail-watches-container');
  if (!container) return;

  container.innerHTML = `
    <div class="watches-section-header">
      <div class="watches-heading">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <circle cx="12" cy="12" r="10"></circle>
          <polyline points="12 6 12 12 16 14"></polyline>
        </svg>
        <span>Watch History</span>
      </div>
      <span class="watches-badge-loading">Checking other watches...</span>
    </div>
  `;

  try {
    const res = await api.getItemWatches(item.id);
    if (!res || !container) return;
    if (state.selectedItem?.id !== item.id) return;

    const count = res.other_watches ? res.other_watches.length : 0;
    const total = res.total_watches || (count + 1);
    const isEpisode = item.media_type === 'episode';
    const mediaLabel = isEpisode ? 'this episode' : 'this film';

    let contentHtml = '';
    if (count === 0) {
      contentHtml = `
        <div class="watches-empty-state">
          <span class="watches-check-icon">✓</span>
          <span>Only recorded watch for ${mediaLabel}.</span>
        </div>
      `;
    } else {
      contentHtml = `
        <div class="watches-table-wrap">
          <table class="watches-table">
            <thead>
              <tr>
                <th>Watched At</th>
                <th>Duration</th>
                <th>Rating</th>
                <th>Status</th>
                <th style="text-align:right;">Action</th>
              </tr>
            </thead>
            <tbody>
              ${res.other_watches.map(w => {
                const dateObj = new Date(w.watched_at);
                const dateStr = !isNaN(dateObj) ? dateObj.toLocaleString(undefined, {
                  year: 'numeric',
                  month: 'short',
                  day: 'numeric',
                  hour: '2-digit',
                  minute: '2-digit'
                }) : w.watched_at;
                const durStr = w.duration_ms ? `${Math.round(w.duration_ms / 60000)}m` : '&mdash;';
                const ratingStr = w.user_rating ? `<span class="watch-star">★ ${w.user_rating}/10</span>` : '&mdash;';
                const statusStr = w.watch_status ? escapeHtml(w.watch_status) : 'completed';

                return `
                  <tr class="other-watch-row" data-id="${escapeHtml(w.id)}">
                    <td>
                      <div class="other-watch-date">${escapeHtml(dateStr)}</div>
                    </td>
                    <td>${durStr}</td>
                    <td>${ratingStr}</td>
                    <td><span class="watch-status-tag ${escapeHtml(statusStr)}">${escapeHtml(statusStr)}</span></td>
                    <td style="text-align:right;">
                      <button class="btn btn-outline btn-xs text-danger btn-delete-other-watch" data-id="${escapeHtml(w.id)}" data-date="${escapeHtml(dateStr)}" title="Delete this watch record">
                        Delete
                      </button>
                    </td>
                  </tr>
                `;
              }).join('')}
            </tbody>
          </table>
        </div>
      `;
    }

    container.innerHTML = `
      <div class="watches-section-header">
        <div class="watches-heading">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <circle cx="12" cy="12" r="10"></circle>
            <polyline points="12 6 12 12 16 14"></polyline>
          </svg>
          <span>Other Watches</span>
          <span class="watches-count-pill">${count} other${count === 1 ? '' : 's'} (${total} total)</span>
        </div>
      </div>
      ${contentHtml}
    `;

    container.querySelectorAll('.btn-delete-other-watch').forEach(btn => {
      btn.addEventListener('click', async (e) => {
        e.stopPropagation();
        const scrobbleId = btn.dataset.id;
        const watchDate = btn.dataset.date;
        if (confirm(`Delete watch from ${watchDate}? This cannot be undone.`)) {
          try {
            btn.disabled = true;
            btn.textContent = 'Deleting...';
            await api.deleteScrobble(scrobbleId);
            await loadItemWatches(item);
            loadTimeline();
            loadHistory();
            loadStats();
          } catch (err) {
            alert(`Failed to delete watch: ${err.message}`);
            btn.disabled = false;
            btn.textContent = 'Delete';
          }
        }
      });
    });
  } catch (err) {
    console.error('Error loading other watches:', err);
    container.innerHTML = `
      <div class="watches-section-header">
        <div class="watches-heading">
          <span>Other Watches</span>
        </div>
      </div>
      <div class="watches-error" style="font-size:0.8rem;color:var(--text-dim);font-style:italic;">Could not load other watches for this item.</div>
    `;
  }
}

el.btnCloseDetailModal.addEventListener('click', () => {
  el.modalItemDetail.style.display = 'none';
});

el.btnDismissDetailModal.addEventListener('click', () => {
  el.modalItemDetail.style.display = 'none';
});

el.btnDeleteDetailItem.addEventListener('click', async () => {
  if (state.selectedItem && confirm('Delete this scrobble from your history?')) {
    try {
      await api.deleteScrobble(state.selectedItem.id);
      el.modalItemDetail.style.display = 'none';
      loadTimeline();
      loadHistory();
      loadStats();
    } catch (err) {
      alert(`Failed to delete scrobble: ${err.message}`);
    }
  }
});

if (el.btnRefreshDetailMetadata) {
  el.btnRefreshDetailMetadata.addEventListener('click', async () => {
    if (!state.selectedItem) return;
    const originalText = el.btnRefreshDetailMetadata.innerHTML;
    el.btnRefreshDetailMetadata.disabled = true;
    el.btnRefreshDetailMetadata.textContent = 'Refreshing...';
    try {
      const res = await api.refreshScrobbleMetadata(state.selectedItem.id);
      if (res && res.scrobble) {
        state.selectedItem = res.scrobble;
        await openDetailModal(res.scrobble);
      }
      loadTimeline();
      loadHistory();
    } catch (err) {
      alert(`Failed to refresh metadata: ${err.message}`);
    } finally {
      el.btnRefreshDetailMetadata.disabled = false;
      el.btnRefreshDetailMetadata.innerHTML = originalText;
    }
  });
}

if (el.btnFixMatch) {
  el.btnFixMatch.addEventListener('click', () => {
    const box = document.getElementById('detail-fix-match-box');
    if (!box) return;
    const isHidden = box.style.display === 'none' || !box.style.display;
    box.style.display = isHidden ? 'block' : 'none';
    if (isHidden) {
      const inp = document.getElementById('input-fix-match-tvdb-id');
      if (inp) {
        inp.focus();
        inp.select();
      }
    }
  });
}

// Formatters
function formatDisplayDate(dateStr) {
  const parts = dateStr.split('-');
  if (parts.length === 3) {
    const d = new Date(parts[0], parts[1] - 1, parts[2]);
    const today = new Date();
    today.setHours(0, 0, 0, 0);

    const diff = Math.round((today - d) / (1000 * 60 * 60 * 24));
    if (diff === 0) return t('timeline.today');
    if (diff === 1) return t('timeline.yesterday');

    return d.toLocaleDateString(getLanguage() || undefined, {
      weekday: 'short',
      month: 'short',
      day: 'numeric',
      year: d.getFullYear() !== today.getFullYear() ? 'numeric' : undefined,
    });
  }
  return dateStr;
}

function formatTime(isoStr) {
  try {
    const d = new Date(isoStr);
    return d.toLocaleTimeString(getLanguage() || undefined, { hour: '2-digit', minute: '2-digit' });
  } catch (e) {
    return '';
  }
}

function escapeHtml(str) {
  if (!str) return '';
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}

// Admin: User Management
async function loadAdminUsers() {
  if (!el.adminUsersTbody) return;
  if (el.adminBackchannelUrlInput) {
    if (state.authStatus && state.authStatus.backchannel_logout_url) {
      el.adminBackchannelUrlInput.value = state.authStatus.backchannel_logout_url;
    } else {
      const base = window.location.origin + (window.location.pathname.replace(/\/+$/, '') || '');
      el.adminBackchannelUrlInput.value = `${base}/api/v1/auth/backchannel-logout`;
    }
  }
  try {
    el.adminUsersTbody.innerHTML = '<tr><td colspan="5" style="text-align:center;padding:30px;color:var(--text-muted);">Loading registered users...</td></tr>';
    const users = await api.getAdminUsers();
    renderAdminUsers(users);
  } catch (err) {
    console.error('Error loading admin users:', err);
    el.adminUsersTbody.innerHTML = `<tr><td colspan="5" style="text-align:center;padding:30px;color:var(--accent-red);">Failed to load users: ${escapeHtml(err.message)}</td></tr>`;
  }
}

function renderAdminUsers(users) {
  if (!el.adminUsersTbody) return;
  if (!users || users.length === 0) {
    el.adminUsersTbody.innerHTML = '<tr><td colspan="5" style="text-align:center;padding:30px;color:var(--text-muted);">No users found.</td></tr>';
    return;
  }

  el.adminUsersTbody.innerHTML = users.map(u => {
    const initial = (u.display_name || u.email || 'U').charAt(0).toUpperCase();
    const created = new Date(u.created_at).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
    const isSelf = state.user && u.id === state.user.id;
    const deleteBtn = isSelf
      ? '<span class="text-muted" style="font-size:0.8rem;font-weight:500;">Current Account</span>'
      : `<button class="btn btn-danger btn-sm btn-delete-user" data-id="${u.id}" data-email="${escapeHtml(u.email)}">Delete User</button>`;

    return `
      <tr>
        <td>
          <div class="admin-user-cell">
            <div class="admin-user-avatar">${initial}</div>
            <div class="admin-user-info">
              <span class="admin-user-name">${escapeHtml(u.display_name)}</span>
              <span class="admin-user-email">${escapeHtml(u.email)}</span>
            </div>
          </div>
        </td>
        <td>
          <span class="role-badge role-${u.role}">${u.role}</span>
        </td>
        <td><strong>${u.scrobble_count}</strong> scrobbles</td>
        <td class="text-muted">${created}</td>
        <td style="text-align: right;">${deleteBtn}</td>
      </tr>
    `;
  }).join('');

  // Attach delete user click listeners
  el.adminUsersTbody.querySelectorAll('.btn-delete-user').forEach(btn => {
    btn.addEventListener('click', async () => {
      const userId = btn.dataset.id;
      const email = btn.dataset.email;
      if (confirm(`Are you sure you want to permanently delete user "${email}"?\n\nAll associated watch history, scrobbles, and active playback sessions will be permanently erased.`)) {
        try {
          btn.disabled = true;
          btn.textContent = 'Deleting...';
          await api.deleteAdminUser(userId);
          loadAdminUsers();
        } catch (err) {
          alert('Failed to delete user: ' + err.message);
          btn.disabled = false;
          btn.textContent = 'Delete User';
        }
      }
    });
  });
}

if (el.btnRefreshAdminUsers) {
  el.btnRefreshAdminUsers.addEventListener('click', () => {
    if (state.admin.activeSubview === 'admin-sub-backups') {
      loadAdminBackups();
    } else if (state.admin.activeSubview === 'admin-sub-tasks') {
      loadAdminTasks();
    } else if (state.admin.activeSubview === 'admin-sub-logs') {
      loadAdminLogs();
    } else if (state.admin.activeSubview === 'admin-sub-settings') {
      loadAdminSettings();
    } else {
      loadAdminUsers();
    }
  });
}

if (el.btnCopyBackchannelUrl && el.adminBackchannelUrlInput) {
  el.btnCopyBackchannelUrl.addEventListener('click', () => {
    el.adminBackchannelUrlInput.select();
    navigator.clipboard.writeText(el.adminBackchannelUrlInput.value);
    el.btnCopyBackchannelUrl.textContent = 'Copied!';
    setTimeout(() => {
      el.btnCopyBackchannelUrl.textContent = 'Copy URL';
    }, 2000);
  });
}

// Admin: Sub-navigation
function switchAdminSubview(subviewId) {
  state.admin.activeSubview = subviewId;

  if (el.adminSubnavPills) {
    el.adminSubnavPills.querySelectorAll('.pill').forEach(btn => {
      btn.classList.toggle('active', btn.dataset.subview === subviewId);
    });
  }

  document.querySelectorAll('.admin-subview').forEach(view => {
    view.style.display = view.id === subviewId ? 'block' : 'none';
  });

  if (subviewId === 'admin-sub-users') {
    stopLogsAutoRefresh();
    loadAdminUsers();
  } else if (subviewId === 'admin-sub-settings') {
    stopLogsAutoRefresh();
    loadAdminSettings();
  } else if (subviewId === 'admin-sub-tasks') {
    stopLogsAutoRefresh();
    loadAdminTasks();
  } else if (subviewId === 'admin-sub-logs') {
    loadAdminLogs();
    startLogsAutoRefresh();
  } else if (subviewId === 'admin-sub-backups') {
    stopLogsAutoRefresh();
    loadAdminBackups();
  }
}

if (el.adminSubnavPills) {
  el.adminSubnavPills.querySelectorAll('.pill').forEach(btn => {
    btn.addEventListener('click', () => {
      const subview = btn.dataset.subview;
      if (subview) switchAdminSubview(subview);
    });
  });
}

// Helper to configure masked API key inputs with show/hide toggle
function applySecretField(inputEl, rawKey, maskedKey) {
  if (!inputEl) return;
  const existingRaw = inputEl.dataset.raw || '';
  const currentVal = inputEl.value ? inputEl.value.trim() : '';
  const effectiveRaw = rawKey || (currentVal && !currentVal.includes('•') ? currentVal : existingRaw);
  inputEl.dataset.raw = effectiveRaw;
  inputEl.dataset.masked = maskedKey || '';
  inputEl.dataset.revealed = 'false';
  inputEl.value = maskedKey || effectiveRaw || '';

  const parent = inputEl.closest('.input-secret-wrapper');
  if (parent) {
    const btn = parent.querySelector('.btn-toggle-secret');
    if (btn) {
      btn.title = 'Unhide / Show full key';
      btn.innerHTML = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>`;
    }
  }

  inputEl.oninput = () => {
    inputEl.dataset.raw = inputEl.value;
    inputEl.dataset.masked = '';
    inputEl.dataset.revealed = 'true';
  };
}

function getSecretValue(inputEl) {
  if (!inputEl) return null;
  const val = inputEl.value.trim();
  if (!val) return '';
  if (inputEl.dataset.masked && val === inputEl.dataset.masked) {
    return inputEl.dataset.raw || null;
  }
  return val;
}

document.addEventListener('click', (e) => {
  const btn = e.target.closest('.btn-toggle-secret');
  if (!btn) return;
  const targetId = btn.getAttribute('data-target');
  const input = document.getElementById(targetId);
  if (!input) return;

  const raw = input.dataset.raw || '';
  const masked = input.dataset.masked || '';
  if (!raw && !masked) return;

  const isRevealed = input.dataset.revealed === 'true';
  if (isRevealed) {
    // Hide again
    input.value = masked || raw;
    input.dataset.revealed = 'false';
    btn.title = 'Unhide / Show full key';
    btn.innerHTML = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>`;
  } else {
    // Reveal full key
    if (!raw) {
      alert('For security reasons, existing configured keys cannot be revealed in plaintext. You can enter a new key to update it.');
      return;
    }
    input.value = raw;
    input.dataset.revealed = 'true';
    btn.title = 'Hide / Redact key';
    btn.innerHTML = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24"/><line x1="1" y1="1" x2="23" y2="23"/></svg>`;
  }
});

// Admin: Settings & API Keys
function updateAdminMetadataSourceUI() {
  const src = el.settingMetadataSource ? el.settingMetadataSource.value : 'tvdb';
  if (el.adminTvdbGroup) {
    el.adminTvdbGroup.style.opacity = src === 'tvdb' ? '1' : '0.65';
  }
  if (el.adminTmdbGroup) {
    el.adminTmdbGroup.style.opacity = src === 'tmdb' ? '1' : '0.65';
  }
}

if (el.settingMetadataSource) {
  el.settingMetadataSource.addEventListener('change', updateAdminMetadataSourceUI);
}

async function loadAdminSettings() {
  if (!el.formAdminSettings) return;
  try {
    const s = await api.getAdminSettings();
    if (el.settingMetadataSource) {
      el.settingMetadataSource.value = s.metadata_source || 'tvdb';
      updateAdminMetadataSourceUI();
    }
    applySecretField(el.settingTvdbApiKey, s.tvdb_api_key, s.tvdb_api_key_masked);
    if (el.settingTvdbPin) el.settingTvdbPin.value = s.tvdb_pin || '';
    if (el.settingTvdbBaseUrl) el.settingTvdbBaseUrl.value = s.tvdb_base_url || '';

    applySecretField(el.settingTmdbApiKey, s.tmdb_api_key, s.tmdb_api_key_masked);
    if (el.settingTmdbBaseUrl) el.settingTmdbBaseUrl.value = s.tmdb_base_url || '';

    if (el.settingOidcEnabled) el.settingOidcEnabled.checked = Boolean(s.oidc_enabled);
    if (el.settingOidcIssuerUrl) el.settingOidcIssuerUrl.value = s.oidc_issuer_url || '';
    if (el.settingOidcClientId) el.settingOidcClientId.value = s.oidc_client_id || '';
    applySecretField(el.settingOidcClientSecret, s.oidc_client_secret, s.oidc_client_secret_masked);
    if (el.settingOidcRedirectUrl) el.settingOidcRedirectUrl.value = s.oidc_redirect_url || '';
    if (el.settingOidcAdminGroup) el.settingOidcAdminGroup.value = s.oidc_admin_group || '';
    if (el.settingScrobbleThreshold) el.settingScrobbleThreshold.value = s.scrobble_threshold_percent != null ? s.scrobble_threshold_percent : 80;
  } catch (err) {
    console.error('Failed to load admin settings:', err);
    if (el.adminSettingsStatus) {
      el.adminSettingsStatus.style.color = 'var(--accent-red)';
      el.adminSettingsStatus.textContent = 'Failed to load settings: ' + err.message;
    }
  }
}

if (el.formAdminSettings) {
  el.formAdminSettings.addEventListener('submit', async (e) => {
    e.preventDefault();
    if (el.btnSaveAdminSettings) {
      el.btnSaveAdminSettings.disabled = true;
      el.btnSaveAdminSettings.textContent = 'Saving...';
    }
    if (el.adminSettingsStatus) el.adminSettingsStatus.textContent = '';

    try {
      const payload = {
        metadata_source: el.settingMetadataSource ? el.settingMetadataSource.value : null,
        tvdb_api_key: getSecretValue(el.settingTvdbApiKey),
        tvdb_pin: el.settingTvdbPin ? el.settingTvdbPin.value.trim() : null,
        tvdb_base_url: el.settingTvdbBaseUrl ? el.settingTvdbBaseUrl.value.trim() : null,
        tmdb_api_key: getSecretValue(el.settingTmdbApiKey),
        tmdb_base_url: el.settingTmdbBaseUrl ? el.settingTmdbBaseUrl.value.trim() : null,
        oidc_enabled: el.settingOidcEnabled ? el.settingOidcEnabled.checked : null,
        oidc_issuer_url: el.settingOidcIssuerUrl ? el.settingOidcIssuerUrl.value.trim() : null,
        oidc_client_id: el.settingOidcClientId ? el.settingOidcClientId.value.trim() : null,
        oidc_client_secret: getSecretValue(el.settingOidcClientSecret),
        oidc_redirect_url: el.settingOidcRedirectUrl ? el.settingOidcRedirectUrl.value.trim() : null,
        oidc_admin_group: el.settingOidcAdminGroup ? el.settingOidcAdminGroup.value.trim() : null,
        scrobble_threshold_percent: el.settingScrobbleThreshold && el.settingScrobbleThreshold.value ? parseInt(el.settingScrobbleThreshold.value, 10) : null,
      };

      await api.updateAdminSettings(payload);
      await loadAdminSettings();
      if (el.adminSettingsStatus) {
        el.adminSettingsStatus.style.color = '#34d399';
        el.adminSettingsStatus.textContent = 'Settings saved successfully!';
        setTimeout(() => {
          if (el.adminSettingsStatus) el.adminSettingsStatus.textContent = '';
        }, 4000);
      }
    } catch (err) {
      console.error('Failed to save settings:', err);
      if (el.adminSettingsStatus) {
        el.adminSettingsStatus.style.color = 'var(--accent-red)';
        el.adminSettingsStatus.textContent = 'Error: ' + err.message;
      }
    } finally {
      if (el.btnSaveAdminSettings) {
        el.btnSaveAdminSettings.disabled = false;
        el.btnSaveAdminSettings.textContent = 'Save Settings';
      }
    }
  });
}

// Admin: Background Tasks
async function loadAdminTasks() {
  if (!el.adminTasksContainer) return;
  try {
    const res = await api.getAdminTasks();
    const tasks = Array.isArray(res) ? res : (res && res.tasks ? res.tasks : []);
    renderAdminTasks(tasks);
  } catch (err) {
    console.error('Failed to load admin tasks:', err);
    el.adminTasksContainer.innerHTML = `<div class="empty-state"><p style="color:var(--accent-red);">Failed to load tasks: ${escapeHtml(err.message)}</p></div>`;
  }
}

function renderAdminTasks(tasks) {
  if (!el.adminTasksContainer) return;
  if (!tasks || tasks.length === 0) {
    el.adminTasksContainer.innerHTML = `<div class="empty-state"><p>No background tasks registered.</p></div>`;
    return;
  }

  el.adminTasksContainer.innerHTML = tasks.map(t => {
    const isRunning = t.status === 'running';
    const statusClass = `task-status-${t.status || 'idle'}`;
    const lastRunTime = t.last_run_at || t.last_run;
    const lastSuccessTime = t.last_success_at || t.last_success;
    const nextRunTime = t.next_run_at || t.next_run;
    const lastRunStr = lastRunTime ? new Date(lastRunTime).toLocaleString() : 'Never';
    const lastSuccessStr = lastSuccessTime ? new Date(lastSuccessTime).toLocaleString() : 'Never';
    const nextRunStr = nextRunTime ? new Date(nextRunTime).toLocaleString() : 'Pending next cycle';
    const msg = t.last_message || t.message || 'Idle';
    const pending = t.items_pending != null ? t.items_pending : 0;
    const intervalHours = t.interval_hours || 4;

    return `
      <div class="task-card" data-task-id="${t.id}">
        <div class="task-card-header">
          <div>
            <div class="task-card-title">${escapeHtml(t.name)}</div>
            <div class="task-card-desc">${escapeHtml(t.description)}</div>
          </div>
          <span class="task-status-badge ${statusClass}">
            ${isRunning ? '<span class="pulse-dot-sm pulse-active"></span>' : ''}
            ${escapeHtml(t.status)}
          </span>
        </div>

        <div class="task-card-meta">
          <div class="task-meta-row">
            <span>Status Message:</span>
            <strong>${escapeHtml(msg)}</strong>
          </div>
          <div class="task-meta-row">
            <span>Processed Items:</span>
            <strong>${t.items_processed} processed (${pending} pending)</strong>
          </div>
          <div class="task-meta-row" style="align-items: center;">
            <span>Run Schedule:</span>
            <div style="display: flex; align-items: center; gap: 8px;">
              <select class="input-select task-schedule-select" data-task-id="${t.id}" style="padding: 3px 8px; font-size: 0.85rem; width: auto; background: var(--bg-card); color: var(--text-main); border: 1px solid var(--border-color); border-radius: 4px;">
                <option value="1" ${intervalHours === 1 ? 'selected' : ''}>Every 1 hour</option>
                <option value="2" ${intervalHours === 2 ? 'selected' : ''}>Every 2 hours</option>
                <option value="4" ${intervalHours === 4 ? 'selected' : ''}>Every 4 hours (Default)</option>
                <option value="6" ${intervalHours === 6 ? 'selected' : ''}>Every 6 hours</option>
                <option value="12" ${intervalHours === 12 ? 'selected' : ''}>Every 12 hours</option>
                <option value="24" ${intervalHours === 24 ? 'selected' : ''}>Every 24 hours (Daily)</option>
              </select>
              <span class="schedule-save-status" data-task-id="${t.id}" style="font-size: 0.75rem; color: #34d399;"></span>
            </div>
          </div>
          <div class="task-meta-row">
            <span>Next Scheduled Run:</span>
            <strong class="task-next-run">${nextRunStr}</strong>
          </div>
          <div class="task-meta-row">
            <span>Last Started:</span>
            <strong>${lastRunStr}</strong>
          </div>
          <div class="task-meta-row">
            <span>Last Completed:</span>
            <strong>${lastSuccessStr}</strong>
          </div>
        </div>

        <div style="margin-top: 8px; display: flex; gap: 8px; flex-wrap: wrap;">
          <button class="btn btn-outline btn-sm btn-trigger-task" data-task-id="${t.id}">
            ${isRunning ? 'Running... (Run Again)' : 'Run Now'}
          </button>
          ${t.id !== 'db_backup' && t.id !== 'backup' ? `
          <button class="btn btn-outline btn-sm btn-force-task" data-task-id="${t.id}" title="${
            t.id === 'image_cache'
              ? 'Reset poster thumbnails and re-download posters for all records'
              : t.id === 'user_sync'
              ? 'Clear user cache and re-sync all user mappings'
              : 'Re-download metadata for all records regardless of existing metadata'
          }">
            Force Refresh All
          </button>
          ` : ''}
        </div>
      </div>
    `;
  }).join('');

  el.adminTasksContainer.querySelectorAll('.task-schedule-select').forEach(select => {
    select.addEventListener('change', async (e) => {
      const taskId = select.dataset.taskId;
      const hours = parseInt(e.target.value, 10);
      const statusSpan = el.adminTasksContainer.querySelector(`.schedule-save-status[data-task-id="${taskId}"]`);
      if (statusSpan) statusSpan.textContent = 'Saving...';
      try {
        await api.updateAdminTaskSchedule(taskId, hours);
        if (statusSpan) {
          statusSpan.textContent = 'Saved!';
          setTimeout(() => { if (statusSpan) statusSpan.textContent = ''; }, 2500);
        }
        await loadAdminTasks();
      } catch (err) {
        alert('Failed to update task schedule: ' + err.message);
        if (statusSpan) statusSpan.textContent = '';
      }
    });
  });

  el.adminTasksContainer.querySelectorAll('.btn-trigger-task').forEach(btn => {
    btn.addEventListener('click', async () => {
      const taskId = btn.dataset.taskId;
      btn.disabled = true;
      btn.textContent = 'Triggering...';
      try {
        await api.triggerAdminTask(taskId);
        setTimeout(() => loadAdminTasks(), 500);
        setTimeout(() => loadAdminTasks(), 1500);
        setTimeout(() => loadAdminTasks(), 3000);
      } catch (err) {
        alert('Failed to trigger task: ' + err.message);
        btn.disabled = false;
        btn.textContent = 'Run Now';
      }
    });
  });

  el.adminTasksContainer.querySelectorAll('.btn-force-task').forEach(btn => {
    btn.addEventListener('click', async () => {
      const taskId = btn.dataset.taskId;
      let confirmMsg = 'Force refresh all items for this background task?';
      if (taskId === 'tvdb_metadata') {
        confirmMsg = 'Force refresh metadata for ALL items? This will reset metadata timestamps and re-download all metadata from the active metadata provider.';
      } else if (taskId === 'image_cache') {
        confirmMsg = 'Force refresh poster images for ALL items? This will reset poster thumbnails and re-download artwork for all scrobbles.';
      } else if (taskId === 'user_sync') {
        confirmMsg = 'Force refresh user cache? This will clear all cached Plex and Jellyfin user mappings and re-sync them afresh.';
      }

      if (!confirm(confirmMsg)) {
        return;
      }
      btn.disabled = true;
      btn.textContent = 'Triggering...';
      try {
        if (taskId === 'tvdb_metadata') {
          await api.forceRefreshMetadata();
        } else if (taskId === 'image_cache') {
          await api.forceRefreshPosters();
        } else if (taskId === 'user_sync') {
          await api.forceRefreshUserCache();
        } else {
          await api.triggerAdminTask(taskId, true);
        }
        btn.textContent = 'Triggered!';
        setTimeout(() => loadAdminTasks(), 500);
        setTimeout(() => loadAdminTasks(), 1500);
        setTimeout(() => loadAdminTasks(), 3000);
      } catch (err) {
        alert('Failed to force refresh: ' + err.message);
        btn.disabled = false;
        btn.textContent = 'Force Refresh All';
      }
    });
  });
}

if (el.btnRefreshAdminTasks) {
  el.btnRefreshAdminTasks.addEventListener('click', () => loadAdminTasks());
}

if (el.btnForceRefreshAllMetadata) {
  el.btnForceRefreshAllMetadata.addEventListener('click', async () => {
    if (!confirm('Force refresh metadata for ALL items? This will reset metadata timestamps and re-download all metadata from the active metadata provider.')) {
      return;
    }
    el.btnForceRefreshAllMetadata.disabled = true;
    const originalText = el.btnForceRefreshAllMetadata.textContent;
    el.btnForceRefreshAllMetadata.textContent = 'Triggering...';
    try {
      await api.forceRefreshMetadata();
      el.btnForceRefreshAllMetadata.textContent = 'Triggered!';
      setTimeout(() => {
        el.btnForceRefreshAllMetadata.disabled = false;
        el.btnForceRefreshAllMetadata.textContent = originalText;
        loadAdminTasks();
      }, 1500);
    } catch (err) {
      alert('Failed to force refresh metadata: ' + err.message);
      el.btnForceRefreshAllMetadata.disabled = false;
      el.btnForceRefreshAllMetadata.textContent = originalText;
    }
  });
}

// Admin: Application Logs
let logsAutoRefreshInterval = null;

async function loadAdminLogs() {
  if (!el.adminLogsViewer) return;
  try {
    const level = el.adminLogsFilterLevel ? el.adminLogsFilterLevel.value : '';
    const res = await api.getAdminLogs(level ? { level } : {});
    const logs = Array.isArray(res) ? res : (res && res.entries ? res.entries : []);
    renderAdminLogs(logs);
  } catch (err) {
    console.error('Failed to load admin logs:', err);
  }
}

function renderAdminLogs(logs) {
  if (!el.adminLogsViewer) return;
  if (!logs || logs.length === 0) {
    el.adminLogsViewer.innerHTML = `<div style="color:var(--text-dim);padding:14px;text-align:center;">No logs captured yet.</div>`;
    return;
  }

  const wasAtBottom = el.adminLogsViewer.scrollHeight - el.adminLogsViewer.scrollTop <= el.adminLogsViewer.clientHeight + 40;

  el.adminLogsViewer.innerHTML = logs.map(l => {
    const time = new Date(l.timestamp).toLocaleTimeString([], { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' });
    const level = (l.level || 'INFO').toUpperCase();
    return `
      <div class="log-entry">
        <span class="log-time">${time}</span>
        <span class="log-level log-level-${level}">${level}</span>
        <span class="log-target">${escapeHtml(l.target)}</span>
        <span class="log-msg">${escapeHtml(l.message)}</span>
      </div>
    `;
  }).join('');

  if (wasAtBottom) {
    el.adminLogsViewer.scrollTop = el.adminLogsViewer.scrollHeight;
  }
}

function startLogsAutoRefresh() {
  stopLogsAutoRefresh();
  if (el.adminLogsAutorefresh && el.adminLogsAutorefresh.checked) {
    logsAutoRefreshInterval = setInterval(() => {
      if (state.activeTab === 'view-admin' && state.admin.activeSubview === 'admin-sub-logs') {
        loadAdminLogs();
      }
    }, 3000);
  }
}

function stopLogsAutoRefresh() {
  if (logsAutoRefreshInterval) {
    clearInterval(logsAutoRefreshInterval);
    logsAutoRefreshInterval = null;
  }
}

if (el.btnRefreshAdminLogs) {
  el.btnRefreshAdminLogs.addEventListener('click', () => loadAdminLogs());
}

if (el.adminLogsFilterLevel) {
  el.adminLogsFilterLevel.addEventListener('change', () => loadAdminLogs());
}

if (el.adminLogsAutorefresh) {
  el.adminLogsAutorefresh.addEventListener('change', () => {
    if (el.adminLogsAutorefresh.checked) {
      startLogsAutoRefresh();
    } else {
      stopLogsAutoRefresh();
    }
  });
}

if (el.btnClearAdminLogs) {
  el.btnClearAdminLogs.addEventListener('click', async () => {
    if (confirm('Clear all captured application logs from memory?')) {
      try {
        await api.clearAdminLogs();
        loadAdminLogs();
      } catch (err) {
        alert('Failed to clear logs: ' + err.message);
      }
    }
  });
}

// ==========================================
// ADMIN: DATABASE BACKUPS & RESTORE
// ==========================================

let pendingRestoreFilename = null;

async function loadAdminBackups() {
  if (!el.adminBackupsTbody) return;
  el.adminBackupsTbody.innerHTML = `<tr><td colspan="4" style="text-align:center;padding:20px;color:var(--text-dim);"><div class="spinner" style="margin: 0 auto 10px;"></div>Loading database backups...</td></tr>`;
  try {
    const res = await api.getBackups();
    if (el.adminBackupsPath && res.backup_dir) {
      el.adminBackupsPath.textContent = res.backup_dir;
    }
    const backups = Array.isArray(res.backups) ? res.backups : [];
    renderAdminBackups(backups);
  } catch (err) {
    console.error('Failed to load database backups:', err);
    el.adminBackupsTbody.innerHTML = `<tr><td colspan="4" style="text-align:center;padding:20px;color:var(--accent-red);">Failed to load backups: ${escapeHtml(err.message)}</td></tr>`;
  }
}

function renderAdminBackups(backups) {
  if (!el.adminBackupsTbody) return;
  if (!backups || backups.length === 0) {
    el.adminBackupsTbody.innerHTML = `<tr><td colspan="4" style="text-align:center;padding:24px;color:var(--text-dim);">No database backups found in directory. Click "Backup Now" to create your first backup.</td></tr>`;
    return;
  }

  el.adminBackupsTbody.innerHTML = backups
    .map(b => {
      const createdStr = b.created_at ? new Date(b.created_at).toLocaleString() : 'Unknown';
      const isPreRestore = b.filename.includes('pre_restore');
      const badge = isPreRestore
        ? `<span class="badge-pill" style="font-size:0.7rem;background:rgba(234,179,8,0.15);color:#facc15;margin-left:8px;">Safety Snapshot</span>`
        : '';

      return `
        <tr data-filename="${escapeHtml(b.filename)}">
          <td>
            <div style="display:flex;align-items:center;gap:8px;">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="color:#38bdf8;flex-shrink:0;"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" y1="3" x2="12" y2="15"/></svg>
              <span class="font-mono" style="font-size:0.85rem;word-break:break-all;">${escapeHtml(b.filename)}</span>
              ${badge}
            </div>
          </td>
          <td style="color:var(--text-dim);font-size:0.85rem;white-space:nowrap;">${createdStr}</td>
          <td style="font-size:0.85rem;color:#38bdf8;white-space:nowrap;">${escapeHtml(b.size_formatted || '0 B')}</td>
          <td style="text-align: right; white-space:nowrap;">
            <div style="display:inline-flex;gap:8px;">
              <button class="btn btn-outline btn-sm btn-restore-backup" data-filename="${escapeHtml(b.filename)}" title="Restore database from this backup">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="1 4 1 10 7 10"/><path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10"/></svg>
                Restore
              </button>
              <button class="btn btn-outline btn-sm text-danger btn-delete-backup" data-filename="${escapeHtml(b.filename)}" title="Delete this backup file">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
              </button>
            </div>
          </td>
        </tr>
      `;
    })
    .join('');

  // Attach restore buttons
  el.adminBackupsTbody.querySelectorAll('.btn-restore-backup').forEach(btn => {
    btn.addEventListener('click', () => {
      const filename = btn.dataset.filename;
      openRestoreModal(filename);
    });
  });

  // Attach delete buttons
  el.adminBackupsTbody.querySelectorAll('.btn-delete-backup').forEach(btn => {
    btn.addEventListener('click', async () => {
      const filename = btn.dataset.filename;
      if (!confirm(`Are you sure you want to permanently delete backup "${filename}"?`)) {
        return;
      }
      btn.disabled = true;
      try {
        await api.deleteBackup(filename);
        showAdminBackupsStatus(`Backup "${filename}" deleted successfully`, 'success');
        loadAdminBackups();
      } catch (err) {
        alert('Failed to delete backup: ' + err.message);
        btn.disabled = false;
      }
    });
  });
}

function showAdminBackupsStatus(msg, type = 'info') {
  if (!el.adminBackupsStatus) return;
  el.adminBackupsStatus.style.display = 'block';
  el.adminBackupsStatus.className = `import-status-box status-${type}`;
  el.adminBackupsStatus.textContent = msg;
  setTimeout(() => {
    if (el.adminBackupsStatus) el.adminBackupsStatus.style.display = 'none';
  }, 4000);
}

function openRestoreModal(filename) {
  pendingRestoreFilename = filename;
  if (el.restoreTargetFilename) {
    el.restoreTargetFilename.textContent = filename;
  }
  if (el.restoreModalStatus) {
    el.restoreModalStatus.style.display = 'none';
  }
  if (el.btnConfirmRestoreBackup) {
    el.btnConfirmRestoreBackup.disabled = false;
    el.btnConfirmRestoreBackup.textContent = 'Confirm & Restore';
  }
  if (el.modalRestoreBackup) {
    el.modalRestoreBackup.style.display = 'flex';
  }
}

function closeRestoreModal() {
  pendingRestoreFilename = null;
  if (el.modalRestoreBackup) {
    el.modalRestoreBackup.style.display = 'none';
  }
}

if (el.btnCloseRestoreModal) {
  el.btnCloseRestoreModal.addEventListener('click', closeRestoreModal);
}
if (el.btnCancelRestoreModal) {
  el.btnCancelRestoreModal.addEventListener('click', closeRestoreModal);
}
if (el.modalRestoreBackup) {
  el.modalRestoreBackup.addEventListener('click', e => {
    if (e.target === el.modalRestoreBackup) closeRestoreModal();
  });
}

if (el.btnConfirmRestoreBackup) {
  el.btnConfirmRestoreBackup.addEventListener('click', async () => {
    if (!pendingRestoreFilename) return;
    el.btnConfirmRestoreBackup.disabled = true;
    el.btnConfirmRestoreBackup.textContent = 'Restoring...';

    if (el.restoreModalStatus) {
      el.restoreModalStatus.style.display = 'block';
      el.restoreModalStatus.className = 'import-status-box status-info';
      el.restoreModalStatus.textContent = 'Executing database restore and schema verification...';
    }

    try {
      const res = await api.restoreBackup(pendingRestoreFilename);
      if (el.restoreModalStatus) {
        el.restoreModalStatus.className = 'import-status-box status-success';
        el.restoreModalStatus.textContent = res.message || 'Database restored successfully!';
      }
      setTimeout(() => {
        closeRestoreModal();
        loadAdminBackups();
        // Also refresh user data / stats / timeline
        fetchTimeline();
        fetchStats();
      }, 1500);
    } catch (err) {
      console.error('Failed to restore backup:', err);
      if (el.restoreModalStatus) {
        el.restoreModalStatus.className = 'import-status-box status-error';
        el.restoreModalStatus.textContent = 'Restore failed: ' + err.message;
      }
      el.btnConfirmRestoreBackup.disabled = false;
      el.btnConfirmRestoreBackup.textContent = 'Try Again';
    }
  });
}

if (el.btnAdminCreateBackup) {
  el.btnAdminCreateBackup.addEventListener('click', async () => {
    el.btnAdminCreateBackup.disabled = true;
    const origHtml = el.btnAdminCreateBackup.innerHTML;
    el.btnAdminCreateBackup.innerHTML = `<span class="spinner" style="width:14px;height:14px;border-width:2px;display:inline-block;vertical-align:middle;margin-right:6px;"></span> Creating Backup...`;
    try {
      const res = await api.createBackup();
      showAdminBackupsStatus(`Backup created successfully: ${res.backup ? res.backup.filename : 'complete'}`, 'success');
      loadAdminBackups();
    } catch (err) {
      alert('Failed to create database backup: ' + err.message);
    } finally {
      el.btnAdminCreateBackup.disabled = false;
      el.btnAdminCreateBackup.innerHTML = origHtml;
    }
  });
}

if (el.btnRefreshAdminBackups) {
  el.btnRefreshAdminBackups.addEventListener('click', () => loadAdminBackups());
}

// ==========================================
// LIBRARY CLEANUP (Radarr & Sonarr)
// ==========================================

async function loadCleanupItems() {
  if (!el.cleanupItemsGrid) return;
  state.cleanup.isLoading = true;
  el.cleanupItemsGrid.innerHTML = `
    <div style="grid-column: 1 / -1; display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 40px; color: var(--text-muted); gap: 12px;">
      <div class="spinner"></div>
      <p>Scanning Radarr and Sonarr libraries for watched media...</p>
    </div>
  `;

  try {
    const res = await api.getCleanupItems();
    state.cleanup.items = res.items || [];

    // Update summary metrics
    if (el.cleanupTotalSize) el.cleanupTotalSize.textContent = res.total_size_formatted || '0 B';
    if (el.cleanupTotalCount) {
      el.cleanupTotalCount.textContent = `${res.total_items} watched item${res.total_items === 1 ? '' : 's'} across libraries`;
    }

    const radarrItems = state.cleanup.items.filter(i => i.service === 'radarr');
    const radarrSize = radarrItems.reduce((acc, i) => acc + (i.size_on_disk || 0), 0);
    if (el.cleanupRadarrCount) el.cleanupRadarrCount.textContent = radarrItems.length;
    if (el.cleanupRadarrSize) el.cleanupRadarrSize.textContent = `${formatBytesClient(radarrSize)} on disk`;

    const sonarrItems = state.cleanup.items.filter(i => i.service === 'sonarr');
    const sonarrSize = sonarrItems.reduce((acc, i) => acc + (i.size_on_disk || 0), 0);
    if (el.cleanupSonarrCount) el.cleanupSonarrCount.textContent = sonarrItems.length;
    if (el.cleanupSonarrSize) el.cleanupSonarrSize.textContent = `${formatBytesClient(sonarrSize)} on disk`;

    // Filter counts
    const chipAll = document.getElementById('count-chip-all');
    const chipMovies = document.getElementById('count-chip-movies');
    const chipSeries = document.getElementById('count-chip-series');
    if (chipAll) chipAll.textContent = state.cleanup.items.length;
    if (chipMovies) chipMovies.textContent = radarrItems.length;
    if (chipSeries) chipSeries.textContent = sonarrItems.length;

    // Handle alert banner for unconfigured or error states
    if (el.cleanupAlertBanner) {
      if (!res.radarr_configured && !res.sonarr_configured) {
        el.cleanupAlertBanner.style.display = 'block';
        el.cleanupAlertBanner.className = 'alert-box alert-warning';
        el.cleanupAlertBanner.innerHTML = `
          <strong>Radarr & Sonarr Not Configured:</strong> To scan your media libraries and delete watched files, please enter your server URL and API key in <a href="#" id="link-cleanup-goto-settings" style="color:var(--accent-primary);text-decoration:underline;">Settings</a>.
        `;
        const link = document.getElementById('link-cleanup-goto-settings');
        if (link) link.addEventListener('click', (e) => { e.preventDefault(); switchTab('view-settings'); });
      } else if (res.radarr_error || res.sonarr_error) {
        el.cleanupAlertBanner.style.display = 'block';
        el.cleanupAlertBanner.className = 'alert-box alert-warning';
        const msgs = [];
        if (res.radarr_error) msgs.push(`Radarr: ${escapeHtml(res.radarr_error)}`);
        if (res.sonarr_error) msgs.push(`Sonarr: ${escapeHtml(res.sonarr_error)}`);
        el.cleanupAlertBanner.innerHTML = `<strong>Library connection warning:</strong> ${msgs.join(' | ')}`;
      } else {
        el.cleanupAlertBanner.style.display = 'none';
      }
    }

    renderCleanupItems();
  } catch (err) {
    if (el.cleanupItemsGrid) {
      el.cleanupItemsGrid.innerHTML = `
        <div style="grid-column: 1 / -1; text-align: center; padding: 40px; color: var(--text-muted);">
          <p style="color: #f87171; font-weight: 500;">Failed to scan libraries: ${escapeHtml(err.message)}</p>
          <button class="btn btn-outline btn-sm" onclick="loadCleanupItems()" style="margin-top: 12px;">Try Again</button>
        </div>
      `;
    }
  } finally {
    state.cleanup.isLoading = false;
  }
}

function formatBytesClient(bytes) {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  let val = bytes;
  while (val >= 1024 && i < units.length - 1) {
    val /= 1024;
    i++;
  }
  return `${val.toFixed(1)} ${units[i]}`;
}

function renderCleanupItems() {
  if (!el.cleanupItemsGrid) return;

  let filtered = [...state.cleanup.items];

  // 1. Filter by mediaType
  if (state.cleanup.filter === 'movie') {
    filtered = filtered.filter(i => i.media_type === 'movie');
  } else if (state.cleanup.filter === 'series') {
    filtered = filtered.filter(i => i.media_type === 'series');
  }

  // 2. Search
  if (state.cleanup.search.trim()) {
    const q = state.cleanup.search.trim().toLowerCase();
    filtered = filtered.filter(i => i.title.toLowerCase().includes(q));
  }

  // 3. Sort
  if (state.cleanup.sort === 'size') {
    filtered.sort((a, b) => (b.size_on_disk || 0) - (a.size_on_disk || 0));
  } else if (state.cleanup.sort === 'title') {
    filtered.sort((a, b) => a.title.localeCompare(b.title));
  } else {
    // recent
    filtered.sort((a, b) => String(b.last_watched_at).localeCompare(String(a.last_watched_at)));
  }

  if (filtered.length === 0) {
    el.cleanupItemsGrid.innerHTML = `
      <div style="grid-column: 1 / -1; text-align: center; padding: 60px 20px; color: var(--text-muted); background: var(--bg-card); border-radius: 12px; border: 1px dashed var(--border-color);">
        <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" style="margin-bottom: 12px; opacity: 0.6;"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg>
        <h3 style="font-size: 1.15rem; color: var(--text-main); margin-bottom: 6px;">All Clean!</h3>
        <p style="font-size: 0.9rem; max-width: 450px; margin: 0 auto;">No watched films or TV shows currently match your filter criteria in Radarr or Sonarr.</p>
      </div>
    `;
    return;
  }

  el.cleanupItemsGrid.innerHTML = filtered.map(item => {
    const isMovie = item.media_type === 'movie';
    const serviceBadgeClass = item.service === 'radarr' ? 'badge-radarr' : 'badge-sonarr';
    const serviceName = item.service === 'radarr' ? 'Radarr' : 'Sonarr';
    const watchedDateStr = item.last_watched_at ? new Date(item.last_watched_at).toLocaleDateString() : 'Watched';
    const posterImg = resolveImageUrl(item.poster_url) || './favicon.svg';

    let extraBadge = '';
    if (!isMovie) {
      const epCount = item.episode_file_count != null ? item.episode_file_count : 0;
      extraBadge = `<span class="badge-pill" style="background: rgba(255,255,255,0.06); color: var(--text-muted);">${epCount} Episode File${epCount === 1 ? '' : 's'}</span>`;
    }

    return `
      <div class="cleanup-card" data-item-id="${escapeHtml(item.id)}">
        <div class="cleanup-card-body">
          <img src="${escapeHtml(posterImg)}" alt="${escapeHtml(item.title)}" class="cleanup-card-poster" loading="lazy" onerror="this.src='./favicon.svg'" />
          <div class="cleanup-card-info">
            <div>
              <div class="cleanup-card-title">${escapeHtml(item.title)}${item.year ? ` <span style="font-size:0.8rem;color:var(--text-dim);">(${item.year})</span>` : ''}</div>
              <div class="cleanup-card-meta">
                <span>Last watched: <strong style="color:var(--text-main);">${escapeHtml(watchedDateStr)}</strong></span>
                <span>Times watched: <strong style="color:var(--text-main);">${item.watch_count || 1}</strong></span>
              </div>
            </div>

            <div class="cleanup-card-badges">
              <span class="badge-pill ${serviceBadgeClass}">${serviceName}</span>
              <span class="badge-pill badge-size">${escapeHtml(item.size_formatted || '0 B')}</span>
              ${extraBadge}
            </div>
          </div>
        </div>
        <div class="cleanup-card-footer">
          <span style="font-size: 0.78rem; color: var(--text-dim);">Storage reclaimable</span>
          <button type="button" class="btn btn-outline btn-sm btn-cleanup-item" data-item-id="${escapeHtml(item.id)}" style="padding: 4px 10px; font-size: 0.82rem; border-color: rgba(244,63,94,0.4); color: #fb7185;">
            Cleanup...
          </button>
        </div>
      </div>
    `;
  }).join('');

  // Attach click listeners to cards and buttons
  el.cleanupItemsGrid.querySelectorAll('.cleanup-card').forEach(card => {
    card.addEventListener('click', () => {
      const itemId = card.dataset.itemId;
      const item = state.cleanup.items.find(i => i.id === itemId);
      if (item) openCleanupModal(item);
    });
  });
}

function openCleanupModal(item) {
  if (!el.modalCleanupAction) return;
  state.cleanup.selectedItem = item;

  if (el.cleanupModalItemTitle) el.cleanupModalItemTitle.textContent = item.title + (item.year ? ` (${item.year})` : '');
  if (el.cleanupModalPoster) {
    el.cleanupModalPoster.src = resolveImageUrl(item.poster_url) || './favicon.svg';
  }
  if (el.cleanupModalServiceBadge) {
    el.cleanupModalServiceBadge.textContent = item.service === 'radarr' ? 'Radarr' : 'Sonarr';
    el.cleanupModalServiceBadge.className = `badge-pill ${item.service === 'radarr' ? 'badge-radarr' : 'badge-sonarr'}`;
  }
  if (el.cleanupModalSize) el.cleanupModalSize.textContent = item.size_formatted || '0 B';
  if (el.cleanupModalWatched) {
    el.cleanupModalWatched.textContent = item.last_watched_at ? new Date(item.last_watched_at).toLocaleDateString() : 'Watched';
  }
  if (el.cleanupModalStatus) {
    el.cleanupModalStatus.style.display = 'none';
    el.cleanupModalStatus.textContent = '';
  }
  if (el.cleanupAlsoRemoveHistory) {
    el.cleanupAlsoRemoveHistory.checked = false;
  }

  // Populate extra info & options based on media type
  const historyOnlyOptionHtml = `
    <div class="cleanup-action-opt" id="opt-cleanup-history-only" style="border-color: rgba(168, 85, 247, 0.35); margin-top: 6px;">
      <div class="cleanup-action-header">
        <span class="cleanup-action-title" style="color: #c084fc;">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 14 14"/></svg>
          Remove from Watch History Only
        </span>
        <span class="badge-pill" style="background: rgba(168, 85, 247, 0.15); color: #c084fc;">BingeSync History</span>
      </div>
      <div class="cleanup-action-desc">
        Deletes watch scrobbles for this title from BingeSync. Leaves media files and library monitoring untouched on ${item.service === 'radarr' ? 'Radarr' : 'Sonarr'}.
      </div>
    </div>
  `;

  if (item.service === 'radarr') {
    if (el.cleanupModalExtraInfo) {
      el.cleanupModalExtraInfo.textContent = `Film stored in Radarr library • ${item.has_file ? 'Video file exists on disk' : 'No file on disk'}`;
    }
    if (el.cleanupModalOptions) {
      el.cleanupModalOptions.innerHTML = `
        <div class="cleanup-action-opt opt-danger" id="opt-radarr-delete-both">
          <div class="cleanup-action-header">
            <span class="cleanup-action-title" style="color: #f87171;">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
              Delete Files & Remove from Radarr
            </span>
            <span class="badge-pill" style="background: rgba(239, 68, 68, 0.2); color: #f87171;">Reclaim ${escapeHtml(item.size_formatted)}</span>
          </div>
          <div class="cleanup-action-desc">
            Deletes media files from disk to reclaim space and permanently removes movie from Radarr monitoring.
          </div>
        </div>

        <div class="cleanup-action-opt" id="opt-radarr-delete-entry-only">
          <div class="cleanup-action-header">
            <span class="cleanup-action-title">
              Remove from Radarr Only (Keep Files)
            </span>
            <span class="badge-pill" style="background: rgba(255, 255, 255, 0.08); color: var(--text-muted);">Keep Media</span>
          </div>
          <div class="cleanup-action-desc">
            Removes movie from Radarr library but leaves downloaded video files untouched on your storage.
          </div>
        </div>

        ${historyOnlyOptionHtml}
      `;

      document.getElementById('opt-radarr-delete-both').addEventListener('click', () => executeRadarrCleanup(item, true));
      document.getElementById('opt-radarr-delete-entry-only').addEventListener('click', () => executeRadarrCleanup(item, false));
      document.getElementById('opt-cleanup-history-only').addEventListener('click', () => executeRemoveWatchHistory(item));
    }
  } else {
    // Sonarr TV Show
    const epFiles = item.episode_file_count || 0;
    if (el.cleanupModalExtraInfo) {
      el.cleanupModalExtraInfo.textContent = `Series in Sonarr • ${epFiles} episode file${epFiles === 1 ? '' : 's'} on disk • ${item.watched_episodes_count || 0} episodes watched`;
    }

    let seasonsHtml = '';
    if (item.seasons && item.seasons.length > 0) {
      const activeSeasons = item.seasons.filter(s => s.season_number > 0 && (s.episode_file_count > 0 || s.watched));
      if (activeSeasons.length > 0) {
        seasonsHtml = `
          <div style="margin-top: 10px; border-top: 1px solid var(--border-color); padding-top: 10px;">
            <div style="font-size: 0.82rem; font-weight: 600; color: var(--text-muted); margin-bottom: 6px;">Delete Individual Seasons & Unmonitor:</div>
            <div style="display: flex; flex-direction: column; gap: 6px; max-height: 180px; overflow-y: auto;">
              ${activeSeasons.map(s => `
                <div class="season-cleanup-row">
                  <div>
                    <strong>Season ${s.season_number}</strong>
                    <span style="font-size: 0.78rem; color: var(--text-dim); margin-left: 6px;">(${s.episode_file_count} file${s.episode_file_count === 1 ? '' : 's'})</span>
                    ${s.watched ? '<span class="badge-pill" style="font-size:0.68rem;background:rgba(52,211,153,0.15);color:#34d399;margin-left:4px;">Watched</span>' : ''}
                  </div>
                  <button type="button" class="btn btn-outline btn-sm btn-delete-season" data-season="${s.season_number}" style="padding: 2px 8px; font-size: 0.75rem; color: #fb923c; border-color: rgba(251,146,60,0.4);">
                    Delete Season Files
                  </button>
                </div>
              `).join('')}
            </div>
          </div>
        `;
      }
    }

    if (el.cleanupModalOptions) {
      el.cleanupModalOptions.innerHTML = `
        <div class="cleanup-action-opt opt-danger" id="opt-sonarr-delete-series">
          <div class="cleanup-action-header">
            <span class="cleanup-action-title" style="color: #f87171;">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
              Delete Entire Series & All Files
            </span>
            <span class="badge-pill" style="background: rgba(239, 68, 68, 0.2); color: #f87171;">Reclaim ${escapeHtml(item.size_formatted)}</span>
          </div>
          <div class="cleanup-action-desc">
            Completely removes series from Sonarr and deletes all episode files from disk.
          </div>
        </div>

        <div class="cleanup-action-opt opt-warning" id="opt-sonarr-delete-all-files">
          <div class="cleanup-action-header">
            <span class="cleanup-action-title" style="color: #fb923c;">
              Delete All Files & Unmonitor All Seasons
            </span>
            <span class="badge-pill" style="background: rgba(251, 146, 60, 0.15); color: #fb923c;">Reclaim ${escapeHtml(item.size_formatted)}</span>
          </div>
          <div class="cleanup-action-desc">
            Keeps the series in Sonarr, but deletes all downloaded episode files to reclaim space and sets all seasons to unmonitored.
          </div>
        </div>

        ${seasonsHtml}

        ${historyOnlyOptionHtml}
      `;

      document.getElementById('opt-sonarr-delete-series').addEventListener('click', () => {
        executeSonarrCleanup(item, { action: 'delete_series', delete_files: true });
      });

      document.getElementById('opt-sonarr-delete-all-files').addEventListener('click', () => {
        executeSonarrCleanup(item, { action: 'delete_all_files_unmonitor', delete_files: true });
      });

      document.getElementById('opt-cleanup-history-only').addEventListener('click', () => executeRemoveWatchHistory(item));

      el.cleanupModalOptions.querySelectorAll('.btn-delete-season').forEach(btn => {
        btn.addEventListener('click', (e) => {
          e.stopPropagation();
          const seasonNum = parseInt(btn.dataset.season, 10);
          executeSonarrCleanup(item, { action: 'delete_season', season_number: seasonNum, delete_files: true });
        });
      });
    }
  }

  el.modalCleanupAction.style.display = 'flex';
}

function closeCleanupModal() {
  if (el.modalCleanupAction) el.modalCleanupAction.style.display = 'none';
  state.cleanup.selectedItem = null;
}

async function executeRadarrCleanup(item, deleteFiles) {
  if (!confirm(`Are you sure you want to ${deleteFiles ? 'DELETE the media files and remove' : 'remove'} "${item.title}" from Radarr?`)) {
    return;
  }

  if (el.cleanupModalStatus) {
    el.cleanupModalStatus.style.display = 'block';
    el.cleanupModalStatus.className = 'import-status-box';
    el.cleanupModalStatus.textContent = 'Contacting Radarr to remove movie...';
  }

  try {
    const alsoRemoveHistory = el.cleanupAlsoRemoveHistory && el.cleanupAlsoRemoveHistory.checked;
    await api.deleteFromRadarr(item.service_id, deleteFiles);
    if (alsoRemoveHistory) {
      await api.deleteHistoryByMedia(item.media_type, item.title, item.year);
    }
    if (el.cleanupModalStatus) {
      el.cleanupModalStatus.className = 'import-status-box success';
      el.cleanupModalStatus.textContent = alsoRemoveHistory
        ? `Successfully removed "${item.title}" from Radarr and watch history!`
        : `Successfully removed "${item.title}" from Radarr!`;
    }

    state.cleanup.items = state.cleanup.items.filter(i => i.id !== item.id);
    setTimeout(() => {
      closeCleanupModal();
      loadCleanupItems();
    }, 1200);
  } catch (err) {
    if (el.cleanupModalStatus) {
      el.cleanupModalStatus.className = 'import-status-box error';
      el.cleanupModalStatus.textContent = `Radarr delete failed: ${err.message}`;
    }
  }
}

async function executeSonarrCleanup(item, options) {
  let confirmMsg = `Are you sure you want to clean up "${item.title}" in Sonarr?`;
  if (options.action === 'delete_series') {
    confirmMsg = `Are you sure you want to delete the ENTIRE series "${item.title}" and ALL its episode files from Sonarr?`;
  } else if (options.action === 'delete_all_files_unmonitor') {
    confirmMsg = `Are you sure you want to delete all episode files for "${item.title}" and unmonitor all seasons?`;
  } else if (options.action === 'delete_season') {
    confirmMsg = `Are you sure you want to delete episode files for Season ${options.season_number} of "${item.title}" and unmonitor the season?`;
  }

  if (!confirm(confirmMsg)) return;

  if (el.cleanupModalStatus) {
    el.cleanupModalStatus.style.display = 'block';
    el.cleanupModalStatus.className = 'import-status-box';
    el.cleanupModalStatus.textContent = 'Contacting Sonarr to perform cleanup action...';
  }

  try {
    const alsoRemoveHistory = el.cleanupAlsoRemoveHistory && el.cleanupAlsoRemoveHistory.checked;
    await api.deleteFromSonarr(item.service_id, options);
    if (alsoRemoveHistory) {
      await api.deleteHistoryByMedia(item.media_type, item.title, item.year);
    }
    if (el.cleanupModalStatus) {
      el.cleanupModalStatus.className = 'import-status-box success';
      el.cleanupModalStatus.textContent = alsoRemoveHistory
        ? `Successfully updated "${item.title}" in Sonarr and removed from watch history!`
        : `Successfully updated "${item.title}" in Sonarr!`;
    }

    if (options.action === 'delete_series' || alsoRemoveHistory) {
      state.cleanup.items = state.cleanup.items.filter(i => i.id !== item.id);
    }

    setTimeout(() => {
      closeCleanupModal();
      loadCleanupItems();
    }, 1200);
  } catch (err) {
    if (el.cleanupModalStatus) {
      el.cleanupModalStatus.className = 'import-status-box error';
      el.cleanupModalStatus.textContent = `Sonarr action failed: ${err.message}`;
    }
  }
}

async function executeRemoveWatchHistory(item) {
  if (!confirm(`Are you sure you want to remove "${item.title}" from your watch history? This will delete all watch scrobbles for this title in BingeSync.`)) {
    return;
  }

  if (el.cleanupModalStatus) {
    el.cleanupModalStatus.style.display = 'block';
    el.cleanupModalStatus.className = 'import-status-box';
    el.cleanupModalStatus.textContent = 'Removing scrobbles from watch history...';
  }

  try {
    const res = await api.deleteHistoryByMedia(item.media_type, item.title, item.year);
    if (el.cleanupModalStatus) {
      el.cleanupModalStatus.className = 'import-status-box success';
      el.cleanupModalStatus.textContent = `Successfully removed "${item.title}" from watch history (${res.deleted_count || 0} scrobbles deleted)!`;
    }

    state.cleanup.items = state.cleanup.items.filter(i => i.id !== item.id);
    setTimeout(() => {
      closeCleanupModal();
      loadCleanupItems();
    }, 1200);
  } catch (err) {
    if (el.cleanupModalStatus) {
      el.cleanupModalStatus.className = 'import-status-box error';
      el.cleanupModalStatus.textContent = `Failed to delete watch history: ${err.message}`;
    }
  }
}

if (el.btnRefreshCleanup) {
  el.btnRefreshCleanup.addEventListener('click', () => loadCleanupItems());
}

if (el.btnCloseCleanupModal) {
  el.btnCloseCleanupModal.addEventListener('click', () => closeCleanupModal());
}

if (el.btnCancelCleanupModal) {
  el.btnCancelCleanupModal.addEventListener('click', () => closeCleanupModal());
}

if (el.modalCleanupAction) {
  el.modalCleanupAction.addEventListener('click', (e) => {
    if (e.target === el.modalCleanupAction) closeCleanupModal();
  });
}

document.querySelectorAll('#cleanup-filter-chips .filter-chip').forEach(chip => {
  chip.addEventListener('click', () => {
    document.querySelectorAll('#cleanup-filter-chips .filter-chip').forEach(c => c.classList.remove('active'));
    chip.classList.add('active');
    state.cleanup.filter = chip.dataset.filter || 'all';
    renderCleanupItems();
  });
});

if (el.cleanupSearchInput) {
  el.cleanupSearchInput.addEventListener('input', (e) => {
    state.cleanup.search = e.target.value;
    renderCleanupItems();
  });
}

if (el.cleanupSortSelect) {
  el.cleanupSortSelect.addEventListener('change', (e) => {
    state.cleanup.sort = e.target.value;
    renderCleanupItems();
  });
}

// ==========================================================================
// Theme Accent System (Trakt Red, Plex Amber, Jellyfin Purple, Emby Cyan, Emerald, Rose, OLED)
// ==========================================================================
const themeColors = {
  red: '#e50914',
  amber: '#e5a00d',
  purple: '#a855f7',
  cyan: '#06b6d4',
  emerald: '#10b981',
  rose: '#f43f5e',
  oled: '#000000',
};

function applyTheme(theme) {
  const validThemes = ['red', 'amber', 'purple', 'cyan', 'emerald', 'rose', 'oled'];
  const activeTheme = validThemes.includes(theme) ? theme : 'red';
  document.documentElement.setAttribute('data-theme', activeTheme);
  document.body.setAttribute('data-theme', activeTheme);
  try {
    localStorage.setItem('bingesync_theme', activeTheme);
  } catch (e) {}

  document.querySelectorAll('.theme-select').forEach(sel => {
    if (sel.value !== activeTheme) {
      sel.value = activeTheme;
    }
  });

  const previewDot = document.getElementById('theme-preview-dot');
  if (previewDot) {
    previewDot.style.backgroundColor = themeColors[activeTheme] || themeColors.red;
    previewDot.style.boxShadow = activeTheme === 'oled' ? '0 0 4px #555' : `0 0 10px ${themeColors[activeTheme]}66`;
  }

  document.querySelectorAll('.theme-swatch').forEach(btn => {
    btn.classList.toggle('active', btn.dataset.theme === activeTheme);
  });
}

function initTheme() {
  let savedTheme = 'red';
  try {
    savedTheme = localStorage.getItem('bingesync_theme') || 'red';
  } catch (e) {}
  applyTheme(savedTheme);

  document.querySelectorAll('.theme-select').forEach(sel => {
    sel.addEventListener('change', (e) => {
      applyTheme(e.target.value);
    });
  });

  document.querySelectorAll('.theme-swatch').forEach(btn => {
    btn.addEventListener('click', () => {
      applyTheme(btn.dataset.theme);
    });
  });
}

// ==========================================================================
// PWA Service Worker Registration
// ==========================================================================
function initServiceWorker() {
  if ('serviceWorker' in navigator) {
    window.addEventListener('load', () => {
      navigator.serviceWorker.register('./sw.js').catch(err => {
        console.warn('Service worker registration failed:', err);
      });
    });
  }
}

// ==========================================================================
// Weekly Viewing Heatmap (7 Days x 24 Hours)
// ==========================================================================
function renderWeeklyHeatmap(heatmapData) {
  if (!el.weeklyHeatmapGrid) return;
  if (!heatmapData || heatmapData.length === 0) {
    el.weeklyHeatmapGrid.innerHTML = '<div class="empty-state" style="padding:20px;grid-column:1/-1;"><p>No viewing activity recorded yet.</p></div>';
    return;
  }

  const days = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  const map = new Map();
  for (const cell of heatmapData) {
    map.set(`${cell.day_of_week}-${cell.hour}`, cell);
  }

  let html = '<div></div>'; // top-left empty cell
  for (let h = 0; h < 24; h++) {
    const hh = h < 10 ? `0${h}` : `${h}`;
    html += `<div class="heatmap-hour-header">${hh}</div>`;
  }

  for (let d = 0; d < 7; d++) {
    html += `<div class="heatmap-day-label">${days[d]}</div>`;
    for (let h = 0; h < 24; h++) {
      const cell = map.get(`${d}-${h}`);
      const count = cell ? cell.count : 0;
      let level = 0;
      if (cell && cell.intensity !== undefined) {
        if (cell.intensity > 0.75) level = 4;
        else if (cell.intensity > 0.5) level = 3;
        else if (cell.intensity > 0.25) level = 2;
        else if (cell.intensity > 0) level = 1;
      } else if (count > 0) {
        level = count >= 4 ? 4 : count;
      }
      const hh = h < 10 ? `0${h}:00` : `${h}:00`;
      const tip = `${days[d]} at ${hh} — ${count} ${count === 1 ? 'watch' : 'watches'}`;
      html += `<div class="heatmap-cell" data-level="${level}" title="${escapeHtml(tip)}" data-day="${d}" data-hour="${h}" data-count="${count}"></div>`;
    }
  }

  el.weeklyHeatmapGrid.innerHTML = html;
}

// ==========================================================================
// Recommendations & Overseerr Request Integration
// ==========================================================================
let overseerrConfigured = false;

async function checkOverseerrStatus() {
  try {
    const st = await api.getOverseerrStatus();
    overseerrConfigured = Boolean(st && st.configured);
  } catch (e) {
    overseerrConfigured = false;
  }
}

// ==========================================================================
// Binge Wrapped / Year in Review Modal
// ==========================================================================
let currentWrappedData = null;

async function openWrappedModal(targetYear) {
  if (!el.modalWrapped) return;
  const now = new Date();
  const currentYear = now.getFullYear();
  const year = targetYear || currentYear;

  // Populate year select options
  if (el.wrappedYearSelect) {
    el.wrappedYearSelect.innerHTML = '';
    for (let y = currentYear; y >= currentYear - 5; y--) {
      const opt = document.createElement('option');
      opt.value = y;
      opt.textContent = y;
      if (y === year) opt.selected = true;
      el.wrappedYearSelect.appendChild(opt);
    }
  }

  el.modalWrapped.style.display = 'flex';
  await loadWrappedYear(year);
}

async function loadWrappedYear(year) {
  if (el.wrappedYearBadge) el.wrappedYearBadge.textContent = year;
  if (el.wrappedValHours) el.wrappedValHours.textContent = '...';
  if (el.wrappedValScrobbles) el.wrappedValScrobbles.textContent = '...';
  if (el.wrappedValMovies) el.wrappedValMovies.textContent = '...';
  if (el.wrappedValEpisodes) el.wrappedValEpisodes.textContent = '...';

  try {
    const w = await api.getWrapped(year);
    currentWrappedData = w;
    if (el.wrappedValHours) el.wrappedValHours.textContent = (w.total_hours || 0).toLocaleString();
    if (el.wrappedValScrobbles) el.wrappedValScrobbles.textContent = (w.total_scrobbles || 0).toLocaleString();
    if (el.wrappedValMovies) el.wrappedValMovies.textContent = ((w.total_movies ?? w.movies_watched) || 0).toLocaleString();
    if (el.wrappedValEpisodes) el.wrappedValEpisodes.textContent = ((w.total_episodes ?? w.episodes_watched) || 0).toLocaleString();

    if (w.longest_movie && el.wrappedLongestMovieCard) {
      el.wrappedLongestMovieCard.style.display = 'block';
      if (el.wrappedLongestMovieTitle) el.wrappedLongestMovieTitle.textContent = `${w.longest_movie.title} (${w.longest_movie.year})`;
      if (el.wrappedLongestMovieDur) el.wrappedLongestMovieDur.textContent = `${w.longest_movie.duration_minutes || w.longest_movie.runtime_minutes} minutes`;
    } else if (el.wrappedLongestMovieCard) {
      el.wrappedLongestMovieCard.style.display = 'none';
    }

    if (el.wrappedTopShowsList) {
      if (!w.top_shows || w.top_shows.length === 0) {
        el.wrappedTopShowsList.innerHTML = '<div style="color:var(--text-muted);font-size:0.85rem;padding:8px 0;">No series watched this year.</div>';
      } else {
        el.wrappedTopShowsList.innerHTML = w.top_shows.map((s, idx) => `
          <div class="wrapped-top-card">
            <div class="wrapped-card-poster">
              <span class="wrapped-card-rank">#${idx + 1}</span>
              ${resolveImageUrl(s.thumb_url) ? `<img src="${escapeHtml(resolveImageUrl(s.thumb_url))}" alt="${escapeHtml(s.show_title)}" onerror="this.parentElement.innerHTML='<div class=\\'wrapped-poster-fallback\\'>📺</div>'"/>` : '<div class="wrapped-poster-fallback">📺</div>'}
            </div>
            <div class="wrapped-card-info">
              <div class="wrapped-card-title" title="${escapeHtml(s.show_title)}">${escapeHtml(s.show_title)}</div>
              <div class="wrapped-card-subtitle">${s.count} episodes</div>
            </div>
          </div>
        `).join('');
      }
    }

    if (el.wrappedTopMoviesList) {
      if (!w.top_movies || w.top_movies.length === 0) {
        el.wrappedTopMoviesList.innerHTML = '<div style="color:var(--text-muted);font-size:0.85rem;padding:8px 0;">No movies watched this year.</div>';
      } else {
        el.wrappedTopMoviesList.innerHTML = w.top_movies.map((m, idx) => `
          <div class="wrapped-top-card">
            <div class="wrapped-card-poster">
              <span class="wrapped-card-rank">#${idx + 1}</span>
              ${resolveImageUrl(m.thumb_url) ? `<img src="${escapeHtml(resolveImageUrl(m.thumb_url))}" alt="${escapeHtml(m.title)}" onerror="this.parentElement.innerHTML='<div class=\\'wrapped-poster-fallback\\'>🎬</div>'"/>` : '<div class="wrapped-poster-fallback">🎬</div>'}
            </div>
            <div class="wrapped-card-info">
              <div class="wrapped-card-title" title="${escapeHtml(m.title)}">${escapeHtml(m.title)}${m.year ? ` (${m.year})` : ''}</div>
              <div class="wrapped-card-subtitle">${m.count} ${m.count === 1 ? 'play' : 'plays'}</div>
            </div>
          </div>
        `).join('');
      }
    }
  } catch (err) {
    console.error('Failed to load wrapped:', err);
  }
}

function closeWrappedModal() {
  if (el.modalWrapped) el.modalWrapped.style.display = 'none';
}

if (el.btnOpenWrapped) {
  el.btnOpenWrapped.addEventListener('click', () => openWrappedModal());
}

if (el.btnCloseWrappedModal) {
  el.btnCloseWrappedModal.addEventListener('click', closeWrappedModal);
}

if (el.btnDoneWrappedModal) {
  el.btnDoneWrappedModal.addEventListener('click', closeWrappedModal);
}

if (el.modalWrapped) {
  el.modalWrapped.addEventListener('click', (e) => {
    if (e.target === el.modalWrapped) closeWrappedModal();
  });
}

if (el.wrappedYearSelect) {
  el.wrappedYearSelect.addEventListener('change', (e) => {
    loadWrappedYear(parseInt(e.target.value, 10));
  });
}

if (el.btnCopyWrappedSummary) {
  el.btnCopyWrappedSummary.addEventListener('click', async () => {
    if (!currentWrappedData) return;
    const w = currentWrappedData;
    const totalMovies = w.total_movies ?? w.movies_watched ?? 0;
    const totalEpisodes = w.total_episodes ?? w.episodes_watched ?? 0;
    const text = `✨ My BingeSync Wrapped ${w.year} ✨\n` +
      `⏱️ ${w.total_hours} Hours Watched\n` +
      `🍿 ${totalMovies} Movies Explored\n` +
      `📺 ${totalEpisodes} Episodes Binged\n` +
      `🎯 ${w.total_scrobbles} Total Completed Watches\n` +
      (w.top_shows && w.top_shows.length > 0 ? `🏆 Top Show: ${w.top_shows[0].show_title} (${w.top_shows[0].count} eps)\n` : '') +
      (w.top_movies && w.top_movies.length > 0 ? `🎬 Top Movie: ${w.top_movies[0].title}\n` : '') +
      `Tracked with BingeSync`;

    try {
      await navigator.clipboard.writeText(text);
    } catch (e) {
      // In headless browser / restricted environments, writeText may be unavailable
    }
    el.btnCopyWrappedSummary.innerHTML = '<span>Copied to Clipboard!</span>';
    setTimeout(() => {
      if (el.btnCopyWrappedSummary) {
        el.btnCopyWrappedSummary.innerHTML = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg><span>Copy / Share Card</span>';
      }
    }, 2500);
  });
}

// ==========================================================================
// Duplicate Watch Detector & Cleanup Tool
// ==========================================================================

function openDuplicatesModal() {
  if (!el.modalDuplicates) return;
  el.modalDuplicates.style.display = 'flex';
  scanDuplicates();
}

function closeDuplicatesModal() {
  if (el.modalDuplicates) el.modalDuplicates.style.display = 'none';
  state.duplicates.groups = [];
  state.duplicates.selectedDeleteIds.clear();
}

async function scanDuplicates() {
  if (!el.dupResultsContainer) return;
  state.duplicates.isLoading = true;
  state.duplicates.selectedDeleteIds.clear();
  updateDupSelectionSummary();

  const windowHours = el.dupWindowSelect ? parseInt(el.dupWindowSelect.value, 10) : 24;
  state.duplicates.windowHours = windowHours;

  if (el.dupLoading) el.dupLoading.style.display = 'block';
  if (el.dupEmptyState) el.dupEmptyState.style.display = 'none';
  if (el.dupGroupsList) el.dupGroupsList.innerHTML = '';
  if (el.dupBulkActions) el.dupBulkActions.style.display = 'none';
  if (el.dupStatusBanner) el.dupStatusBanner.style.display = 'none';

  try {
    const res = await api.getDuplicates(windowHours);
    state.duplicates.groups = res.groups || [];

    if (el.dupLoading) el.dupLoading.style.display = 'none';

    if (state.duplicates.groups.length === 0) {
      if (el.dupEmptyState) el.dupEmptyState.style.display = 'block';
      if (el.dupBulkActions) el.dupBulkActions.style.display = 'none';
      if (el.btnDeleteSelectedDuplicates) el.btnDeleteSelectedDuplicates.style.display = 'none';
      return;
    }

    if (el.dupBulkActions) el.dupBulkActions.style.display = 'flex';

    // Default selection: for each group, pre-select the newer duplicates for deletion (keep oldest)
    for (const group of state.duplicates.groups) {
      if (group.scrobbles && group.scrobbles.length > 1) {
        for (let i = 1; i < group.scrobbles.length; i++) {
          state.duplicates.selectedDeleteIds.add(group.scrobbles[i].id);
        }
      }
    }

    renderDuplicateGroups();
    updateDupSelectionSummary();
  } catch (err) {
    if (el.dupLoading) el.dupLoading.style.display = 'none';
    if (el.dupStatusBanner) {
      el.dupStatusBanner.className = 'alert-box alert-error';
      el.dupStatusBanner.textContent = `Error scanning duplicates: ${err.message}`;
      el.dupStatusBanner.style.display = 'block';
    }
  } finally {
    state.duplicates.isLoading = false;
  }
}

function updateDupSelectionSummary() {
  const count = state.duplicates.selectedDeleteIds.size;
  if (el.dupSelectedCount) el.dupSelectedCount.textContent = count;
  if (el.dupSelectionSummary) {
    el.dupSelectionSummary.textContent = count > 0 ? `${count} duplicate scrobble(s) selected for deletion` : 'No scrobbles selected';
  }
  if (el.btnDeleteSelectedDuplicates) {
    el.btnDeleteSelectedDuplicates.style.display = count > 0 ? 'inline-flex' : 'none';
  }
}

function renderDuplicateGroups() {
  if (!el.dupGroupsList) return;
  el.dupGroupsList.innerHTML = '';

  for (const group of state.duplicates.groups) {
    const card = document.createElement('div');
    card.className = 'dup-group-card';

    const isEpisode = group.media_type === 'episode';
    const mainTitle = isEpisode ? (group.grandparent_title || group.title) : group.title;
    const subTitle = isEpisode
      ? `S${String(group.season_number ?? 1).padStart(2, '0')}E${String(group.episode_number ?? 1).padStart(2, '0')}: ${group.title}`
      : (group.year ? `(${group.year})` : '');

    const posterSrc = group.thumb_url || '';
    const posterHtml = posterSrc
      ? `<img src="${posterSrc}" class="dup-group-poster" alt="${escapeHtml(mainTitle)}" onerror="this.onerror=null;this.src='/favicon.svg';" />`
      : `<div class="dup-group-poster" style="display:flex;align-items:center;justify-content:center;color:var(--text-muted);"><svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect width="18" height="18" x="3" y="3" rx="2"/><circle cx="9" cy="2" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/></svg></div>`;

    let scrobblesHtml = '';
    group.scrobbles.forEach((s, idx) => {
      const isSelected = state.duplicates.selectedDeleteIds.has(s.id);
      const isOldest = idx === 0;
      const formattedDate = formatFullDateTime(s.watched_at);
      const statusText = s.watch_status === 'in_progress' ? `${s.progress_percent || 0}% In Progress` : 'Completed';
      const ratingHtml = s.user_rating ? `<span style="color:#eab308;font-weight:600;">★ ${s.user_rating}</span>` : '';

      scrobblesHtml += `
        <div class="dup-scrobble-row ${isSelected ? 'selected-for-delete' : ''}" data-id="${s.id}">
          <div class="dup-scrobble-left">
            <input type="checkbox" class="dup-checkbox" data-id="${s.id}" ${isSelected ? 'checked' : ''} style="cursor: pointer;" />
            <div class="dup-scrobble-meta">
              <span class="dup-scrobble-time">${formattedDate}</span>
              <span class="badge-pill" style="font-size: 0.72rem; padding: 2px 6px;">${statusText}</span>
              ${ratingHtml}
              ${isSelected
                ? `<span class="dup-scrobble-badge-prune">Will Delete</span>`
                : `<span class="dup-scrobble-badge-keep">${isOldest ? 'Original (Keep)' : 'Keep'}</span>`}
            </div>
          </div>
          <button type="button" class="btn btn-ghost btn-sm text-danger btn-delete-single-dup" data-id="${s.id}" title="Delete this scrobble">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
            <span>Delete</span>
          </button>
        </div>
      `;
    });

    card.innerHTML = `
      <div class="dup-group-header">
        <div class="dup-group-media">
          ${posterHtml}
          <div class="dup-group-info">
            <h4 class="dup-group-title">
              <span>${escapeHtml(mainTitle)}</span>
              <span class="badge-pill" style="font-size:0.7rem;text-transform:uppercase;">${group.media_type}</span>
            </h4>
            <div class="dup-group-subtitle">${escapeHtml(subTitle)}</div>
          </div>
        </div>
        <div class="dup-group-badge">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
          <span>${escapeHtml(group.time_difference_display)} (${group.scrobbles.length} watches)</span>
        </div>
      </div>
      <div class="dup-scrobbles-table">
        ${scrobblesHtml}
      </div>
    `;

    // Event listener for checkbox
    card.querySelectorAll('.dup-checkbox').forEach(cb => {
      cb.addEventListener('change', (e) => {
        const id = e.target.dataset.id;
        if (e.target.checked) {
          state.duplicates.selectedDeleteIds.add(id);
        } else {
          state.duplicates.selectedDeleteIds.delete(id);
        }
        renderDuplicateGroups();
        updateDupSelectionSummary();
      });
    });

    // Event listener for single delete button
    card.querySelectorAll('.btn-delete-single-dup').forEach(btn => {
      btn.addEventListener('click', async (e) => {
        e.stopPropagation();
        const id = btn.dataset.id;
        await executeCleanDuplicates({ strategy: 'custom', delete_ids: [id] });
      });
    });

    el.dupGroupsList.appendChild(card);
  }
}

async function executeCleanDuplicates(payload) {
  if (!el.dupStatusBanner) return;
  el.dupStatusBanner.className = 'alert-box alert-info';
  el.dupStatusBanner.textContent = 'Pruning duplicate scrobbles...';
  el.dupStatusBanner.style.display = 'block';

  try {
    const res = await api.cleanDuplicates(payload);
    el.dupStatusBanner.className = 'alert-box alert-success';
    el.dupStatusBanner.textContent = res.message || 'Duplicate scrobbles pruned successfully.';
    el.dupStatusBanner.style.display = 'block';

    // Refresh history / timeline
    loadHistory();
    loadTimeline();

    // Rescan duplicates after short delay
    setTimeout(() => {
      scanDuplicates();
    }, 700);
  } catch (err) {
    el.dupStatusBanner.className = 'alert-box alert-error';
    el.dupStatusBanner.textContent = `Failed to prune duplicates: ${err.message}`;
    el.dupStatusBanner.style.display = 'block';
  }
}

function formatFullDateTime(raw) {
  if (!raw) return '';
  try {
    const d = new Date(raw);
    if (isNaN(d.getTime())) return raw;
    return d.toLocaleString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  } catch (e) {
    return raw;
  }
}

if (el.btnOpenDuplicatesModal) {
  el.btnOpenDuplicatesModal.addEventListener('click', openDuplicatesModal);
}

if (el.btnCleanupFindDuplicates) {
  el.btnCleanupFindDuplicates.addEventListener('click', openDuplicatesModal);
}

if (el.btnCloseDuplicatesModal) {
  el.btnCloseDuplicatesModal.addEventListener('click', closeDuplicatesModal);
}

if (el.btnCloseDuplicatesModalFooter) {
  el.btnCloseDuplicatesModalFooter.addEventListener('click', closeDuplicatesModal);
}

if (el.modalDuplicates) {
  el.modalDuplicates.addEventListener('click', (e) => {
    if (e.target === el.modalDuplicates) closeDuplicatesModal();
  });
}

if (el.btnScanDuplicates) {
  el.btnScanDuplicates.addEventListener('click', scanDuplicates);
}

if (el.dupWindowSelect) {
  el.dupWindowSelect.addEventListener('change', scanDuplicates);
}

if (el.btnCleanKeepOldest) {
  el.btnCleanKeepOldest.addEventListener('click', () => {
    executeCleanDuplicates({ strategy: 'keep_oldest', window_hours: state.duplicates.windowHours });
  });
}

if (el.btnCleanKeepNewest) {
  el.btnCleanKeepNewest.addEventListener('click', () => {
    executeCleanDuplicates({ strategy: 'keep_newest', window_hours: state.duplicates.windowHours });
  });
}

if (el.btnDeleteSelectedDuplicates) {
  el.btnDeleteSelectedDuplicates.addEventListener('click', () => {
    executeCleanDuplicates({ strategy: 'custom', delete_ids: Array.from(state.duplicates.selectedDeleteIds) });
  });
}

// Kickoff
init();

