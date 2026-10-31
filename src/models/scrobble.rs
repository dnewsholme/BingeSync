use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Scrobble {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub media_type: String,
    pub grandparent_title: Option<String>,
    pub parent_title: Option<String>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub year: Option<i32>,
    pub overview: Option<String>,
    pub thumb_url: Option<String>,
    pub rating_key: Option<String>,
    pub guid: Option<String>,
    pub duration_ms: Option<i64>,
    pub user_rating: Option<i32>,
    pub genres: Option<String>,
    pub directors: Option<String>,
    #[sqlx(default)]
    pub director: Option<String>,
    #[sqlx(default)]
    pub cinematographer: Option<String>,
    #[sqlx(default)]
    pub tvdb_id: Option<String>,
    #[sqlx(default)]
    pub tmdb_id: Option<String>,
    pub actors: Option<String>,
    pub watch_status: Option<String>,
    pub progress_percent: Option<i64>,
    pub view_offset_ms: Option<i64>,
    #[serde(default)]
    pub metadata_updated_at: Option<String>,
    #[sqlx(default)]
    pub tags: Option<String>,
    #[sqlx(default)]
    pub notes: Option<String>,
    #[sqlx(default)]
    pub source: Option<String>,
    #[sqlx(default)]
    #[serde(alias = "watched_by")]
    pub source_user: Option<String>,
    #[sqlx(default)]
    #[serde(alias = "import_from")]
    pub imported_from: Option<String>,
    pub watched_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewScrobbleRequest {
    pub title: String,
    pub media_type: String,
    pub grandparent_title: Option<String>,
    pub parent_title: Option<String>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub year: Option<i32>,
    pub overview: Option<String>,
    pub thumb_url: Option<String>,
    pub duration_ms: Option<i64>,
    pub user_rating: Option<i32>,
    pub genres: Option<String>,
    pub directors: Option<String>,
    pub director: Option<String>,
    pub cinematographer: Option<String>,
    pub tvdb_id: Option<String>,
    pub tmdb_id: Option<String>,
    pub actors: Option<String>,
    pub watch_status: Option<String>,
    pub progress_percent: Option<i64>,
    pub view_offset_ms: Option<i64>,
    pub tags: Option<String>,
    pub notes: Option<String>,
    pub source: Option<String>,
    #[serde(alias = "watched_by")]
    pub source_user: Option<String>,
    #[serde(default, alias = "import_from")]
    pub imported_from: Option<String>,
    pub watched_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FixMatchRequest {
    #[serde(default)]
    pub tvdb_id: Option<String>,
    #[serde(default)]
    pub tmdb_id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryResponse {
    pub items: Vec<Scrobble>,
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    pub total_pages: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineDay {
    pub date: String,
    pub total_count: usize,
    pub total_minutes: i64,
    pub items: Vec<Scrobble>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopShowItem {
    pub show_title: String,
    pub count: i64,
    pub thumb_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopMovieItem {
    pub title: String,
    pub count: i64,
    pub thumb_url: Option<String>,
    pub year: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyActivityItem {
    pub date: String,
    pub movies: i64,
    pub episodes: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenreCountItem {
    pub genre: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopPersonItem {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayOfWeekActivity {
    pub day: String,
    pub day_index: u32,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeOfDayActivity {
    pub period: String,
    pub count: i64,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeatmapCell {
    pub day_index: u32,
    pub hour: u32,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LongestMovieItem {
    pub title: String,
    pub duration_minutes: i64,
    pub year: Option<i32>,
    pub thumb_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrappedSummary {
    pub year: i32,
    pub total_scrobbles: i64,
    pub total_hours: f64,
    pub total_minutes: i64,
    pub total_movies: i64,
    pub total_episodes: i64,
    pub unique_shows: i64,
    pub busiest_day: Option<String>,
    pub busiest_day_count: i64,
    pub top_shows: Vec<TopShowItem>,
    pub top_movies: Vec<TopMovieItem>,
    pub top_actors: Vec<TopPersonItem>,
    pub top_directors: Vec<TopPersonItem>,
    pub top_genres: Vec<GenreCountItem>,
    pub longest_movie: Option<LongestMovieItem>,
    pub available_years: Vec<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsSummary {
    pub total_scrobbles: i64,
    pub movies_watched: i64,
    pub episodes_watched: i64,
    #[serde(default)]
    pub unique_movies: i64,
    #[serde(default)]
    pub unique_shows: i64,
    pub total_minutes: i64,
    pub movie_minutes: i64,
    pub episode_minutes: i64,
    #[serde(default)]
    pub avg_daily_minutes: i64,
    pub current_streak_days: i64,
    #[serde(default)]
    pub longest_streak_days: i64,
    #[serde(default)]
    pub average_rating: Option<f64>,
    #[serde(default)]
    pub total_rated: i64,
    #[serde(default)]
    pub completion_rate_percent: f64,
    pub top_shows: Vec<TopShowItem>,
    #[serde(default)]
    pub top_movies: Vec<TopMovieItem>,
    pub daily_activity: Vec<DailyActivityItem>,
    pub top_genres: Vec<GenreCountItem>,
    #[serde(default)]
    pub top_actors: Vec<TopPersonItem>,
    #[serde(default)]
    pub top_directors: Vec<TopPersonItem>,
    #[serde(default)]
    pub day_of_week_activity: Vec<DayOfWeekActivity>,
    #[serde(default)]
    pub time_of_day_activity: Vec<TimeOfDayActivity>,
    #[serde(default)]
    pub peak_day: Option<String>,
    #[serde(default)]
    pub peak_day_count: i64,
    #[serde(default)]
    pub weekly_heatmap: Vec<HeatmapCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ActiveSession {
    pub id: String,
    pub user_id: String,
    pub player_title: Option<String>,
    pub title: String,
    pub media_type: String,
    pub grandparent_title: Option<String>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub year: Option<i32>,
    pub thumb_url: Option<String>,
    pub state: String,
    #[sqlx(default)]
    pub source: Option<String>,
    #[sqlx(default)]
    #[serde(alias = "watched_by")]
    pub source_user: Option<String>,
    pub view_offset_ms: i64,
    pub duration_ms: i64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub group_key: String,
    pub media_type: String,
    pub title: String,
    pub grandparent_title: Option<String>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub year: Option<i32>,
    pub thumb_url: Option<String>,
    pub time_difference_display: String,
    pub scrobbles: Vec<Scrobble>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateScanResponse {
    pub total_duplicate_groups: usize,
    pub total_redundant_scrobbles: usize,
    pub window_hours: i64,
    pub groups: Vec<DuplicateGroup>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CleanDuplicatesRequest {
    pub strategy: Option<String>,
    pub delete_ids: Option<Vec<String>>,
    pub window_hours: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanDuplicatesResponse {
    pub success: bool,
    pub deleted_count: usize,
    pub message: String,
}
