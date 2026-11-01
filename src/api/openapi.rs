use axum::{
    http::header,
    response::{Html, IntoResponse},
};

pub const OPENAPI_SPEC_JSON: &str = r###"{
    "openapi": "3.0.3",
        "info": {
            "title": "BingeSync API",
            "version": "1.0.0",
            "description": "High-performance, self-hosted watch history scrobbler, timeline tracker, and Trakt/Tautulli alternative with Plex webhooks, multi-user isolation, OIDC SSO, and Radarr/Sonarr integrations.",
            "contact": {
                "name": "BingeSync Support",
                "url": "https://github.com/dnewsholme/BingeSync"
            },
            "license": {
                "name": "MIT"
            }
        },
        "tags": [
            { "name": "Auth", "description": "Authentication, OIDC SSO, PKCE, Dev Login, and Back-Channel Logout" },
            { "name": "Webhooks", "description": "Plex media scrobbling and playback session webhooks" },
            { "name": "History", "description": "Watch history search, filtering, manual logging, ratings, and deletion" },
            { "name": "Timeline & Stats", "description": "Daily watch timeline, playback metrics, and top media analytics" },
            { "name": "Imports", "description": "Trakt.tv and Tautulli watch history and ratings import engines" },
            { "name": "Integrations", "description": "Radarr & Sonarr library checks and post-watch cleanup" },
            { "name": "Images", "description": "Disk-based cached media posters and artwork" },
            { "name": "User", "description": "Per-user webhook token retrieval and regeneration" },
            { "name": "Admin", "description": "User administration, system settings, live logs, and background tasks" }
        ],
        "paths": {
            "/api/v1/setup/status": {
                "get": {
                    "tags": ["Setup"],
                    "summary": "Check first-run setup status",
                    "description": "Returns whether the instance requires initial OIDC configuration wizard setup.",
                    "responses": {
                        "200": { "description": "Instance setup status and connection URLs" }
                    }
                }
            },
            "/api/v1/setup/test-oidc": {
                "post": {
                    "tags": ["Setup"],
                    "summary": "Test OIDC discovery URL",
                    "description": "Tests connectivity and validates the .well-known/openid-configuration endpoint of an OIDC issuer.",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["issuer_url"],
                                    "properties": {
                                        "issuer_url": { "type": "string" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "OIDC discovery succeeded with discovered endpoints" },
                        "400": { "description": "Invalid issuer URL" },
                        "502": { "description": "Failed to connect to issuer discovery endpoint" }
                    }
                }
            },
            "/api/v1/setup/save": {
                "post": {
                    "tags": ["Setup"],
                    "summary": "Complete initial OIDC setup",
                    "description": "Saves OIDC credentials and marks setup complete when no users exist.",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["oidc_issuer_url", "oidc_client_id"],
                                    "properties": {
                                        "oidc_issuer_url": { "type": "string" },
                                        "oidc_client_id": { "type": "string" },
                                        "oidc_client_secret": { "type": "string" },
                                        "oidc_redirect_url": { "type": "string" },
                                        "oidc_admin_group": { "type": "string" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Setup saved successfully" },
                        "400": { "description": "Missing required fields or invalid URL" },
                        "403": { "description": "Setup already completed on this instance" }
                    }
                }
            },
            "/api/v1/auth/status": {
                "get": {
                    "tags": ["Auth"],
                    "summary": "Get authentication capabilities",
                    "description": "Returns whether OIDC and dev login modes are enabled on this BingeSync instance.",
                    "responses": {
                        "200": { "description": "Authentication capabilities status" }
                    }
                }
            },
            "/api/v1/auth/login": {
                "get": {
                    "tags": ["Auth"],
                    "summary": "Initiate OIDC login",
                    "description": "Generates a PKCE challenge and redirects user to configured OIDC provider.",
                    "responses": {
                        "302": { "description": "Redirects to OIDC authorization endpoint" }
                    }
                }
            },
            "/api/v1/auth/callback": {
                "get": {
                    "tags": ["Auth"],
                    "summary": "OIDC OAuth callback",
                    "description": "Exchanges authorization code for tokens, sets session cookie, and redirects to frontend.",
                    "parameters": [
                        { "name": "code", "in": "query", "required": true, "schema": { "type": "string" } },
                        { "name": "state", "in": "query", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "302": { "description": "Redirects to app with session established" }
                    }
                }
            },
            "/api/v1/auth/dev-login": {
                "post": {
                    "tags": ["Auth"],
                    "summary": "Local / Dev sign in",
                    "description": "Signs in or registers a local user when DEV_LOGIN_ENABLED is true.",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["email"],
                                    "properties": {
                                        "email": { "type": "string", "format": "email" },
                                        "display_name": { "type": "string" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Successfully authenticated with JWT token" },
                        "403": { "description": "Dev login disabled" }
                    }
                }
            },
            "/api/v1/auth/me": {
                "get": {
                    "tags": ["Auth"],
                    "summary": "Get current user profile",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Authenticated user profile" },
                        "401": { "description": "Unauthorized" }
                    }
                }
            },
            "/api/v1/auth/logout": {
                "post": {
                    "tags": ["Auth"],
                    "summary": "Sign out current session",
                    "responses": {
                        "200": { "description": "Session cookie cleared" }
                    }
                }
            },
            "/api/v1/auth/backchannel-logout": {
                "post": {
                    "tags": ["Auth"],
                    "summary": "OIDC Back-Channel Logout",
                    "description": "Receives logout_token from OIDC provider to immediately invalidate user sessions and tokens.",
                    "responses": {
                        "200": { "description": "User session invalidated successfully" },
                        "400": { "description": "Invalid logout token" }
                    }
                }
            },
            "/api/v1/webhooks/plex": {
                "post": {
                    "tags": ["Webhooks"],
                    "summary": "Plex webhook endpoint (query param token)",
                    "parameters": [
                        { "name": "token", "in": "query", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Webhook processed" },
                        "401": { "description": "Invalid webhook token" }
                    }
                }
            },
            "/api/v1/webhooks/plex/{token}": {
                "post": {
                    "tags": ["Webhooks"],
                    "summary": "Plex webhook endpoint (path token)",
                    "parameters": [
                        { "name": "token", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Webhook processed" },
                        "401": { "description": "Invalid webhook token" }
                    }
                }
            },
            "/api/v1/webhooks/jellyfin": {
                "post": {
                    "tags": ["Webhooks"],
                    "summary": "Jellyfin webhook endpoint (query param token)",
                    "parameters": [
                        { "name": "token", "in": "query", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Webhook processed" },
                        "201": { "description": "Media scrobbled" },
                        "401": { "description": "Invalid webhook token" }
                    }
                }
            },
            "/api/v1/webhooks/jellyfin/{token}": {
                "post": {
                    "tags": ["Webhooks"],
                    "summary": "Jellyfin webhook endpoint (path token)",
                    "parameters": [
                        { "name": "token", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Webhook processed" },
                        "201": { "description": "Media scrobbled" },
                        "401": { "description": "Invalid webhook token" }
                    }
                }
            },
            "/api/v1/history": {
                "get": {
                    "tags": ["History"],
                    "summary": "List and search watch history",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "q", "in": "query", "description": "Search keyword", "schema": { "type": "string" } },
                        { "name": "media_type", "in": "query", "description": "Filter by movie or episode", "schema": { "type": "string", "enum": ["all", "movie", "episode"] } },
                        { "name": "page", "in": "query", "schema": { "type": "integer", "default": 1 } },
                        { "name": "page_size", "in": "query", "schema": { "type": "integer", "default": 25 } },
                        { "name": "date", "in": "query", "description": "Filter by exact date YYYY-MM-DD", "schema": { "type": "string" } },
                        { "name": "genre", "in": "query", "schema": { "type": "string" } },
                        { "name": "person", "in": "query", "schema": { "type": "string" } },
                        { "name": "rating", "in": "query", "description": "Filter by rating threshold (e.g. 10, 9+, 8+, 7+, 6+, 5+, 4+, 3+, 2+, 1+), rated, or unrated", "schema": { "type": "string" } },
                        { "name": "watch_status", "in": "query", "schema": { "type": "string", "enum": ["all", "completed", "in_progress"] } }
                    ],
                    "responses": {
                        "200": { "description": "Paginated history response" },
                        "401": { "description": "Unauthorized" }
                    }
                },
                "post": {
                    "tags": ["History"],
                    "summary": "Manually log a watch record",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["title", "media_type"],
                                    "properties": {
                                        "title": { "type": "string" },
                                        "media_type": { "type": "string", "enum": ["movie", "episode"] },
                                        "year": { "type": "integer" },
                                        "watched_at": { "type": "string", "format": "date-time" },
                                        "grandparent_title": { "type": "string" },
                                        "season_number": { "type": "integer" },
                                        "episode_number": { "type": "integer" },
                                        "duration_ms": { "type": "integer" },
                                        "user_rating": { "type": "integer", "minimum": 1, "maximum": 10 }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "201": { "description": "Scrobble created" }
                    }
                }
            },
            "/api/v1/history/{id}": {
                "delete": {
                    "tags": ["History"],
                    "summary": "Delete scrobble by ID",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Scrobble deleted" },
                        "404": { "description": "Scrobble not found" }
                    }
                }
            },
            "/api/v1/history/{id}/rate": {
                "post": {
                    "tags": ["History"],
                    "summary": "Rate or unrate a watched item",
                    "description": "Set a user rating between 1 and 10, or send null to remove the rating.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "rating": { "type": "integer", "nullable": true, "minimum": 1, "maximum": 10 }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Rating updated successfully" },
                        "400": { "description": "Rating must be between 1 and 10" },
                        "404": { "description": "Scrobble not found" }
                    }
                },
                "put": {
                    "tags": ["History"],
                    "summary": "Rate or unrate a watched item (idempotent PUT)",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "rating": { "type": "integer", "nullable": true, "minimum": 1, "maximum": 10 }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Rating updated successfully" },
                        "400": { "description": "Rating must be between 1 and 10" },
                        "404": { "description": "Scrobble not found" }
                    }
                }
            },
            "/api/v1/history/{id}/watches": {
                "get": {
                    "tags": ["History"],
                    "summary": "Get other watches of the same item",
                    "description": "Retrieves other watch instances for this user matching the same movie or exact same TV episode.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "List of other watches" }
                    }
                }
            },
            "/api/v1/history/by-show": {
                "delete": {
                    "tags": ["History"],
                    "summary": "Delete all scrobbles for a TV show",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "show", "in": "query", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Show scrobbles deleted" }
                    }
                }
            },
            "/api/v1/history/delete-batch": {
                "post": {
                    "tags": ["History"],
                    "summary": "Batch delete scrobbles",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["ids"],
                                    "properties": {
                                        "ids": { "type": "array", "items": { "type": "string" } }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Batch deleted successfully" }
                    }
                }
            },
            "/api/v1/history/by-date": {
                "delete": {
                    "tags": ["History"],
                    "summary": "Delete all scrobbles on a specific date",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "date", "in": "query", "required": true, "schema": { "type": "string", "example": "2026-10-17" } }
                    ],
                    "responses": {
                        "200": { "description": "Scrobbles for date deleted" }
                    }
                }
            },
            "/api/v1/history/duplicates": {
                "get": {
                    "tags": ["History"],
                    "summary": "Detect duplicate scrobbles within a time window",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "window_hours", "in": "query", "required": false, "schema": { "type": "integer", "default": 24, "example": 24 } }
                    ],
                    "responses": {
                        "200": { "description": "Duplicate scrobbles grouped by media item and time window" }
                    }
                }
            },
            "/api/v1/history/duplicates/clean": {
                "post": {
                    "tags": ["History"],
                    "summary": "Clean and prune duplicate scrobbles",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "required": false,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "strategy": { "type": "string", "enum": ["keep_oldest", "keep_newest", "custom"], "default": "keep_oldest" },
                                        "window_hours": { "type": "integer", "default": 24 },
                                        "delete_ids": { "type": "array", "items": { "type": "string" } }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Pruning results" }
                    }
                }
            },
            "/api/v1/timeline": {
                "get": {
                    "tags": ["Timeline & Stats"],
                    "summary": "Get grouped daily watch timeline",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "page", "in": "query", "schema": { "type": "integer", "default": 1 } },
                        { "name": "page_size", "in": "query", "schema": { "type": "integer", "default": 50 } },
                        { "name": "all", "in": "query", "description": "Set to true to load all timeline records", "schema": { "type": "boolean" } },
                        { "name": "search", "in": "query", "schema": { "type": "string" } },
                        { "name": "genre", "in": "query", "schema": { "type": "string" } },
                        { "name": "person", "in": "query", "schema": { "type": "string" } },
                        { "name": "rating", "in": "query", "description": "Filter by rating threshold (e.g. 10, 9+, 8+, 7+, 6+, 5+, 4+, 3+, 2+, 1+), rated, or unrated", "schema": { "type": "string" } },
                        { "name": "watch_status", "in": "query", "schema": { "type": "string" } },
                        { "name": "date", "in": "query", "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Timeline grouped by day" }
                    }
                }
            },
            "/api/v1/stats": {
                "get": {
                    "tags": ["Timeline & Stats"],
                    "summary": "Get user watch statistics and top charts",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Summary stats, top shows, top movies, and monthly breakdown" }
                    }
                }
            },
            "/api/v1/now-playing": {
                "get": {
                    "tags": ["Timeline & Stats"],
                    "summary": "Get current active playback sessions",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "List of active playback sessions" }
                    }
                }
            },
            "/api/v1/user/webhook": {
                "tags": ["User"],
                "get": {
                    "summary": "Get user webhook token and URLs",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Webhook configuration URLs" }
                    }
                }
            },
            "/api/v1/user/regenerate-token": {
                "post": {
                    "tags": ["User"],
                    "summary": "Regenerate unique webhook token",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "New webhook token issued" }
                    }
                }
            },
            "/api/v1/user/api-keys": {
                "get": {
                    "tags": ["User"],
                    "summary": "List personal API keys",
                    "description": "Lists all active API keys generated by the authenticated user.",
                    "security": [{ "BearerAuth": [] }, { "ApiKeyAuth": [] }],
                    "responses": {
                        "200": { "description": "List of user API keys with prefixes and usage timestamps" }
                    }
                },
                "post": {
                    "tags": ["User"],
                    "summary": "Generate new personal API key",
                    "description": "Generates a new API key scoped to the authenticated user's permissions and data. The plaintext key is only returned once.",
                    "security": [{ "BearerAuth": [] }, { "ApiKeyAuth": [] }],
                    "requestBody": {
                        "required": false,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "name": { "type": "string", "example": "Home Assistant" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "201": { "description": "API key generated successfully" }
                    }
                }
            },
            "/api/v1/user/api-keys/{id}": {
                "delete": {
                    "tags": ["User"],
                    "summary": "Revoke personal API key",
                    "description": "Permanently revokes and deletes a personal API key.",
                    "security": [{ "BearerAuth": [] }, { "ApiKeyAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "API key revoked successfully" },
                        "404": { "description": "API key not found" }
                    }
                }
            },
            "/api/v1/user/settings/plex/users": {
                "post": {
                    "tags": ["User"],
                    "summary": "Fetch Plex users",
                    "description": "Queries configured or provided Plex Media Server instance for registered accounts and users.",
                    "security": [{ "BearerAuth": [] }, { "ApiKeyAuth": [] }],
                    "responses": {
                        "200": { "description": "List of Plex users" }
                    }
                },
                "get": {
                    "tags": ["User"],
                    "summary": "Fetch Plex users (GET)",
                    "security": [{ "BearerAuth": [] }, { "ApiKeyAuth": [] }],
                    "responses": {
                        "200": { "description": "List of Plex users" }
                    }
                }
            },
            "/api/v1/user/settings/jellyfin/users": {
                "post": {
                    "tags": ["User"],
                    "summary": "Fetch Jellyfin users",
                    "description": "Queries configured or provided Jellyfin server instance for registered users.",
                    "security": [{ "BearerAuth": [] }, { "ApiKeyAuth": [] }],
                    "responses": {
                        "200": { "description": "List of Jellyfin users" }
                    }
                },
                "get": {
                    "tags": ["User"],
                    "summary": "Fetch Jellyfin users (GET)",
                    "security": [{ "BearerAuth": [] }, { "ApiKeyAuth": [] }],
                    "responses": {
                        "200": { "description": "List of Jellyfin users" }
                    }
                }
            },
            "/api/v1/import/trakt": {
                "post": {
                    "tags": ["Imports"],
                    "summary": "Import Trakt history & ratings",
                    "description": "Imports Trakt export data from JSON, CSV, or ZIP multi-file archive with automatic chronological sorting.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Import result statistics" }
                    }
                }
            },
            "/api/v1/import/tautulli": {
                "post": {
                    "tags": ["Imports"],
                    "summary": "Import Tautulli history",
                    "description": "Imports playback history from Tautulli live API, uploaded JSON, or CSV with deduplication against existing records.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Tautulli import result summary" }
                    }
                }
            },
            "/api/v1/import/tautulli/users": {
                "get": {
                    "tags": ["Imports"],
                    "summary": "Fetch Tautulli users",
                    "description": "Queries configured or provided Tautulli instance for registered users.",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "url", "in": "query", "schema": { "type": "string" } },
                        { "name": "api_key", "in": "query", "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "List of Tautulli users" }
                    }
                }
            },
            "/api/v1/images/{filename}": {
                "get": {
                    "tags": ["Images"],
                    "summary": "Serve cached media artwork",
                    "parameters": [
                        { "name": "filename", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Image binary data" },
                        "404": { "description": "Image not found" }
                    }
                }
            },
            "/api/v1/integrations/status": {
                "get": {
                    "tags": ["Integrations"],
                    "summary": "Check Radarr/Sonarr service status",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Integration connectivity status" }
                    }
                }
            },
            "/api/v1/integrations/cleanup": {
                "get": {
                    "tags": ["Integrations"],
                    "summary": "List watched films and TV shows that still exist in Radarr/Sonarr libraries",
                    "description": "Cross-references user watch history with Radarr and Sonarr instances to identify cleanup candidates and reclaimable disk space.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "List of cleanup candidates with file sizes and season details" }
                    }
                }
            },
            "/api/v1/integrations/check": {
                "get": {
                    "tags": ["Integrations"],
                    "summary": "Check if media exists in Radarr/Sonarr libraries",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "title", "in": "query", "required": true, "schema": { "type": "string" } },
                        { "name": "media_type", "in": "query", "required": true, "schema": { "type": "string", "enum": ["movie", "episode"] } },
                        { "name": "year", "in": "query", "schema": { "type": "integer" } },
                        { "name": "season_number", "in": "query", "schema": { "type": "integer" } }
                    ],
                    "responses": {
                        "200": { "description": "Library presence check result" }
                    }
                }
            },
            "/api/v1/integrations/radarr/delete": {
                "post": {
                    "tags": ["Integrations"],
                    "summary": "Delete movie from Radarr",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["radarr_id"],
                                    "properties": {
                                        "radarr_id": { "type": "integer" },
                                        "delete_files": { "type": "boolean", "default": true }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Movie deleted from Radarr" }
                    }
                }
            },
            "/api/v1/integrations/sonarr/delete-series": {
                "post": {
                    "tags": ["Integrations"],
                    "summary": "Delete series or season files from Sonarr",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "required": ["sonarr_id"],
                                    "properties": {
                                        "sonarr_id": { "type": "integer" },
                                        "action": { "type": "string", "enum": ["delete_all", "delete_season_files", "delete_all_files_keep_series"] },
                                        "season_number": { "type": "integer" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Sonarr series action completed" }
                    }
                }
            },
            "/api/v1/admin/users": {
                "get": {
                    "tags": ["Admin"],
                    "summary": "List all registered users",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "List of users with scrobble counts" },
                        "403": { "description": "Admin access required" }
                    }
                }
            },
            "/api/v1/admin/users/{id}": {
                "delete": {
                    "tags": ["Admin"],
                    "summary": "Cascade delete user and all scrobbles",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "User deleted" },
                        "403": { "description": "Admin access required" }
                    }
                }
            },
            "/api/v1/admin/settings": {
                "get": {
                    "tags": ["Admin"],
                    "summary": "Get system settings",
                    "description": "Returns current system settings including metadata provider configuration (TheTVDB or TMDB), OIDC SSO details, and task schedules with sensitive secrets masked.",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": {
                            "description": "Current system settings",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "metadata_source": { "type": "string", "enum": ["tvdb", "tmdb"], "description": "Active metadata enrichment provider" },
                                            "tvdb_api_key_configured": { "type": "boolean" },
                                            "tvdb_api_key_masked": { "type": "string" },
                                            "tvdb_pin": { "type": "string" },
                                            "tvdb_base_url": { "type": "string" },
                                            "tmdb_api_key_configured": { "type": "boolean" },
                                            "tmdb_api_key_masked": { "type": "string" },
                                            "tmdb_base_url": { "type": "string" },
                                            "oidc_enabled": { "type": "boolean" },
                                            "oidc_issuer_url": { "type": "string" },
                                            "oidc_client_id": { "type": "string" },
                                            "oidc_client_secret_configured": { "type": "boolean" },
                                            "oidc_client_secret_masked": { "type": "string" },
                                            "oidc_redirect_url": { "type": "string" },
                                            "oidc_admin_group": { "type": "string", "description": "Custom OIDC group name(s) granting admin privileges" },
                                            "scrobble_threshold_percent": { "type": "integer" },
                                            "task_image_interval_hours": { "type": "integer" },
                                            "task_tvdb_interval_hours": { "type": "integer" }
                                        }
                                    }
                                }
                            }
                        },
                        "403": { "description": "Admin access required" }
                    }
                },
                "post": {
                    "tags": ["Admin"],
                    "summary": "Update system settings",
                    "description": "Updates system-wide settings including metadata provider selection (TheTVDB or TMDB), API credentials, and background task intervals.",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "metadata_source": { "type": "string", "enum": ["tvdb", "tmdb"], "description": "Active metadata enrichment provider" },
                                        "tvdb_api_key": { "type": "string" },
                                        "tvdb_pin": { "type": "string" },
                                        "tvdb_base_url": { "type": "string" },
                                        "tmdb_api_key": { "type": "string" },
                                        "tmdb_base_url": { "type": "string" },
                                        "oidc_enabled": { "type": "boolean" },
                                        "oidc_issuer_url": { "type": "string" },
                                        "oidc_client_id": { "type": "string" },
                                        "oidc_client_secret": { "type": "string" },
                                        "oidc_redirect_url": { "type": "string" },
                                        "oidc_admin_group": { "type": "string" },
                                        "scrobble_threshold_percent": { "type": "integer" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Settings updated" },
                        "403": { "description": "Admin access required" }
                    }
                },
                "put": {
                    "tags": ["Admin"],
                    "summary": "Update system settings (idempotent PUT)",
                    "description": "Idempotent update for system settings.",
                    "security": [{ "BearerAuth": [] }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "metadata_source": { "type": "string", "enum": ["tvdb", "tmdb"] },
                                        "tvdb_api_key": { "type": "string" },
                                        "tvdb_pin": { "type": "string" },
                                        "tvdb_base_url": { "type": "string" },
                                        "tmdb_api_key": { "type": "string" },
                                        "tmdb_base_url": { "type": "string" },
                                        "oidc_enabled": { "type": "boolean" },
                                        "oidc_issuer_url": { "type": "string" },
                                        "oidc_client_id": { "type": "string" },
                                        "oidc_client_secret": { "type": "string" },
                                        "oidc_redirect_url": { "type": "string" },
                                        "oidc_admin_group": { "type": "string" },
                                        "scrobble_threshold_percent": { "type": "integer" }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Settings updated" },
                        "403": { "description": "Admin access required" }
                    }
                }
            },
            "/api/v1/admin/logs": {
                "get": {
                    "tags": ["Admin"],
                    "summary": "View recent system logs",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "level", "in": "query", "schema": { "type": "string", "enum": ["ALL", "INFO", "WARN", "ERROR"] } },
                        { "name": "search", "in": "query", "schema": { "type": "string" } },
                        { "name": "limit", "in": "query", "schema": { "type": "integer", "default": 200 } }
                    ],
                    "responses": {
                        "200": { "description": "Captured log entries" },
                        "403": { "description": "Admin access required" }
                    }
                },
                "delete": {
                    "tags": ["Admin"],
                    "summary": "Clear log buffer",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Logs cleared" },
                        "403": { "description": "Admin access required" }
                    }
                }
            },
            "/api/v1/admin/tasks": {
                "get": {
                    "tags": ["Admin"],
                    "summary": "List background tasks & status",
                    "security": [{ "BearerAuth": [] }],
                    "responses": {
                        "200": { "description": "Background tasks metrics and statuses" },
                        "403": { "description": "Admin access required" }
                    }
                }
            },
            "/api/v1/admin/tasks/{id}/run": {
                "post": {
                    "tags": ["Admin"],
                    "summary": "Trigger immediate execution of a background task",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "responses": {
                        "200": { "description": "Task triggered" },
                        "403": { "description": "Admin access required" }
                    }
                }
            },
            "/api/v1/admin/tasks/{id}/schedule": {
                "post": {
                    "tags": ["Admin"],
                    "summary": "Update periodic schedule for background task",
                    "security": [{ "BearerAuth": [] }],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "type": "object",
                                    "properties": {
                                        "interval_hours": { "type": "integer", "minimum": 1, "maximum": 168 }
                                    }
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": { "description": "Task schedule updated" },
                        "403": { "description": "Admin access required" }
                    }
                }
            }
        },
        "components": {
            "securitySchemes": {
                "BearerAuth": {
                    "type": "http",
                    "scheme": "bearer",
                    "bearerFormat": "JWT",
                    "description": "JWT Bearer token or personal API key passed in the Authorization header or bingesync_token cookie"
                },
                "ApiKeyAuth": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-Api-Key",
                    "description": "Personal API key scoped to user permissions (bs_live_...)"
                }
            }
        }
    }
"###;

pub async fn get_openapi_json() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/json; charset=utf-8")], OPENAPI_SPEC_JSON)
}

pub async fn get_swagger_ui() -> impl IntoResponse {
    let html = r###"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>BingeSync API - Swagger UI</title>
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <link rel="icon" type="image/svg+xml" href="./favicon.svg">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;600&display=swap" rel="stylesheet">
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css" />
  <style>
    :root {
      --bg-base: #0a0f1d;
      --bg-surface: #0f172a;
      --bg-card: #141f36;
      --text-main: #f8fafc;
      --text-muted: #94a3b8;
      --accent: #8b5cf6;
      --accent-hover: #7c3aed;
      --border: rgba(255, 255, 255, 0.08);
    }
    html, body {
      margin: 0;
      padding: 0;
      background: var(--bg-base);
      color: var(--text-main);
      font-family: 'Inter', -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    }
    .topbar {
      display: none !important;
    }
    .bingesync-custom-header {
      background: rgba(15, 23, 42, 0.95);
      border-bottom: 1px solid var(--border);
      backdrop-filter: blur(12px);
      padding: 14px 28px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      position: sticky;
      top: 0;
      z-index: 100;
    }
    .bingesync-brand {
      display: flex;
      align-items: center;
      gap: 12px;
      text-decoration: none;
      color: #fff;
    }
    .bingesync-brand-title {
      font-size: 1.25rem;
      font-weight: 800;
      letter-spacing: -0.02em;
    }
    .bingesync-brand-title span {
      color: #a78bfa;
    }
    .bingesync-header-links {
      display: flex;
      align-items: center;
      gap: 12px;
    }
    .bingesync-btn {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 6px 14px;
      font-size: 0.85rem;
      font-weight: 600;
      border-radius: 6px;
      text-decoration: none;
      background: rgba(255, 255, 255, 0.06);
      border: 1px solid var(--border);
      color: var(--text-main);
      transition: all 0.15s ease;
    }
    .bingesync-btn:hover {
      background: rgba(255, 255, 255, 0.12);
      border-color: rgba(255, 255, 255, 0.2);
    }
    .bingesync-btn.primary {
      background: linear-gradient(135deg, #7c3aed, #6366f1);
      border-color: transparent;
      color: #fff;
    }
    .bingesync-btn.primary:hover {
      background: linear-gradient(135deg, #6d28d9, #4f46e5);
    }
    #swagger-ui {
      max-width: 1280px;
      margin: 0 auto;
      padding: 24px 20px 60px;
    }
    /* Swagger UI Custom Dark Polish */
    .swagger-ui {
      color: var(--text-main) !important;
    }
    .swagger-ui .info {
      margin: 20px 0 30px !important;
    }
    .swagger-ui .info .title {
      color: #f8fafc !important;
      font-family: 'Inter', sans-serif !important;
      font-weight: 800 !important;
    }
    .swagger-ui .info p, .swagger-ui .info li {
      color: var(--text-muted) !important;
      font-size: 0.95rem !important;
    }
    .swagger-ui .scheme-container {
      background: var(--bg-surface) !important;
      box-shadow: none !important;
      border: 1px solid var(--border) !important;
      border-radius: 8px !important;
      padding: 14px 20px !important;
    }
    .swagger-ui .opblock {
      background: var(--bg-surface) !important;
      border-radius: 8px !important;
      border: 1px solid var(--border) !important;
      box-shadow: 0 4px 12px rgba(0,0,0,0.2) !important;
      margin-bottom: 14px !important;
    }
    .swagger-ui .opblock .opblock-summary {
      padding: 10px 16px !important;
    }
    .swagger-ui .opblock .opblock-summary-method {
      border-radius: 6px !important;
      font-weight: 700 !important;
      font-family: 'JetBrains Mono', monospace !important;
    }
    .swagger-ui .opblock .opblock-summary-path {
      color: #f1f5f9 !important;
      font-family: 'JetBrains Mono', monospace !important;
      font-size: 0.92rem !important;
    }
    .swagger-ui .opblock .opblock-summary-description {
      color: var(--text-muted) !important;
    }
    .swagger-ui .opblock-body {
      background: #080c16 !important;
    }
    .swagger-ui table thead tr th, .swagger-ui table thead tr td {
      color: var(--text-muted) !important;
      border-bottom: 1px solid var(--border) !important;
    }
    .swagger-ui .parameter__name {
      color: #a78bfa !important;
      font-family: 'JetBrains Mono', monospace !important;
    }
    .swagger-ui .parameter__type {
      color: var(--text-muted) !important;
    }
    .swagger-ui select, .swagger-ui input[type=text] {
      background: #0f172a !important;
      color: #fff !important;
      border: 1px solid rgba(255,255,255,0.15) !important;
      border-radius: 6px !important;
    }
    .swagger-ui .btn {
      border-radius: 6px !important;
    }
    .swagger-ui section.models {
      background: var(--bg-surface) !important;
      border: 1px solid var(--border) !important;
      border-radius: 8px !important;
    }
    .swagger-ui section.models h4 {
      color: #f8fafc !important;
    }
    .swagger-ui .model-box {
      background: #080c16 !important;
    }
    /* Fallback box if CDN is unavailable */
    #fallback-viewer {
      display: none;
      background: var(--bg-surface);
      border: 1px solid var(--border);
      border-radius: 8px;
      padding: 24px;
      margin-top: 20px;
    }
  </style>
</head>
<body>
  <header class="bingesync-custom-header">
    <a href="./" class="bingesync-brand">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#a78bfa" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
        <path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/>
        <path d="M3 3v5h5"/>
        <path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16"/>
        <path d="M16 21h5v-5"/>
        <polygon points="10 8 16 12 10 16 10 8" fill="#a78bfa" stroke="none"/>
      </svg>
      <div class="bingesync-brand-title">Binge<span>Sync</span> <span style="font-size:0.85rem;color:var(--text-muted);font-weight:500;">API Docs</span></div>
    </a>
    <div class="bingesync-header-links">
      <a id="bingesync-spec-link" href="./api/v1/openapi.json" target="_blank" class="bingesync-btn">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>
        OpenAPI Spec (.json)
      </a>
      <a id="bingesync-back-link" href="./" class="bingesync-btn primary">
        &larr; Back to App
      </a>
    </div>
  </header>

  <div id="swagger-ui">
    <div id="swagger-loading" style="text-align:center;padding:60px 20px;color:var(--text-muted);">
      <h3 style="color:#fff;margin-bottom:8px;">Loading BingeSync Interactive API Docs...</h3>
      <p>Initialising Swagger UI and OpenAPI 3.0 specification.</p>
    </div>
  </div>

  <div id="fallback-viewer" style="max-width:1280px;margin:20px auto;display:none;">
    <h2>BingeSync REST API Specification</h2>
    <p style="color:var(--text-muted);">Interactive Swagger UI bundle could not be loaded from CDN. You can explore the raw JSON specification below or view the endpoints.</p>
    <div style="margin:16px 0;">
      <a id="fallback-spec-btn" href="./api/v1/openapi.json" target="_blank" class="bingesync-btn primary">View / Download openapi.json</a>
    </div>
    <pre id="fallback-json" style="background:#080c14;padding:16px;border-radius:6px;overflow-x:auto;max-height:600px;font-family:monospace;font-size:0.85rem;color:#38bdf8;"></pre>
  </div>

  <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js" onerror="handleSwaggerLoadFallback()"></script>
  <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-standalone-preset.js"></script>
  <script>
    function getApiSpecUrl() {
      var loc = window.location;
      var path = loc.pathname;
      var m = path.match(/^(.*)\/(?:docs|swagger)\/?$/);
      if (m) {
        var base = m[1] || '';
        return base + '/api/v1/openapi.json';
      }
      return './api/v1/openapi.json';
    }

    function updateNavLinks() {
      var specUrl = getApiSpecUrl();
      var specLink = document.getElementById('bingesync-spec-link');
      if (specLink) specLink.href = specUrl;
      var fallbackBtn = document.getElementById('fallback-spec-btn');
      if (fallbackBtn) fallbackBtn.href = specUrl;

      var backLink = document.getElementById('bingesync-back-link');
      if (backLink) {
        var m = window.location.pathname.match(/^(.*)\/(?:docs|swagger)\/?$/);
        var base = m && m[1] ? m[1] : '';
        backLink.href = base ? (base + '/') : './';
      }
    }

    function initSwagger() {
      var loadingEl = document.getElementById('swagger-loading');
      if (typeof SwaggerUIBundle !== 'undefined') {
        if (loadingEl) loadingEl.style.display = 'none';
        SwaggerUIBundle({
          url: getApiSpecUrl(),
          dom_id: '#swagger-ui',
          deepLinking: true,
          presets: [
            SwaggerUIBundle.presets.apis,
            SwaggerUIStandalonePreset
          ],
          layout: "BaseLayout",
          docExpansion: "list",
          defaultModelsExpandDepth: 1,
          persistAuthorization: true,
          validatorUrl: null
        });
      } else {
        handleSwaggerLoadFallback();
      }
    }

    var _hasTriedCdnjs = false;
    function handleSwaggerLoadFallback() {
      if (!_hasTriedCdnjs) {
        _hasTriedCdnjs = true;
        // Try fallback CDN (cdnjs)
        var s1 = document.createElement('script');
        s1.src = 'https://cdnjs.cloudflare.com/ajax/libs/swagger-ui/5.18.2/swagger-ui-bundle.min.js';
        var s2 = document.createElement('script');
        s2.src = 'https://cdnjs.cloudflare.com/ajax/libs/swagger-ui/5.18.2/swagger-ui-standalone-preset.min.js';
        s1.onload = function() {
          if (typeof SwaggerUIBundle !== 'undefined') {
            initSwagger();
          } else {
            handleSwaggerLoadError();
          }
        };
        s1.onerror = handleSwaggerLoadError;
        document.head.appendChild(s1);
        document.head.appendChild(s2);
        return;
      }
      handleSwaggerLoadError();
    }

    function handleSwaggerLoadError() {
      var loadingEl = document.getElementById('swagger-loading');
      if (loadingEl) loadingEl.style.display = 'none';
      var fallback = document.getElementById('fallback-viewer');
      if (fallback) {
        fallback.style.display = 'block';
        fetch(getApiSpecUrl())
          .then(function(res) { return res.json(); })
          .then(function(data) {
            var el = document.getElementById('fallback-json');
            if (el) el.textContent = JSON.stringify(data, null, 2);
          })
          .catch(function(e) {
            var el = document.getElementById('fallback-json');
            if (el) el.textContent = "Could not fetch openapi.json: " + e;
          });
      }
    }

    updateNavLinks();
    if (document.readyState === 'loading') {
      document.addEventListener('DOMContentLoaded', initSwagger);
    } else {
      initSwagger();
    }
  </script>
</body>
</html>
"###;

    Html(html)
}
