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
    if (params.search) url += `&search=${encodeURIComponent(params.search)}`;
    if (params.date) url += `&date=${encodeURIComponent(params.date)}`;
    if (params.genre) url += `&genre=${encodeURIComponent(params.genre)}`;
    if (params.person) url += `&person=${encodeURIComponent(params.person)}`;
    if (params.rating && params.rating !== 'all') url += `&rating=${encodeURIComponent(params.rating)}`;
    if (params.watch_status && params.watch_status !== 'all') url += `&watch_status=${encodeURIComponent(params.watch_status)}`;
    if (params.media_type && params.media_type !== 'all') url += `&media_type=${encodeURIComponent(params.media_type)}`;
    return request(url);
  },
  getHistory: (arg1 = {}, limitArg, mediaTypeArg, searchArg) => {
    const params = typeof arg1 === 'number'
      ? { page: arg1, limit: limitArg || 20, media_type: mediaTypeArg || 'all', search: searchArg || '' }
      : arg1;
    let url = `/history?page=${params.page || 1}&limit=${params.limit || 20}`;
    if (params.media_type && params.media_type !== 'all') url += `&media_type=${encodeURIComponent(params.media_type)}`;
    if (params.search) url += `&search=${encodeURIComponent(params.search)}`;
    if (params.date) url += `&date=${encodeURIComponent(params.date)}`;
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
  getStats: () => request('/stats'),
  getNowPlaying: () => request('/now-playing'),
  getWebhookInfo: () => request('/user/webhook'),
  regenerateWebhookToken: () =>
    request('/user/regenerate-token', {
      method: 'POST',
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
  deleteByShow: (showTitle) =>
    request(`/history/by-show?show=${encodeURIComponent(showTitle)}`, {
      method: 'DELETE',
    }),
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
  deleteFromSonarr: (seriesId, deleteFiles = true) =>
    request('/integrations/sonarr/delete-series', {
      method: 'POST',
      body: JSON.stringify({ series_id: seriesId, delete_files: deleteFiles }),
    }),
  getAdminUsers: () => request('/admin/users'),
  deleteAdminUser: (userId) => request(`/admin/users/${userId}`, { method: 'DELETE' }),
};
