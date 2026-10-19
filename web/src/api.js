function getApiBase() {
  const path = window.location.pathname.replace(/\/+$/, '');
  if (path && path !== '' && !path.startsWith('/api')) {
    return `${path}/api/v1`;
  }
  return '/api/v1';
}

export const API_BASE = getApiBase();

async function request(endpoint, options = {}) {
  const config = {
    headers: {
      'Content-Type': 'application/json',
      ...options.headers,
    },
    ...options,
  };

  const response = await fetch(`${API_BASE}${endpoint}`, config);

  if (response.status === 401) {
    // Return unauthorized object
    return { unauthorized: true };
  }

  const contentType = response.headers.get('content-type');
  if (contentType && contentType.includes('application/json')) {
    const data = await response.json();
    if (!response.ok) {
      throw new Error(data.error || `HTTP error ${response.status}`);
    }
    return data;
  }

  if (!response.ok) {
    throw new Error(`HTTP error ${response.status}`);
  }

  return response;
}

export const api = {
  getAuthStatus: () => request('/auth/status'),
  devLogin: (email, displayName) =>
    request('/auth/dev-login', {
      method: 'POST',
      body: JSON.stringify({ email, display_name: displayName }),
    }),
  getMe: () => request('/auth/me'),
  logout: () => request('/auth/logout', { method: 'POST' }),
  getTimeline: (params = {}) => {
    let url = `/timeline?limit=${params.limit || 50}&offset=${params.offset || 0}`;
    if (params.all || params.all_results) url += `&all=true`;
    if (params.all_dates) url += `&all_dates=true`;
    if (params.search) url += `&search=${encodeURIComponent(params.search)}`;
    if (params.date && !params.all_dates) url += `&date=${encodeURIComponent(params.date)}`;
    if (params.genre) url += `&genre=${encodeURIComponent(params.genre)}`;
    if (params.person) url += `&person=${encodeURIComponent(params.person)}`;
    if (params.rating && params.rating !== 'all') url += `&rating=${encodeURIComponent(params.rating)}`;
    if (params.watch_status && params.watch_status !== 'all') url += `&watch_status=${encodeURIComponent(params.watch_status)}`;
    if (params.media_type && params.media_type !== 'all') url += `&media_type=${encodeURIComponent(params.media_type)}`;
    return request(url);
  },
  getTimelinePeriods: () => request('/timeline/periods'),
  getHistory: (arg1 = {}, limitArg, mediaTypeArg, searchArg) => {
    const params = typeof arg1 === 'number'
      ? { page: arg1, limit: limitArg || 20, media_type: mediaTypeArg || 'all', search: searchArg || '' }
      : arg1;
    let url = `/history?page=${params.page || 1}&limit=${params.limit || 20}`;
    if (params.all || params.all_results) url += `&all=true`;
    if (params.all_dates) url += `&all_dates=true`;
    if (params.media_type && params.media_type !== 'all') url += `&media_type=${encodeURIComponent(params.media_type)}`;
    if (params.search) url += `&search=${encodeURIComponent(params.search)}`;
    if (params.date && !params.all_dates) url += `&date=${encodeURIComponent(params.date)}`;
    if (params.genre) url += `&genre=${encodeURIComponent(params.genre)}`;
    if (params.person) url += `&person=${encodeURIComponent(params.person)}`;
    if (params.rating && params.rating !== 'all') url += `&rating=${encodeURIComponent(params.rating)}`;
    if (params.watch_status && params.watch_status !== 'all') url += `&watch_status=${encodeURIComponent(params.watch_status)}`;
    return request(url);
  },
  createScrobble: (scrobbleData) =>
    request('/history', {
      method: 'POST',
      body: JSON.stringify(scrobbleData),
    }),
  deleteScrobble: (id) =>
    request(`/history/${id}`, {
      method: 'DELETE',
    }),
  rateScrobble: (id, rating) =>
    request(`/history/${id}/rate`, {
      method: 'POST',
      body: JSON.stringify({ rating }),
    }),
  getItemWatches: (id) => request(`/history/${id}/watches`),
  refreshScrobbleMetadata: (id) =>
    request(`/history/${id}/refresh-metadata`, {
      method: 'POST',
    }),
  fixMatch: (id, tvdbId) =>
    request(`/history/${id}/fix-match`, {
      method: 'POST',
      body: JSON.stringify({ tvdb_id: tvdbId }),
    }),
  getStats: (params = {}) => {
    const query = new URLSearchParams();
    if (params.media_type && params.media_type !== 'all') {
      query.append('media_type', params.media_type);
    }
    if (params.time_range && params.time_range !== 'all') {
      query.append('time_range', params.time_range);
    }
    if (params.start_date) {
      query.append('start_date', params.start_date);
    }
    if (params.end_date) {
      query.append('end_date', params.end_date);
    }
    const qStr = query.toString();
    return request(qStr ? `/stats?${qStr}` : '/stats');
  },
  getNowPlaying: () => request('/now-playing'),
  getWebhookInfo: () => request('/user/webhook'),
  regenerateWebhookToken: () =>
    request('/user/regenerate-token', {
      method: 'POST',
    }),
  getUserSettings: () => request('/user/settings'),
  updateUserSettings: (data) =>
    request('/user/settings', {
      method: 'POST',
      body: JSON.stringify(data),
    }),
  importTrakt: (payload, isZip = false) => {
    if (isZip || payload instanceof ArrayBuffer || payload instanceof Uint8Array || payload instanceof Blob) {
      return request('/import/trakt', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/zip',
        },
        body: payload,
      });
    }
    return request('/import/trakt', {
      method: 'POST',
      body: typeof payload === 'string' ? payload : JSON.stringify(payload),
    });
  },
  getTautulliUsers: (url, apiKey) => {
    const params = new URLSearchParams();
    if (url) params.append('url', url);
    if (apiKey) params.append('api_key', apiKey);
    const qs = params.toString();
    return request(`/import/tautulli/users${qs ? '?' + qs : ''}`);
  },
  importTautulli: (payload) => {
    if (payload instanceof FormData) {
      return request('/import/tautulli', {
        method: 'POST',
        body: payload,
      });
    }
    return request('/import/tautulli', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(payload),
    });
  },
  deleteByShow: (showTitle) =>
    request(`/history/by-show?show=${encodeURIComponent(showTitle)}`, {
      method: 'DELETE',
    }),
  deleteHistoryByMedia: (mediaType, title, year) => {
    let url = `/history/by-media?media_type=${encodeURIComponent(mediaType)}&title=${encodeURIComponent(title)}`;
    if (year) url += `&year=${year}`;
    return request(url, { method: 'DELETE' });
  },
  deleteBatch: (ids) =>
    request('/history/delete-batch', {
      method: 'POST',
      body: JSON.stringify({ ids }),
    }),
  deleteByDate: (dateStr) =>
    request(`/history/by-date?date=${encodeURIComponent(dateStr)}`, {
      method: 'DELETE',
    }),
  getIntegrationsStatus: () => request('/integrations/status'),
  getCleanupItems: () => request('/integrations/cleanup'),
  checkMediaInLibrary: (params) => {
    let url = `/integrations/check?title=${encodeURIComponent(params.title)}&media_type=${encodeURIComponent(params.media_type)}`;
    if (params.year) url += `&year=${params.year}`;
    if (params.grandparent_title) url += `&grandparent_title=${encodeURIComponent(params.grandparent_title)}`;
    return request(url);
  },
  deleteFromRadarr: (movieId, deleteFiles = true) =>
    request('/integrations/radarr/delete', {
      method: 'POST',
      body: JSON.stringify({ movie_id: movieId, delete_files: deleteFiles }),
    }),
  deleteFromSonarr: (seriesId, options = {}) => {
    let body = { series_id: seriesId };
    if (typeof options === 'boolean') {
      body.delete_files = options;
      body.action = 'delete_series';
    } else {
      body.action = options.action || 'delete_series';
      body.delete_files = options.delete_files !== false;
      if (options.season_number != null) {
        body.season_number = Number(options.season_number);
      }
    }
    return request('/integrations/sonarr/delete-series', {
      method: 'POST',
      body: JSON.stringify(body),
    });
  },
  getAdminUsers: () => request('/admin/users'),
  deleteAdminUser: (userId) => request(`/admin/users/${userId}`, { method: 'DELETE' }),
  getAdminSettings: () => request('/admin/settings'),
  updateAdminSettings: (data) => request('/admin/settings', { method: 'POST', body: JSON.stringify(data) }),
  getAdminLogs: (params = {}) => {
    let url = `/admin/logs?limit=${params.limit || 200}`;
    if (params.level) url += `&level=${encodeURIComponent(params.level)}`;
    return request(url);
  },
  clearAdminLogs: () => request('/admin/logs', { method: 'DELETE' }),
  getAdminTasks: () => request('/admin/tasks'),
  triggerAdminTask: (taskId, force = false) =>
    request(`/admin/tasks/${taskId}/run${force ? '?force=true' : ''}`, { method: 'POST' }),
  forceRefreshMetadata: () => request('/admin/metadata/force-refresh', { method: 'POST' }),
  forceRefreshPosters: () => request('/admin/images/force-refresh', { method: 'POST' }),
  forceRefreshUserCache: () => request('/admin/users-cache/force-refresh', { method: 'POST' }),
  updateAdminTaskSchedule: (taskId, intervalHours) =>
    request(`/admin/tasks/${taskId}/schedule`, {
      method: 'POST',
      body: JSON.stringify({ interval_hours: intervalHours }),
    }),
  getBackups: () => request('/admin/backups'),
  createBackup: () => request('/admin/backups/create', { method: 'POST' }),
  restoreBackup: (filename) =>
    request('/admin/backups/restore', {
      method: 'POST',
      body: JSON.stringify({ filename }),
    }),
  deleteBackup: (filename) =>
    request(`/admin/backups/${encodeURIComponent(filename)}`, { method: 'DELETE' }),
};
