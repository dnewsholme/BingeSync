import { api, API_BASE } from './api.js';

// Application State
const state = {
  user: null,
  authStatus: null,
  activeTab: 'view-timeline',
  timeline: {
    limit: 60,
    offset: 0,
    search: '',
    date: '',
    genre: '',
    person: '',
    rating: 'all',
    watch_status: 'all',
  },
  history: {
    page: 1,
    limit: 12,
    mediaType: 'all',
    search: '',
    totalPages: 1,
    selectMode: false,
    selectedIds: new Set(),
  },
  stats: {
    filter: 'all',
    data: null,
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
  btnRefreshTimeline: document.getElementById('btn-refresh-timeline'),
  timelineSearch: document.getElementById('timeline-search'),
  timelineDateFilter: document.getElementById('timeline-date-filter'),
  timelineGenreFilter: document.getElementById('timeline-genre-filter'),
  timelinePersonFilter: document.getElementById('timeline-person-filter'),
  timelineRatingFilter: document.getElementById('timeline-rating-filter'),
  timelineStatusFilter: document.getElementById('timeline-status-filter'),
  btnResetTimelineFilters: document.getElementById('btn-reset-timeline-filters'),

  // History View
  historySearch: document.getElementById('history-search'),
  historyList: document.getElementById('history-list'),
  historyPagination: document.getElementById('history-pagination'),
  paginationInfo: document.getElementById('pagination-info'),
  btnPagePrev: document.getElementById('btn-page-prev'),
  btnPageNext: document.getElementById('btn-page-next'),
  pills: document.querySelectorAll('.pill:not(.pill-action)'),
  btnToggleSelectMode: document.getElementById('btn-toggle-select-mode'),
  historyBulkBar: document.getElementById('history-bulk-bar'),
  bulkSelectAllCheckbox: document.getElementById('bulk-select-all-checkbox'),
  bulkSelectedCount: document.getElementById('bulk-selected-count'),
  btnDeleteSelected: document.getElementById('btn-delete-selected'),
  btnCancelSelectMode: document.getElementById('btn-cancel-select-mode'),

  // Stats View
  statScrobbles: document.getElementById('stat-scrobbles'),
  statHours: document.getElementById('stat-hours'),
  statMinutes: document.getElementById('stat-minutes'),
  statMovies: document.getElementById('stat-movies'),
  statEpisodes: document.getElementById('stat-episodes'),
  statMovieTime: document.getElementById('stat-movie-time'),
  statEpisodeTime: document.getElementById('stat-episode-time'),
  statStreak: document.getElementById('stat-streak'),
  topShowsList: document.getElementById('top-shows-list'),
  topShowsSection: document.getElementById('top-shows-section'),
  statsFilterPills: document.querySelectorAll('#stats-filter-pills .pill'),
  activityChart: document.getElementById('activity-chart'),
  genresChart: document.getElementById('genres-chart'),

  // Webhook View
  webhookUrlInput: document.getElementById('webhook-url-input'),
  btnCopyWebhook: document.getElementById('btn-copy-webhook'),
  copyStatus: document.getElementById('copy-status'),
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
  modalImportTrakt: document.getElementById('modal-import-trakt'),
  btnCloseImportModal: document.getElementById('btn-close-import-modal'),
  btnCancelImportModal: document.getElementById('btn-cancel-import-modal'),
  formImportTrakt: document.getElementById('form-import-trakt'),
  importFileInput: document.getElementById('import-file-input'),
  importPasteInput: document.getElementById('import-paste-input'),
  importStatusBox: document.getElementById('import-status-box'),

  modalItemDetail: document.getElementById('modal-item-detail'),
  btnCloseDetailModal: document.getElementById('btn-close-detail-modal'),
  btnDismissDetailModal: document.getElementById('btn-dismiss-detail-modal'),
  btnDeleteDetailItem: document.getElementById('btn-delete-detail-item'),
  detailTitle: document.getElementById('detail-title'),
  detailBody: document.getElementById('detail-body'),
  detailImdbLink: document.getElementById('detail-imdb-link'),
  btnDeleteShowScrobbles: document.getElementById('btn-delete-show-scrobbles'),

  // Admin User Management
  tabAdmin: document.getElementById('tab-admin'),
  adminUsersTbody: document.getElementById('admin-users-tbody'),
  btnRefreshAdminUsers: document.getElementById('btn-refresh-admin-users'),
  adminBackchannelUrlInput: document.getElementById('admin-backchannel-url-input'),
  btnCopyBackchannelUrl: document.getElementById('btn-copy-backchannel-url'),
};

// Initialization
async function init() {
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

  // Populate webhook input
  if (user.webhook_url) {
    el.webhookUrlInput.value = user.webhook_url;
  }

  // Show Admin tab if user has admin role
  if (user.role === 'admin') {
    if (el.tabAdmin) el.tabAdmin.style.display = 'inline-flex';
  } else {
    if (el.tabAdmin) el.tabAdmin.style.display = 'none';
  }

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
  if (targetViewId === 'view-settings') loadWebhookInfo();
  if (targetViewId === 'view-admin') loadAdminUsers();
}

el.navTabs.forEach(tab => {
  tab.addEventListener('click', () => switchTab(tab.dataset.target));
});

// View 1: Timeline
async function loadTimeline() {
  try {
    const days = await api.getTimeline({
      limit: state.timeline.limit,
      offset: state.timeline.offset,
      search: state.timeline.search,
      date: state.timeline.date,
      genre: state.timeline.genre,
      person: state.timeline.person,
      rating: state.timeline.rating,
      watch_status: state.timeline.watch_status,
    });
    renderTimeline(days);
  } catch (err) {
    console.error('Error loading timeline:', err);
    el.timelineContainer.innerHTML = `<div class="empty-state"><p>Failed to load timeline. Please try again.</p></div>`;
  }
}

function renderTimeline(days) {
  if (!days || days.length === 0) {
    const hasFilters = Boolean(
      state.timeline.search ||
      state.timeline.date ||
      state.timeline.genre ||
      state.timeline.person ||
      (state.timeline.rating && state.timeline.rating !== 'all') ||
      (state.timeline.watch_status && state.timeline.watch_status !== 'all')
    );
    const msg = hasFilters
      ? 'No watch history matches your timeline filters.'
      : 'No watch history yet. Connect your Plex Webhook or log a scrobble to see your timeline!';

    el.timelineContainer.innerHTML = `
      <div class="empty-state">
        <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
        <p>${msg}</p>
      </div>`;
    return;
  }

  el.timelineContainer.innerHTML = days.map(day => `
    <div class="timeline-day" data-date="${day.date}">
      <div class="timeline-day-header">
        <div style="display:flex;align-items:center;gap:12px;">
          <div class="timeline-date-title">${formatDisplayDate(day.date)}</div>
          <button class="day-header-delete-btn" data-date="${day.date}" title="Delete all scrobbles on ${day.date}">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
            Delete Day
          </button>
        </div>
        <div class="timeline-day-stats">
          <span class="stat-chip">${day.total_count} ${day.total_count === 1 ? 'item' : 'items'}</span>
          <span class="stat-chip">${day.total_minutes} min watched</span>
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
      if (confirm(`Delete all scrobbles logged on ${dateStr}? This action cannot be undone.`)) {
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

// Timeline Filter Event Listeners
let timelineSearchTimer = null;
if (el.timelineSearch) {
  el.timelineSearch.addEventListener('input', (e) => {
    clearTimeout(timelineSearchTimer);
    timelineSearchTimer = setTimeout(() => {
      state.timeline.search = e.target.value.trim();
      loadTimeline();
    }, 300);
  });
}

if (el.timelineDateFilter) {
  el.timelineDateFilter.addEventListener('change', (e) => {
    state.timeline.date = e.target.value;
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

if (el.btnResetTimelineFilters) {
  el.btnResetTimelineFilters.addEventListener('click', () => {
    state.timeline.search = '';
    state.timeline.date = '';
    state.timeline.genre = '';
    state.timeline.person = '';
    state.timeline.rating = 'all';
    state.timeline.watch_status = 'all';

    if (el.timelineSearch) el.timelineSearch.value = '';
    if (el.timelineDateFilter) el.timelineDateFilter.value = '';
    if (el.timelineGenreFilter) el.timelineGenreFilter.value = '';
    if (el.timelinePersonFilter) el.timelinePersonFilter.value = '';
    if (el.timelineRatingFilter) el.timelineRatingFilter.value = 'all';
    if (el.timelineStatusFilter) el.timelineStatusFilter.value = 'all';

    loadTimeline();
  });
}

function renderScrobbleCard(item, isHistory = false) {
  const isEpisode = item.media_type === 'episode';
  const badgeClass = isEpisode ? 'badge-episode' : 'badge-movie';
  const badgeText = isEpisode
    ? `S${String(item.season_number || 1).padStart(2, '0')}E${String(item.episode_number || 1).padStart(2, '0')}`
    : 'Movie';

  const subTitle = isEpisode && item.grandparent_title
    ? `${item.grandparent_title}`
    : (item.year ? `${item.year}` : '');

  const durationMin = item.duration_ms ? `${Math.round(item.duration_ms / 60000)}m` : '';

  const posterHtml = item.thumb_url
    ? `<img src="${escapeHtml(item.thumb_url)}" alt="${escapeHtml(item.title)}" class="scrobble-poster" onerror="this.outerHTML='<div class=\\'scrobble-poster\\'>🎬</div>'"/>`
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

  return `
    <div class="${cardClasses.join(' ')}" data-id="${item.id}">
      ${checkboxHtml}
      ${posterHtml}
      <div class="scrobble-header-badges">
        <span class="scrobble-badge ${badgeClass}">${badgeText}</span>
        ${ratingHtml}
        <a href="${imdbUrl}" target="_blank" rel="noopener noreferrer" class="card-imdb-link" title="Open on IMDb" onclick="event.stopPropagation()">IMDb ↗</a>
      </div>
      <div class="scrobble-card-title">${escapeHtml(item.title)}</div>
      ${subTitle ? `<div class="scrobble-card-sub">${escapeHtml(subTitle)}</div>` : ''}
      <div class="scrobble-card-meta">
        <span>${formatTime(item.watched_at)}</span>
        <span>${durationMin}</span>
      </div>
    </div>
  `;
}

function attachScrobbleCardListeners(container, items, isHistory = false) {
  const itemMap = new Map(items.map(i => [i.id, i]));
  container.querySelectorAll('.scrobble-card').forEach(card => {
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

// View 2: History
async function loadHistory() {
  try {
    const res = await api.getHistory({
      page: state.history.page,
      limit: state.history.limit,
      media_type: state.history.mediaType,
      search: state.history.search,
    });

    state.history.totalPages = res.total_pages || 1;
    state.history.currentItems = res.items || [];
    renderHistory(res);
  } catch (err) {
    console.error('Error loading history:', err);
    el.historyList.innerHTML = `<div class="empty-state"><p>Error loading history.</p></div>`;
  }
}

function renderHistory(res) {
  if (!res.items || res.items.length === 0) {
    el.historyList.innerHTML = `
      <div class="empty-state">
        <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>
        <p>No scrobbles match your search criteria.</p>
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
  el.paginationInfo.textContent = `Page ${res.page} of ${res.total_pages} (${res.total} total)`;
  el.btnPagePrev.disabled = res.page <= 1;
  el.btnPageNext.disabled = res.page >= res.total_pages;
}

// Multi-Select Handlers
function updateBulkBarState() {
  if (!el.historyBulkBar) return;
  const count = state.history.selectedIds.size;
  if (el.bulkSelectedCount) el.bulkSelectedCount.textContent = `${count} selected`;
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

el.pills.forEach(pill => {
  pill.addEventListener('click', () => {
    el.pills.forEach(p => p.classList.remove('active'));
    pill.classList.add('active');
    state.history.mediaType = pill.dataset.type;
    state.history.page = 1;
    loadHistory();
  });
});

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
    const s = await api.getStats();
    state.stats.data = s;
    renderStats();
  } catch (err) {
    console.error('Error loading stats:', err);
  }
}

function renderStats() {
  const s = state.stats.data;
  if (!s) return;

  const filter = state.stats.filter;
  const isAll = filter === 'all';
  const isMovie = filter === 'movie';
  const isTv = filter === 'episode';

  // Total counts based on filter
  if (isMovie) {
    el.statScrobbles.textContent = s.movies_watched;
    const mHours = Math.floor((s.movie_minutes || 0) / 60);
    const mMins = (s.movie_minutes || 0) % 60;
    el.statHours.textContent = mMins > 0 ? `${mHours}h ${mMins}m` : `${mHours}h`;
    el.statMinutes.textContent = `${s.movie_minutes || 0} minutes of films watched`;
  } else if (isTv) {
    el.statScrobbles.textContent = s.episodes_watched;
    const eHours = Math.floor((s.episode_minutes || 0) / 60);
    const eMins = (s.episode_minutes || 0) % 60;
    el.statHours.textContent = eMins > 0 ? `${eHours}h ${eMins}m` : `${eHours}h`;
    el.statMinutes.textContent = `${s.episode_minutes || 0} minutes of series watched`;
  } else {
    el.statScrobbles.textContent = s.total_scrobbles;
    const hours = Math.floor(s.total_minutes / 60);
    const mins = s.total_minutes % 60;
    el.statHours.textContent = mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
    el.statMinutes.textContent = `${s.total_minutes} total minutes watched`;
  }

  el.statMovies.textContent = s.movies_watched;
  el.statEpisodes.textContent = s.episodes_watched;
  el.statStreak.textContent = `${s.current_streak_days} 🔥`;

  if (el.statMovieTime) {
    const mH = Math.floor((s.movie_minutes || 0) / 60);
    el.statMovieTime.textContent = `${mH}h in films`;
  }
  if (el.statEpisodeTime) {
    const eH = Math.floor((s.episode_minutes || 0) / 60);
    el.statEpisodeTime.textContent = `${eH}h in TV series`;
  }

  // Toggle Top Shows section
  if (el.topShowsSection) {
    el.topShowsSection.style.display = isMovie ? 'none' : 'block';
  }

  // Render Top Shows
  if (!isMovie) {
    if (!s.top_shows || s.top_shows.length === 0) {
      el.topShowsList.innerHTML = `<div class="empty-state"><p>No TV show history recorded yet.</p></div>`;
    } else {
      el.topShowsList.innerHTML = s.top_shows.map(show => `
        <div class="top-show-item">
          <div class="top-show-thumb">
            ${show.thumb_url
              ? `<img src="${escapeHtml(show.thumb_url)}" alt="${escapeHtml(show.show_title)}" onerror="this.outerHTML='📺'"/>`
              : '📺'}
          </div>
          <div class="top-show-info">
            <div class="top-show-name">${escapeHtml(show.show_title)}</div>
            <div class="top-show-count">${show.count} ${show.count === 1 ? 'episode' : 'episodes'} watched</div>
          </div>
        </div>
      `).join('');
    }
  }

  // Render Daily Activity Chart (Past 14 Days)
  if (el.activityChart) {
    if (!s.daily_activity || s.daily_activity.length === 0) {
      el.activityChart.innerHTML = `<div class="empty-state" style="padding:20px;width:100%;"><p>No recent activity data.</p></div>`;
    } else {
      const maxTotal = Math.max(...s.daily_activity.map(a => a.total), 1);
      el.activityChart.innerHTML = s.daily_activity.map(item => {
        const moviePct = (item.movies / maxTotal) * 100;
        const episodePct = (item.episodes / maxTotal) * 100;
        const totalPct = (item.total / maxTotal) * 100;
        const dateParts = item.date.split('-');
        const shortDate = dateParts.length === 3 ? `${dateParts[1]}/${dateParts[2]}` : item.date;

        return `
          <div class="activity-bar-group" title="${item.date}: ${item.movies} movies, ${item.episodes} episodes (${item.total} total)">
            <div class="activity-bar-stacked" style="height: ${Math.max(totalPct, 8)}%;">
              <div class="bar-segment-movie" style="height: ${item.total > 0 ? (item.movies / item.total) * 100 : 0}%;"></div>
              <div class="bar-segment-episode" style="height: ${item.total > 0 ? (item.episodes / item.total) * 100 : 0}%;"></div>
            </div>
            <span class="activity-bar-label">${shortDate}</span>
          </div>
        `;
      }).join('');
    }
  }

  // Render Top Genres Breakdown
  if (el.genresChart) {
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
}

// Stats filter pills event listeners
if (el.statsFilterPills) {
  el.statsFilterPills.forEach(pill => {
    pill.addEventListener('click', () => {
      el.statsFilterPills.forEach(p => p.classList.remove('active'));
      pill.classList.add('active');
      state.stats.filter = pill.dataset.type;
      renderStats();
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

      el.nowPlayingDevice.textContent = p.player_title ? `on ${p.player_title}` : '';
      el.nowPlayingState.textContent = p.state === 'paused' ? 'PAUSED' : 'NOW WATCHING';

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
  } catch (err) {
    console.error('Error loading webhook info:', err);
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

el.btnRegenToken.addEventListener('click', async () => {
  if (confirm('Are you sure you want to regenerate your webhook token? You will need to update the webhook URL in Plex.')) {
    try {
      const res = await api.regenerateWebhookToken();
      el.webhookUrlInput.value = res.webhook_url;
      alert('Webhook token regenerated successfully!');
    } catch (err) {
      alert(`Error regenerating token: ${err.message}`);
    }
  }
});

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

// Modal: Trakt Import
if (el.btnOpenImportModal) {
  el.btnOpenImportModal.addEventListener('click', () => {
    el.formImportTrakt.reset();
    el.importStatusBox.style.display = 'none';
    el.importStatusBox.className = 'import-status-box';
    el.modalImportTrakt.style.display = 'flex';
  });
}

if (el.btnCloseImportModal) {
  el.btnCloseImportModal.addEventListener('click', () => {
    el.modalImportTrakt.style.display = 'none';
  });
}

if (el.btnCancelImportModal) {
  el.btnCancelImportModal.addEventListener('click', () => {
    el.modalImportTrakt.style.display = 'none';
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

// Modal: Detail View
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
  const ratingBadge = item.user_rating
    ? `<div style="font-size:0.85rem;color:var(--text-dim);">Rating: <strong style="color:#fbbf24;">★ ${item.user_rating}/10</strong></div>`
    : '';

  const genresBadge = item.genres
    ? `<div style="font-size:0.85rem;color:var(--text-dim);">Genres: <strong style="color:#fff;">${escapeHtml(item.genres)}</strong></div>`
    : '';
  const directorBadge = item.directors
    ? `<div style="font-size:0.85rem;color:var(--text-dim);">Director: <strong style="color:#fff;">${escapeHtml(item.directors)}</strong></div>`
    : '';
  const actorsBadge = item.actors
    ? `<div style="font-size:0.85rem;color:var(--text-dim);">Cast: <strong style="color:#fff;">${escapeHtml(item.actors)}</strong></div>`
    : '';

  el.detailBody.innerHTML = `
    ${sub}
    <div style="display:flex;gap:16px;margin-bottom:16px;flex-wrap:wrap;">
      <div style="font-size:0.85rem;color:var(--text-dim);">Release: <strong style="color:#fff;">${yearStr}</strong></div>
      <div style="font-size:0.85rem;color:var(--text-dim);">Duration: <strong style="color:#fff;">${durationStr}</strong></div>
      <div style="font-size:0.85rem;color:var(--text-dim);">Watched: <strong style="color:#fff;">${watchedStr}</strong></div>
      ${ratingBadge}
      ${genresBadge}
      ${directorBadge}
      ${actorsBadge}
    </div>
    ${item.overview ? `<p style="font-size:0.92rem;line-height:1.6;color:var(--text-main);margin-bottom:16px;">${escapeHtml(item.overview)}</p>` : '<p style="color:var(--text-dim);font-style:italic;">No plot overview available.</p>'}
    <div id="detail-arr-container"></div>
    <div style="font-size:0.78rem;font-family:monospace;color:var(--text-dim);background:#080c14;padding:8px 12px;border-radius:6px;margin-top:14px;">
      Scrobble ID: ${item.id}
    </div>
  `;

  // IMDb button setup
  if (el.detailImdbLink) {
    el.detailImdbLink.href = getImdbUrl(item);
    el.detailImdbLink.style.display = 'inline-flex';
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

  el.modalItemDetail.style.display = 'flex';

  // Check Radarr / Sonarr Library integration
  checkLibraryIntegrations(item);
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
      const badgeClass = isRadarr ? 'arr-badge-radarr' : 'arr-badge-sonarr';
      const serviceName = isRadarr ? 'Radarr' : 'Sonarr';

      container.innerHTML = `
        <div class="arr-cleanup-card">
          <div class="arr-info">
            <span class="arr-badge ${badgeClass}">Found in ${serviceName}</span>
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
          if (confirm(`Remove "${res.title || item.title}" from ${serviceName} and delete all media files from disk?`)) {
            try {
              if (isRadarr) {
                await api.deleteFromRadarr(res.movie_id, true);
              } else {
                await api.deleteFromSonarr(res.series_id, true);
              }
              container.innerHTML = `
                <div style="font-size:0.85rem;color:#10b981;padding:8px 0;">
                  ✓ Successfully removed from ${serviceName} and deleted disk files.
                </div>
              `;
            } catch (err) {
              alert(`Failed to clean up from ${serviceName}: ${err.message}`);
            }
          }
        });
      }
    }
  } catch (err) {
    // Non-fatal if integrations are not configured
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

// Formatters
function formatDisplayDate(dateStr) {
  const parts = dateStr.split('-');
  if (parts.length === 3) {
    const d = new Date(parts[0], parts[1] - 1, parts[2]);
    const today = new Date();
    today.setHours(0, 0, 0, 0);

    const diff = Math.round((today - d) / (1000 * 60 * 60 * 24));
    if (diff === 0) return 'Today';
    if (diff === 1) return 'Yesterday';

    return d.toLocaleDateString(undefined, {
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
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
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
  el.btnRefreshAdminUsers.addEventListener('click', () => loadAdminUsers());
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

// Kickoff
init();
