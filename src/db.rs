use std::env;

use axum::http::StatusCode;
use chrono::{Duration, Utc};
use uuid::Uuid;

use crate::*;

const INITIAL_SCHEMA_SQL: &str = include_str!("../migrations/001_initial.sql");
const SMTP_CLEANUP_SQL: &str = include_str!("../migrations/002_remove_smtp_replyto_and_notify.sql");

pub(crate) struct Database<'a> {
    conn: &'a mut PgClient,
}

pub(crate) struct CreateMediaAssetRecordInput {
    pub(crate) id: String,
    pub(crate) provider: String,
    pub(crate) bucket: Option<String>,
    pub(crate) object_key: String,
    pub(crate) filename: String,
    pub(crate) original_filename: String,
    pub(crate) mime_type: String,
    pub(crate) extension: Option<String>,
    pub(crate) size: i64,
    pub(crate) url: String,
    pub(crate) usage: String,
    pub(crate) uploaded_by: String,
}

pub(crate) struct MediaStorageLocator {
    pub(crate) provider: String,
    pub(crate) bucket: Option<String>,
    pub(crate) object_key: String,
}

fn serialize_publication_timestamp(
    published_at: Option<String>,
) -> Result<Option<chrono::DateTime<Utc>>, ApiError> {
    published_at
        .map(|value| {
            chrono::DateTime::parse_from_rfc3339(&value)
                .map(|datetime| datetime.with_timezone(&Utc))
                .map_err(|_| {
                    ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "INVALID_PUBLISHED_AT",
                        "publishedAt is invalid",
                    )
                })
        })
        .transpose()
}

impl<'a> Database<'a> {
    pub(crate) fn new(conn: &'a mut PgClient) -> Self {
        Self { conn }
    }

    pub(crate) fn ensure_default_records(&mut self, config: &Config) -> Result<(), ApiError> {
        self.ensure_base_schema()?;
        self.ensure_media_asset_schema()?;
        self.ensure_page_view_schema()?;
        self.ensure_public_site_settings_schema()?;

        let admin_exists = self
            .conn
            .query_one("SELECT COUNT(*) FROM admins", &[])
            .map_err(db_error)?
            .get::<usize, i64>(0)
            > 0;

        if !admin_exists {
            let username = env::var("ADMIN_SEED_USERNAME").unwrap_or_else(|_| "admin".to_string());
            let password =
                env::var("ADMIN_SEED_PASSWORD").unwrap_or_else(|_| "ChangeMe123!".to_string());
            let display_name =
                env::var("ADMIN_SEED_DISPLAY_NAME").unwrap_or_else(|_| "Site Admin".to_string());
            let email =
                env::var("ADMIN_SEED_EMAIL").unwrap_or_else(|_| "admin@example.com".to_string());

            self.conn
                .execute(
                    "INSERT INTO admins (
                        id, username, password_hash, display_name, email, avatar_url, status, last_login_at, created_at, updated_at
                     ) VALUES (
                        $1, $2, $3, $4, $5, NULL, 'active', NULL, NOW(), NOW()
                     )",
                    &[&Uuid::new_v4().to_string(), &username, &hash_password(&password), &display_name, &email],
                )
                .map_err(db_error)?;
        }

        let settings_exists = self
            .conn
            .query_opt(
                "SELECT id FROM public_site_settings WHERE id = $1",
                &[&"default-public-settings"],
            )
            .map_err(db_error)?
            .is_some();
        if !settings_exists {
            let site_title =
                env::var("SITE_DEFAULT_TITLE").unwrap_or_else(|_| "AKSRT Blog".to_string());
            let site_description = env::var("SITE_DEFAULT_DESCRIPTION")
                .unwrap_or_else(|_| "Personal blog about engineering and writing".to_string());
            let logo_url = normalize_optional_text(env::var("SITE_DEFAULT_LOGO_URL").ok());
            let footer_text = env::var("SITE_DEFAULT_FOOTER_TEXT")
                .unwrap_or_else(|_| "Powered by AKSRT Blog".to_string());
            let comment_enabled = env::var("SITE_DEFAULT_COMMENT_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                == "true";
            let seo_title =
                env::var("SITE_DEFAULT_SEO_TITLE").unwrap_or_else(|_| site_title.clone());
            let seo_description = env::var("SITE_DEFAULT_SEO_DESCRIPTION")
                .unwrap_or_else(|_| site_description.clone());
            let seo_keywords = env::var("SITE_DEFAULT_SEO_KEYWORDS")
                .unwrap_or_else(|_| "blog,engineering,writing".to_string());
            let seo_canonical_url = env::var("SITE_DEFAULT_SEO_CANONICAL_URL")
                .unwrap_or_else(|_| config.public_site_url.clone());

            self.conn
                .execute(
                    "INSERT INTO public_site_settings (
                        id, site_title, site_description, logo_url, footer_text, comment_enabled,
                        seo_title, seo_description, seo_keywords, seo_canonical_url,
                        navigation_items_json, footer_links_json, standalone_pages_json,
                        custom_head_code, custom_footer_code, icp_filing, police_filing, show_filing, github_username,
                        about_display_name, about_bio,
                        created_at, updated_at
                     ) VALUES (
                        $1, $2, $3, $4, $5, $6,
                        $7, $8, $9, $10,
                        '[]'::jsonb, '[]'::jsonb, '[]'::jsonb,
                        NULL, NULL, NULL, NULL, FALSE, NULL,
                        NULL, NULL,
                        NOW(), NOW()
                     )",
                    &[
                        &"default-public-settings",
                        &site_title,
                        &site_description,
                        &logo_url,
                        &footer_text,
                        &comment_enabled,
                        &seo_title,
                        &seo_description,
                        &seo_keywords,
                        &seo_canonical_url,
                    ],
                )
                .map_err(db_error)?;
        }

        let storage_exists = self
            .conn
            .query_opt(
                "SELECT id FROM storage_configs WHERE id = $1",
                &[&"default-storage-config"],
            )
            .map_err(db_error)?
            .is_some();
        if !storage_exists {
            let public_base_url = env::var("STORAGE_PUBLIC_BASE_URL")
                .unwrap_or_else(|_| format!("http://{}/uploads", config.bind));
            let base_folder =
                env::var("STORAGE_DEFAULT_BASE_FOLDER").unwrap_or_else(|_| "blog".to_string());
            let endpoint = normalize_optional_text(env::var("STORAGE_DEFAULT_ENDPOINT").ok());
            let region = normalize_optional_text(env::var("STORAGE_DEFAULT_REGION").ok());
            let bucket = normalize_optional_text(env::var("STORAGE_DEFAULT_BUCKET").ok());
            let access_key_id = env::var("STORAGE_DEFAULT_ACCESS_KEY_ID").unwrap_or_default();
            let secret_access_key =
                env::var("STORAGE_DEFAULT_SECRET_ACCESS_KEY").unwrap_or_default();
            let enabled = env::var("STORAGE_DEFAULT_ENABLED")
                .unwrap_or_else(|_| "true".to_string())
                == "true";
            let force_path_style = env::var("STORAGE_DEFAULT_FORCE_PATH_STYLE")
                .unwrap_or_else(|_| "false".to_string())
                == "true";
            let driver = env::var("STORAGE_DRIVER").unwrap_or_else(|_| "local".to_string());

            self.conn
                .execute(
                    "INSERT INTO storage_configs (
                        id, enabled, driver, endpoint, region, bucket, access_key_id, secret_access_key,
                        public_base_url, base_folder, force_path_style, created_at, updated_at
                     ) VALUES (
                        $1, $2, $3, $4, $5, $6, $7, $8,
                        $9, $10, $11, NOW(), NOW()
                     )",
                    &[
                        &"default-storage-config",
                        &enabled,
                        &driver,
                        &endpoint,
                        &region,
                        &bucket,
                        &access_key_id,
                        &secret_access_key,
                        &public_base_url,
                        &base_folder,
                        &force_path_style,
                    ],
                )
                .map_err(db_error)?;
        }

        let smtp_exists = self
            .conn
            .query_opt(
                "SELECT id FROM smtp_configs WHERE id = $1",
                &[&"default-smtp-config"],
            )
            .map_err(db_error)?
            .is_some();
        if !smtp_exists {
            let enabled =
                env::var("SMTP_DEFAULT_ENABLED").unwrap_or_else(|_| "false".to_string()) == "true";
            let host =
                env::var("SMTP_DEFAULT_HOST").unwrap_or_else(|_| "smtp.example.com".to_string());
            let port = env::var("SMTP_DEFAULT_PORT")
                .ok()
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(587);
            let secure =
                env::var("SMTP_DEFAULT_SECURE").unwrap_or_else(|_| "false".to_string()) == "true";
            let username = env::var("SMTP_DEFAULT_USERNAME").unwrap_or_default();
            let password = env::var("SMTP_DEFAULT_PASSWORD").unwrap_or_default();
            let from_name =
                env::var("SMTP_DEFAULT_FROM_NAME").unwrap_or_else(|_| "AKSRT Blog".to_string());
            let from_email = env::var("SMTP_DEFAULT_FROM_EMAIL")
                .unwrap_or_else(|_| "no-reply@example.com".to_string());

            self.conn
                .execute(
                    "INSERT INTO smtp_configs (
                        id, enabled, host, port, secure, username, password, from_name, from_email,
                        created_at, updated_at, last_test_at, last_test_status, last_error_message
                     ) VALUES (
                        $1, $2, $3, $4, $5, $6, $7, $8, $9,
                        NOW(), NOW(), NULL, 'untested', NULL
                     )",
                    &[
                        &"default-smtp-config",
                        &enabled,
                        &host,
                        &port,
                        &secure,
                        &username,
                        &password,
                        &from_name,
                        &from_email,
                    ],
                )
                .map_err(db_error)?;
        }

        let captcha_exists = self
            .conn
            .query_opt(
                "SELECT id FROM captcha_configs WHERE id = $1",
                &[&"default-captcha-config"],
            )
            .map_err(db_error)?
            .is_some();
        if !captcha_exists {
            self.conn
                .execute(
                    "INSERT INTO captcha_configs (
                        id, enabled, provider, captcha_id, captcha_key,
                        enabled_on_comment, enabled_on_friend_link, enabled_on_login,
                        created_at, updated_at
                     ) VALUES (
                        $1, FALSE, 'geetest', '', '',
                        FALSE, FALSE, FALSE,
                        NOW(), NOW()
                     )",
                    &[&"default-captcha-config"],
                )
                .map_err(db_error)?;
        }

        let category_count = self
            .conn
            .query_one("SELECT COUNT(*) FROM article_categories", &[])
            .map_err(db_error)?
            .get::<usize, i64>(0);
        if category_count == 0 {
            self.conn
                .execute(
                    "INSERT INTO article_categories (id, name, slug, description, is_enabled, created_at, updated_at)
                     VALUES
                     ($1, 'Technology', 'technology', 'Technology articles', TRUE, NOW(), NOW()),
                     ($2, 'Life', 'life', 'Life articles', TRUE, NOW(), NOW())",
                    &[&Uuid::new_v4().to_string(), &Uuid::new_v4().to_string()],
                )
                .map_err(db_error)?;
        }

        let tag_count = self
            .conn
            .query_one("SELECT COUNT(*) FROM article_tags", &[])
            .map_err(db_error)?
            .get::<usize, i64>(0);
        if tag_count == 0 {
            self.conn
                .execute(
                    "INSERT INTO article_tags (id, name, slug, created_at, updated_at)
                     VALUES
                     ($1, 'React', 'react', NOW(), NOW()),
                     ($2, 'Node.js', 'nodejs', NOW(), NOW()),
                     ($3, 'Design', 'design', NOW(), NOW())",
                    &[
                        &Uuid::new_v4().to_string(),
                        &Uuid::new_v4().to_string(),
                        &Uuid::new_v4().to_string(),
                    ],
                )
                .map_err(db_error)?;
        }

        Ok(())
    }

    fn ensure_base_schema(&mut self) -> Result<(), ApiError> {
        self.conn.batch_execute(INITIAL_SCHEMA_SQL).map_err(db_error)?;
        self.conn.batch_execute(SMTP_CLEANUP_SQL).map_err(db_error)?;
        Ok(())
    }

    fn ensure_media_asset_schema(&mut self) -> Result<(), ApiError> {
        self.conn
            .batch_execute(
                "ALTER TABLE media_assets
                    ADD COLUMN IF NOT EXISTS title TEXT,
                    ADD COLUMN IF NOT EXISTS alt_text TEXT,
                    ADD COLUMN IF NOT EXISTS caption TEXT,
                    ADD COLUMN IF NOT EXISTS description TEXT;",
            )
            .map_err(db_error)?;

        self.conn
            .execute(
                "UPDATE media_assets
                 SET title = COALESCE(
                    NULLIF(BTRIM(REGEXP_REPLACE(COALESCE(NULLIF(original_filename, ''), filename), '\\.[^.]+$', '')), ''),
                    filename
                 )
                 WHERE title IS NULL OR BTRIM(title) = ''",
                &[],
            )
            .map_err(db_error)?;

        Ok(())
    }

    fn ensure_page_view_schema(&mut self) -> Result<(), ApiError> {
        self.conn
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS page_views (
                    id TEXT PRIMARY KEY,
                    path TEXT NOT NULL,
                    referrer TEXT,
                    user_agent TEXT,
                    ip TEXT,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                );
                CREATE INDEX IF NOT EXISTS idx_page_views_created_at ON page_views(created_at);
                CREATE INDEX IF NOT EXISTS idx_page_views_path ON page_views(path);",
            )
            .map_err(db_error)?;

        Ok(())
    }

    fn ensure_public_site_settings_schema(&mut self) -> Result<(), ApiError> {
        self.conn
            .batch_execute(
                "ALTER TABLE public_site_settings
                    ADD COLUMN IF NOT EXISTS about_display_name TEXT,
                    ADD COLUMN IF NOT EXISTS about_bio TEXT;",
            )
            .map_err(db_error)?;

        Ok(())
    }

    pub(crate) fn issue_admin_auth_result(
        &mut self,
        admin: &AdminRecord,
        ip: Option<String>,
        user_agent: Option<String>,
        update_last_login_at: bool,
    ) -> Result<AdminAuthResult, ApiError> {
        let session_id = Uuid::new_v4().to_string();
        let refresh_token = issue_refresh_token();
        let refresh_token_hash = sha256_hex(&refresh_token);
        let access_expires_in = access_ttl_seconds();
        let refresh_expires_in = refresh_ttl_seconds();
        let access_expires_at = Utc::now() + Duration::seconds(access_expires_in);
        let refresh_expires_at = Utc::now() + Duration::seconds(refresh_expires_in);
        let access_token =
            build_access_token(&admin.id, &session_id, access_expires_at.timestamp());

        self.conn
            .execute(
                "INSERT INTO admin_sessions (
                    id, admin_id, refresh_token_hash, status, ip, user_agent, expires_at, revoked_at, created_at, updated_at
                 ) VALUES (
                    $1, $2, $3, 'active', $4, $5, $6, NULL, NOW(), NOW()
                 )",
                &[&session_id, &admin.id, &refresh_token_hash, &ip, &user_agent, &refresh_expires_at],
            )
            .map_err(db_error)?;

        if update_last_login_at {
            self.conn
                .execute(
                    "UPDATE admins SET last_login_at = NOW(), updated_at = NOW() WHERE id = $1",
                    &[&admin.id],
                )
                .map_err(db_error)?;
        }

        let reloaded = load_admin_by_id(self.conn, &admin.id)?.ok_or_else(|| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ADMIN_NOT_FOUND",
                "Admin could not be reloaded",
            )
        })?;

        Ok(AdminAuthResult {
            access_token,
            refresh_token,
            access_token_expires_at: access_expires_at.to_rfc3339(),
            refresh_token_expires_at: refresh_expires_at.to_rfc3339(),
            admin: to_admin_profile(&reloaded),
        })
    }

    pub(crate) fn revoke_admin_session(&mut self, session_id: &str) -> Result<(), ApiError> {
        self.conn
            .execute(
                "UPDATE admin_sessions SET status = 'revoked', revoked_at = NOW(), updated_at = NOW() WHERE id = $1",
                &[&session_id],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn update_admin_profile(
        &mut self,
        admin_id: &str,
        username: String,
        email: String,
        display_name: String,
    ) -> Result<AdminProfileItem, ApiError> {
        let current = load_admin_by_id(self.conn, admin_id)?.ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Admin is unavailable",
            )
        })?;

        if username != current.username {
            if let Some(existing) = load_admin_by_username(self.conn, &username)? {
                if existing.id != current.id {
                    return Err(ApiError::new(
                        StatusCode::CONFLICT,
                        "USERNAME_EXISTS",
                        "Username already exists",
                    ));
                }
            }
        }

        self.conn
            .execute(
                "UPDATE admins SET username = $1, email = $2, display_name = $3, updated_at = NOW() WHERE id = $4",
                &[&username, &email, &display_name, &current.id],
            )
            .map_err(db_error)?;

        let updated = load_admin_by_id(self.conn, &current.id)?.ok_or_else(|| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "ADMIN_NOT_FOUND",
                "Admin could not be reloaded",
            )
        })?;

        Ok(to_admin_profile(&updated))
    }

    pub(crate) fn change_admin_password(
        &mut self,
        admin_id: &str,
        new_password: &str,
    ) -> Result<(), ApiError> {
        self.conn
            .execute(
                "UPDATE admins SET password_hash = $1, updated_at = NOW() WHERE id = $2",
                &[&hash_password(new_password), &admin_id],
            )
            .map_err(db_error)?;
        self.conn
            .execute(
                "UPDATE admin_sessions SET status = 'revoked', revoked_at = NOW(), updated_at = NOW() WHERE admin_id = $1 AND status = 'active'",
                &[&admin_id],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn create_public_comment(
        &mut self,
        article_id: &str,
        input: CreateCommentInput,
        ip: Option<String>,
        user_agent: Option<String>,
    ) -> Result<MutationResult, ApiError> {
        let nickname = input.nickname.trim().to_string();
        let email = input.email.trim().to_string();
        let website = normalize_optional_text(input.website);
        let content = input.content.trim().to_string();
        let parent_id = normalize_optional_text(input.parent_id);

        require_length(
            &nickname,
            1,
            40,
            "INVALID_NICKNAME",
            "Nickname must be between 1 and 40 characters",
        )?;
        require_length(
            &email,
            3,
            120,
            "INVALID_EMAIL",
            "Email must be between 3 and 120 characters",
        )?;
        require_length(
            &content,
            1,
            2000,
            "INVALID_CONTENT",
            "Comment content must be between 1 and 2000 characters",
        )?;

        if !validate_email(&email) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_EMAIL",
                "Email format is invalid",
            ));
        }

        if let Some(website) = website.as_ref() {
            require_length(
                website,
                1,
                500,
                "INVALID_WEBSITE",
                "Website URL is too long",
            )?;
            if !validate_url(website) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "INVALID_WEBSITE",
                    "Website URL is invalid",
                ));
            }
        }

        if let Some(parent_id) = parent_id.as_ref() {
            let parent = self
                .conn
                .query_opt(
                    "SELECT article_id, status FROM comments WHERE id = $1",
                    &[&parent_id],
                )
                .map_err(db_error)?
                .map(|row| (row.get::<usize, String>(0), row.get::<usize, String>(1)));

            match parent {
                Some((parent_article_id, parent_status))
                    if parent_article_id == article_id && parent_status == "approved" => {}
                Some(_) => {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "INVALID_PARENT_COMMENT",
                        "Parent comment is invalid",
                    ))
                }
                None => {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "INVALID_PARENT_COMMENT",
                        "Parent comment was not found",
                    ))
                }
            }
        }

        let comment_id = Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO comments (
                    id, article_id, parent_id, nickname, email, website, content, status, ip, user_agent, reviewed_by, reviewed_at, reject_reason, created_at, updated_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, 'pending', $8, $9, NULL, NULL, NULL, NOW(), NOW()
                 )",
                &[&comment_id, &article_id, &parent_id, &nickname, &email, &website, &content, &ip, &user_agent],
            )
            .map_err(db_error)?;

        Ok(MutationResult {
            id: comment_id,
            status: "pending".to_string(),
        })
    }

    pub(crate) fn create_friend_link_application(
        &mut self,
        input: CreateFriendLinkApplicationInput,
        ip: Option<String>,
        user_agent: Option<String>,
    ) -> Result<MutationResult, ApiError> {
        let site_name = input.site_name.trim().to_string();
        let site_url = input.site_url.trim().to_string();
        let icon_url = normalize_optional_text(input.icon_url);
        let description = input.description.trim().to_string();
        let contact_name = input.contact_name.trim().to_string();
        let contact_email = input.contact_email.trim().to_string();
        let message = normalize_optional_text(input.message);

        require_length(
            &site_name,
            1,
            120,
            "INVALID_SITE_NAME",
            "Site name must be between 1 and 120 characters",
        )?;
        require_length(
            &site_url,
            1,
            500,
            "INVALID_SITE_URL",
            "Site URL is too long",
        )?;
        require_length(
            &description,
            1,
            300,
            "INVALID_DESCRIPTION",
            "Description must be between 1 and 300 characters",
        )?;
        require_length(
            &contact_name,
            1,
            80,
            "INVALID_CONTACT_NAME",
            "Contact name must be between 1 and 80 characters",
        )?;
        require_length(
            &contact_email,
            3,
            200,
            "INVALID_CONTACT_EMAIL",
            "Contact email is too long",
        )?;

        if !validate_url(&site_url) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_SITE_URL",
                "Site URL is invalid",
            ));
        }

        if let Some(icon_url) = icon_url.as_ref() {
            require_length(icon_url, 1, 500, "INVALID_ICON_URL", "Icon URL is too long")?;
            if !validate_url(icon_url) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "INVALID_ICON_URL",
                    "Icon URL is invalid",
                ));
            }
        }

        if !validate_email(&contact_email) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_CONTACT_EMAIL",
                "Contact email format is invalid",
            ));
        }

        if let Some(message) = message.as_ref() {
            require_length(message, 0, 1000, "INVALID_MESSAGE", "Message is too long")?;
        }

        let application_id = Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO friend_link_applications (
                    id, site_name, site_url, icon_url, description, contact_name, contact_email, message, status, review_note, reviewed_by, reviewed_at, linked_footer_link_id, ip, user_agent, created_at, updated_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, 'pending', NULL, NULL, NULL, NULL, $9, $10, NOW(), NOW()
                 )",
                &[&application_id, &site_name, &site_url, &icon_url, &description, &contact_name, &contact_email, &message, &ip, &user_agent],
            )
            .map_err(db_error)?;

        Ok(MutationResult {
            id: application_id,
            status: "pending".to_string(),
        })
    }

    pub(crate) fn update_public_site_settings(
        &mut self,
        public_site_url: &str,
        input: UpdatePublicSiteSettingsInput,
    ) -> Result<PublicSiteSettingsItem, ApiError> {
        let current = read_public_settings_data(self.conn, public_site_url)?;

        let site_title = input
            .site_title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.site_title.clone());
        let site_description = input
            .site_description
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.site_description.clone());
        let footer_text = input
            .footer_text
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.footer_text.clone());
        let comment_enabled = input.comment_enabled.unwrap_or(current.comment_enabled);
        let seo_keywords = input
            .seo_keywords
            .map(|value| value.trim().to_string())
            .unwrap_or(current.seo_keywords.clone());
        let custom_head_code = input
            .custom_head_code
            .unwrap_or_else(|| current.custom_head_code.clone().unwrap_or_default());
        let custom_footer_code = input
            .custom_footer_code
            .unwrap_or_else(|| current.custom_footer_code.clone().unwrap_or_default());
        let logo_url = match input.logo_url {
            Some(value) => normalize_optional_text(value),
            None => current.logo_url.clone(),
        };
        let icp_filing = match input.icp_filing {
            Some(value) => normalize_optional_text(value),
            None => current.icp_filing.clone(),
        };
        let police_filing = match input.police_filing {
            Some(value) => normalize_optional_text(value),
            None => current.police_filing.clone(),
        };
        let github_username = match input.github_username {
            Some(value) => normalize_optional_text(value),
            None => current.github_username.clone(),
        };
        let about_display_name = match input.about_display_name {
            Some(value) => normalize_optional_text(value),
            None => current.about_display_name.clone(),
        };
        let about_bio = match input.about_bio {
            Some(value) => normalize_optional_text(value),
            None => current.about_bio.clone(),
        };
        let show_filing = input.show_filing.unwrap_or(current.show_filing);

        require_length(
            &site_title,
            1,
            120,
            "INVALID_SITE_TITLE",
            "Site title is invalid",
        )?;
        require_length(
            &site_description,
            1,
            300,
            "INVALID_SITE_DESCRIPTION",
            "Site description is invalid",
        )?;
        require_length(
            &footer_text,
            1,
            300,
            "INVALID_FOOTER_TEXT",
            "Footer text is invalid",
        )?;
        if let Some(url) = logo_url.as_ref() {
            require_length(url, 1, 500, "INVALID_LOGO_URL", "Logo URL is invalid")?;
            if !validate_url(url) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "INVALID_LOGO_URL",
                    "Logo URL is invalid",
                ));
            }
        }
        if let Some(display_name) = about_display_name.as_ref() {
            require_length(
                display_name,
                1,
                120,
                "INVALID_ABOUT_DISPLAY_NAME",
                "About display name is invalid",
            )?;
        }
        if let Some(bio) = about_bio.as_ref() {
            require_length(
                bio,
                1,
                1000,
                "INVALID_ABOUT_BIO",
                "About bio is invalid",
            )?;
        }

        self.conn
            .execute(
                "UPDATE public_site_settings
                 SET site_title = $1, site_description = $2, logo_url = $3, footer_text = $4, comment_enabled = $5,
                     seo_keywords = $6, custom_head_code = $7, custom_footer_code = $8, icp_filing = $9, police_filing = $10,
                     show_filing = $11, github_username = $12, about_display_name = $13, about_bio = $14, updated_at = NOW()
                 WHERE id = $15",
                &[
                    &site_title,
                    &site_description,
                    &logo_url,
                    &footer_text,
                    &comment_enabled,
                    &seo_keywords,
                    &custom_head_code,
                    &custom_footer_code,
                    &icp_filing,
                    &police_filing,
                    &show_filing,
                    &github_username,
                    &about_display_name,
                    &about_bio,
                    &"default-public-settings",
                ],
            )
            .map_err(db_error)?;

        read_public_site_settings(self.conn, public_site_url)
    }

    pub(crate) fn replace_navigation_items(
        &mut self,
        input: UpdateNavigationItemsEnvelope,
    ) -> Result<Vec<NavigationItemRecord>, ApiError> {
        if input.items.len() > 20 {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_NAVIGATION_ITEMS",
                "Too many navigation items",
            ));
        }

        let items = input
            .items
            .into_iter()
            .map(|item| {
                let label = item.label.trim().to_string();
                let href = item.href.trim().to_string();
                if label.is_empty() || href.is_empty() {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "INVALID_NAVIGATION_ITEM",
                        "Navigation item is invalid",
                    ));
                }
                Ok(NavigationItemRecord {
                    id: item.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                    label,
                    href,
                    sort_order: item.sort_order.max(0),
                    enabled: item.enabled,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let json = serialize_json_value(&items)?;
        self.conn
            .execute(
                "UPDATE public_site_settings SET navigation_items_json = $1::jsonb, updated_at = NOW() WHERE id = $2",
                &[&json, &"default-public-settings"],
            )
            .map_err(db_error)?;

        Ok(items)
    }

    pub(crate) fn replace_footer_links(
        &mut self,
        input: UpdateFooterLinksEnvelope,
    ) -> Result<Vec<FooterLinkRecord>, ApiError> {
        if input.items.len() > 20 {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_FOOTER_LINKS",
                "Too many footer links",
            ));
        }

        let items = input
            .items
            .into_iter()
            .map(|item| {
                let label = item.label.trim().to_string();
                let href = item.href.trim().to_string();
                let icon_url = normalize_optional_text(item.icon_url);
                let description = item.description.unwrap_or_default().trim().to_string();

                if label.is_empty() || href.is_empty() {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "INVALID_FOOTER_LINK",
                        "Footer link is invalid",
                    ));
                }
                if !validate_url(&href) {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "INVALID_FOOTER_LINK",
                        "Footer link URL is invalid",
                    ));
                }
                if let Some(url) = icon_url.as_ref() {
                    if !validate_url(url) {
                        return Err(ApiError::new(
                            StatusCode::BAD_REQUEST,
                            "INVALID_ICON_URL",
                            "Footer icon URL is invalid",
                        ));
                    }
                }

                Ok(FooterLinkRecord {
                    id: item.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                    label,
                    href,
                    icon_url,
                    description,
                    sort_order: item.sort_order.max(0),
                    enabled: item.enabled,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let json = serialize_json_value(&items)?;
        self.conn
            .execute(
                "UPDATE public_site_settings SET footer_links_json = $1::jsonb, updated_at = NOW() WHERE id = $2",
                &[&json, &"default-public-settings"],
            )
            .map_err(db_error)?;

        Ok(items)
    }

    pub(crate) fn replace_standalone_pages(
        &mut self,
        input: UpdateStandalonePagesEnvelope,
    ) -> Result<Vec<StandalonePageRecord>, ApiError> {
        if input.items.len() > 50 {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_STANDALONE_PAGES",
                "Too many standalone pages",
            ));
        }

        let items = input
            .items
            .into_iter()
            .map(|item| {
                let title = item.title.trim().to_string();
                let slug = item.slug.trim().to_string();
                let summary = item.summary.trim().to_string();
                let content = item.content.trim().to_string();

                if title.is_empty()
                    || summary.is_empty()
                    || content.is_empty()
                    || !is_valid_slug(&slug)
                {
                    return Err(ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "INVALID_STANDALONE_PAGE",
                        "Standalone page is invalid",
                    ));
                }

                Ok(StandalonePageRecord {
                    id: item.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
                    title,
                    slug,
                    summary,
                    content,
                    sort_order: item.sort_order.max(0),
                    enabled: item.enabled,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let json = serialize_json_value(&items)?;
        self.conn
            .execute(
                "UPDATE public_site_settings SET standalone_pages_json = $1::jsonb, updated_at = NOW() WHERE id = $2",
                &[&json, &"default-public-settings"],
            )
            .map_err(db_error)?;

        Ok(items)
    }

    pub(crate) fn update_storage_config(
        &mut self,
        input: UpdateStorageConfigInput,
    ) -> Result<StorageConfigItem, ApiError> {
        let current = read_storage_config_record(self.conn)?;

        let driver = input.driver.trim().to_string();
        if !matches!(
            driver.as_str(),
            "local" | "s3-compatible" | "aliyun-oss" | "tencent-cos"
        ) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_STORAGE_DRIVER",
                "Storage driver is invalid",
            ));
        }

        let endpoint = match input.endpoint {
            Some(value) => normalize_optional_text(value),
            None => current.endpoint.clone(),
        };
        let region = match input.region {
            Some(value) => normalize_optional_text(value),
            None => current.region.clone(),
        };
        let bucket = match input.bucket {
            Some(value) => normalize_optional_text(value),
            None => current.bucket.clone(),
        };
        let access_key_id = input
            .access_key_id
            .map(|value| value.trim().to_string())
            .unwrap_or(current.access_key_id.clone());
        let secret_access_key = input
            .secret_access_key
            .map(|value| value.trim().to_string())
            .unwrap_or(current.secret_access_key.clone());
        let public_base_url = input.public_base_url.trim().to_string();
        let base_folder = input.base_folder.trim().to_string();

        if !validate_url(&public_base_url) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_PUBLIC_BASE_URL",
                "Public base URL is invalid",
            ));
        }

        self.conn
            .execute(
                "UPDATE storage_configs
                 SET enabled = $1, driver = $2, endpoint = $3, region = $4, bucket = $5, access_key_id = $6,
                     secret_access_key = $7, public_base_url = $8, base_folder = $9, force_path_style = $10, updated_at = NOW()
                 WHERE id = $11",
                &[
                    &input.enabled,
                    &driver,
                    &endpoint,
                    &region,
                    &bucket,
                    &access_key_id,
                    &secret_access_key,
                    &public_base_url,
                    &base_folder,
                    &input.force_path_style,
                    &"default-storage-config",
                ],
            )
            .map_err(db_error)?;

        Ok(to_storage_config_item(read_storage_config_record(
            self.conn,
        )?))
    }

    pub(crate) fn update_captcha_config(
        &mut self,
        input: UpdateCaptchaConfigInput,
    ) -> Result<CaptchaAdminConfigItem, ApiError> {
        let current = read_internal_captcha_config(self.conn)?;

        let enabled = input.enabled.unwrap_or(current.enabled);
        let captcha_id = input
            .captcha_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.captcha_id);
        let captcha_key = input
            .captcha_key
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.captcha_key);
        let enabled_on_comment = input
            .enabled_on_comment
            .unwrap_or(current.enabled_on_comment);
        let enabled_on_friend_link = input
            .enabled_on_friend_link
            .unwrap_or(current.enabled_on_friend_link);
        let enabled_on_login = input.enabled_on_login.unwrap_or(current.enabled_on_login);

        self.conn
            .execute(
                "UPDATE captcha_configs
                 SET enabled = $1, captcha_id = $2, captcha_key = $3,
                     enabled_on_comment = $4, enabled_on_friend_link = $5, enabled_on_login = $6, updated_at = NOW()
                 WHERE id = $7",
                &[
                    &enabled,
                    &captcha_id,
                    &captcha_key,
                    &enabled_on_comment,
                    &enabled_on_friend_link,
                    &enabled_on_login,
                    &"default-captcha-config",
                ],
            )
            .map_err(db_error)?;

        let config = read_internal_captcha_config(self.conn)?;
        Ok(CaptchaAdminConfigItem {
            id: "default-captcha-config".to_string(),
            enabled: config.enabled,
            provider: config.provider,
            captcha_id: config.captcha_id,
            captcha_key_configured: !config.captcha_key.trim().is_empty(),
            enabled_on_comment: config.enabled_on_comment,
            enabled_on_friend_link: config.enabled_on_friend_link,
            enabled_on_login: config.enabled_on_login,
        })
    }

    pub(crate) fn review_comment(
        &mut self,
        comment_id: &str,
        input: ReviewCommentInput,
        reviewed_by: &str,
    ) -> Result<AdminCommentItem, ApiError> {
        if !matches!(input.status.as_str(), "pending" | "approved" | "rejected") {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_STATUS",
                "Comment status is invalid",
            ));
        }

        let exists = self
            .conn
            .query_opt("SELECT id FROM comments WHERE id = $1", &[&comment_id])
            .map_err(db_error)?
            .is_some();
        if !exists {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "COMMENT_NOT_FOUND",
                "Comment was not found",
            ));
        }

        let reject_reason = if input.status == "rejected" {
            normalize_optional_text(input.reject_reason)
        } else {
            None
        };

        self.conn
            .execute(
                "UPDATE comments
                 SET status = $1, reviewed_by = $2, reviewed_at = NOW(), reject_reason = $3, updated_at = NOW()
                 WHERE id = $4",
                &[&input.status, &reviewed_by, &reject_reason, &comment_id],
            )
            .map_err(db_error)?;

        load_admin_comment_item(self.conn, comment_id)
    }

    pub(crate) fn delete_comment(&mut self, comment_id: &str) -> Result<(), ApiError> {
        let deleted = self
            .conn
            .execute("DELETE FROM comments WHERE id = $1", &[&comment_id])
            .map_err(db_error)?;

        if deleted == 0 {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "COMMENT_NOT_FOUND",
                "Comment was not found",
            ));
        }

        Ok(())
    }

    pub(crate) fn review_friend_link_application(
        &mut self,
        application_id: &str,
        input: ReviewFriendLinkApplicationInput,
        reviewed_by: &str,
        public_site_url: &str,
    ) -> Result<AdminFriendLinkApplicationItem, ApiError> {
        if !matches!(input.status.as_str(), "pending" | "approved" | "rejected") {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_STATUS",
                "Application status is invalid",
            ));
        }

        let current = load_friend_link_applications(self.conn)?
            .into_iter()
            .find(|item| item.id == application_id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "FRIEND_LINK_APPLICATION_NOT_FOUND",
                    "Friend link application was not found",
                )
            })?;

        let mut settings = read_public_settings_data(self.conn, public_site_url)?;
        let linked_footer_link_id =
            sync_footer_link_for_application(&mut settings, &current, &input.status);
        let footer_links_json = serialize_json_value(&settings.footer_links)?;
        self.conn
            .execute(
                "UPDATE public_site_settings SET footer_links_json = $1::jsonb, updated_at = NOW() WHERE id = $2",
                &[&footer_links_json, &"default-public-settings"],
            )
            .map_err(db_error)?;

        let linked_footer_link_id = if linked_footer_link_id.is_empty() {
            None
        } else {
            Some(linked_footer_link_id)
        };

        self.conn
            .execute(
                "UPDATE friend_link_applications
                 SET status = $1, review_note = $2, reviewed_by = $3, reviewed_at = NOW(), linked_footer_link_id = $4, updated_at = NOW()
                 WHERE id = $5",
                &[
                    &input.status,
                    &normalize_optional_text(input.review_note),
                    &reviewed_by,
                    &linked_footer_link_id,
                    &application_id,
                ],
            )
            .map_err(db_error)?;

        load_friend_link_applications(self.conn)?
            .into_iter()
            .find(|item| item.id == application_id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "FRIEND_LINK_APPLICATION_NOT_FOUND",
                    "Friend link application was not found",
                )
            })
    }

    pub(crate) fn create_category(
        &mut self,
        input: CreateCategoryInput,
    ) -> Result<AdminCategoryItem, ApiError> {
        let name = input.name.trim().to_string();
        let slug = input.slug.trim().to_string();
        let description = input.description.trim().to_string();
        if name.is_empty() || description.is_empty() || !is_valid_slug(&slug) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_CATEGORY",
                "Category is invalid",
            ));
        }
        ensure_unique_category_slug(self.conn, &slug, None)?;

        let id = Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO article_categories (id, name, slug, description, is_enabled, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, NOW(), NOW())",
                &[&id, &name, &slug, &description, &input.is_enabled],
            )
            .map_err(db_error)?;

        let item = load_categories(self.conn)?
            .get(&id)
            .cloned()
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "CATEGORY_NOT_FOUND",
                    "Category was not created",
                )
            })?;
        Ok(build_admin_category_item(&item))
    }

    pub(crate) fn update_category(
        &mut self,
        category_id: &str,
        input: UpdateCategoryInput,
    ) -> Result<AdminCategoryItem, ApiError> {
        let current = load_categories(self.conn)?
            .get(category_id)
            .cloned()
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "CATEGORY_NOT_FOUND",
                    "Category was not found",
                )
            })?;

        let name = input
            .name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.name);
        let slug = input
            .slug
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.slug);
        let description = input
            .description
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.description);
        let is_enabled = input.is_enabled.unwrap_or(current.is_enabled);

        if !is_valid_slug(&slug) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_CATEGORY",
                "Category slug is invalid",
            ));
        }
        ensure_unique_category_slug(self.conn, &slug, Some(category_id))?;

        self.conn
            .execute(
                "UPDATE article_categories SET name = $1, slug = $2, description = $3, is_enabled = $4, updated_at = NOW() WHERE id = $5",
                &[&name, &slug, &description, &is_enabled, &category_id],
            )
            .map_err(db_error)?;

        let updated = load_categories(self.conn)?
            .get(category_id)
            .cloned()
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "CATEGORY_NOT_FOUND",
                    "Category was not found",
                )
            })?;
        Ok(build_admin_category_item(&updated))
    }

    pub(crate) fn delete_category(&mut self, category_id: &str) -> Result<(), ApiError> {
        let linked = self
            .conn
            .query_one(
                "SELECT COUNT(*) FROM article_category_links WHERE category_id = $1",
                &[&category_id],
            )
            .map_err(db_error)?
            .get::<usize, i64>(0);
        if linked > 0 {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "CATEGORY_IN_USE",
                "Category is used by existing articles",
            ));
        }

        let deleted = self
            .conn
            .execute(
                "DELETE FROM article_categories WHERE id = $1",
                &[&category_id],
            )
            .map_err(db_error)?;
        if deleted == 0 {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "CATEGORY_NOT_FOUND",
                "Category was not found",
            ));
        }

        Ok(())
    }

    pub(crate) fn create_tag(&mut self, input: CreateTagInput) -> Result<AdminTagItem, ApiError> {
        let name = input.name.trim().to_string();
        let slug = input.slug.trim().to_string();
        if name.is_empty() || !is_valid_slug(&slug) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_TAG",
                "Tag is invalid",
            ));
        }
        ensure_unique_tag_slug(self.conn, &slug, None)?;

        let id = Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO article_tags (id, name, slug, created_at, updated_at) VALUES ($1, $2, $3, NOW(), NOW())",
                &[&id, &name, &slug],
            )
            .map_err(db_error)?;

        let item = load_tags(self.conn)?.get(&id).cloned().ok_or_else(|| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "TAG_NOT_FOUND",
                "Tag was not created",
            )
        })?;
        Ok(build_admin_tag_item(&item))
    }

    pub(crate) fn update_tag(
        &mut self,
        tag_id: &str,
        input: UpdateTagInput,
    ) -> Result<AdminTagItem, ApiError> {
        let current = load_tags(self.conn)?.get(tag_id).cloned().ok_or_else(|| {
            ApiError::new(StatusCode::NOT_FOUND, "TAG_NOT_FOUND", "Tag was not found")
        })?;

        let name = input
            .name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.name);
        let slug = input
            .slug
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.slug);
        if !is_valid_slug(&slug) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_TAG",
                "Tag slug is invalid",
            ));
        }
        ensure_unique_tag_slug(self.conn, &slug, Some(tag_id))?;

        self.conn
            .execute(
                "UPDATE article_tags SET name = $1, slug = $2, updated_at = NOW() WHERE id = $3",
                &[&name, &slug, &tag_id],
            )
            .map_err(db_error)?;

        let updated = load_tags(self.conn)?.get(tag_id).cloned().ok_or_else(|| {
            ApiError::new(StatusCode::NOT_FOUND, "TAG_NOT_FOUND", "Tag was not found")
        })?;
        Ok(build_admin_tag_item(&updated))
    }

    pub(crate) fn delete_tag(&mut self, tag_id: &str) -> Result<(), ApiError> {
        let linked = self
            .conn
            .query_one(
                "SELECT COUNT(*) FROM article_tag_links WHERE tag_id = $1",
                &[&tag_id],
            )
            .map_err(db_error)?
            .get::<usize, i64>(0);
        if linked > 0 {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "TAG_IN_USE",
                "Tag is used by existing articles",
            ));
        }

        let deleted = self
            .conn
            .execute("DELETE FROM article_tags WHERE id = $1", &[&tag_id])
            .map_err(db_error)?;
        if deleted == 0 {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "TAG_NOT_FOUND",
                "Tag was not found",
            ));
        }

        Ok(())
    }

    pub(crate) fn create_article(
        &mut self,
        input: CreateArticleInput,
        admin_id: &str,
    ) -> Result<ArticleDetailItem, ApiError> {
        let title = input.title.trim().to_string();
        let excerpt = input.excerpt.trim().to_string();
        let content = input.content.trim().to_string();
        let status = input.status.unwrap_or_else(|| "draft".to_string());
        let slug = input
            .slug
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(generate_next_article_slug(self.conn)?);
        let cover_image_url = normalize_optional_text(input.cover_image_url);
        let category_ids =
            resolve_category_ids(self.conn, &input.category_ids.unwrap_or_default())?;
        let tag_ids = resolve_tag_ids(self.conn, &input.tag_ids.unwrap_or_default())?;
        let allow_comment = input.allow_comment.unwrap_or(true);
        let published_at = resolve_publication(&status, input.published_at, None)?;
        let published_at_value = serialize_publication_timestamp(published_at)?;

        if !matches!(status.as_str(), "draft" | "published")
            || title.is_empty()
            || excerpt.is_empty()
            || content.is_empty()
            || !is_valid_slug(&slug)
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_ARTICLE",
                "Article payload is invalid",
            ));
        }
        if let Some(url) = cover_image_url.as_ref() {
            if !validate_url(url) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "INVALID_COVER_IMAGE",
                    "Cover image URL is invalid",
                ));
            }
        }
        ensure_unique_article_slug(self.conn, &slug, None)?;

        let article_id = Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO articles (
                    id, title, slug, excerpt, content, cover_image_url, status, allow_comment, published_at, created_by, updated_by, created_at, updated_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10, NOW(), NOW()
                 )",
                &[
                    &article_id,
                    &title,
                    &slug,
                    &excerpt,
                    &content,
                    &cover_image_url,
                    &status,
                    &allow_comment,
                    &published_at_value,
                    &admin_id,
                ],
            )
            .map_err(db_error)?;

        for category_id in category_ids {
            self.conn
                .execute(
                    "INSERT INTO article_category_links (article_id, category_id, created_at) VALUES ($1, $2, NOW())",
                    &[&article_id, &category_id],
                )
                .map_err(db_error)?;
        }
        for tag_id in tag_ids {
            self.conn
                .execute(
                    "INSERT INTO article_tag_links (article_id, tag_id, created_at) VALUES ($1, $2, NOW())",
                    &[&article_id, &tag_id],
                )
                .map_err(db_error)?;
        }

        let article = load_articles(self.conn)?
            .into_iter()
            .find(|item| item.id == article_id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "ARTICLE_NOT_FOUND",
                    "Article was not created",
                )
            })?;
        build_article_detail(self.conn, article)
    }

    pub(crate) fn update_article(
        &mut self,
        article_id: &str,
        input: UpdateArticleInput,
        admin_id: &str,
    ) -> Result<ArticleDetailItem, ApiError> {
        let current = load_articles(self.conn)?
            .into_iter()
            .find(|item| item.id == article_id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "ARTICLE_NOT_FOUND",
                    "Article was not found",
                )
            })?;

        let title = input
            .title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.title.clone());
        let slug = input
            .slug
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.slug.clone());
        let excerpt = input
            .excerpt
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.excerpt.clone());
        let content = input
            .content
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.content.clone());
        let cover_image_url = match input.cover_image_url {
            Some(value) => normalize_optional_text(value),
            None => current.cover_image_url.clone(),
        };
        let status = input.status.unwrap_or(current.status.clone());
        let allow_comment = input.allow_comment.unwrap_or(current.allow_comment);
        let published_at =
            resolve_publication(&status, input.published_at.flatten(), Some(&current))?;
        let published_at_value = serialize_publication_timestamp(published_at)?;

        if !matches!(status.as_str(), "draft" | "published")
            || title.is_empty()
            || excerpt.is_empty()
            || content.is_empty()
            || !is_valid_slug(&slug)
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_ARTICLE",
                "Article payload is invalid",
            ));
        }
        if let Some(url) = cover_image_url.as_ref() {
            if !validate_url(url) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "INVALID_COVER_IMAGE",
                    "Cover image URL is invalid",
                ));
            }
        }
        ensure_unique_article_slug(self.conn, &slug, Some(article_id))?;

        self.conn
            .execute(
                "UPDATE articles
                 SET title = $1, slug = $2, excerpt = $3, content = $4, cover_image_url = $5,
                     status = $6, allow_comment = $7, published_at = $8, updated_by = $9, updated_at = NOW()
                 WHERE id = $10",
                &[&title, &slug, &excerpt, &content, &cover_image_url, &status, &allow_comment, &published_at_value, &admin_id, &article_id],
            )
            .map_err(db_error)?;

        if let Some(category_ids) = input.category_ids {
            let resolved = resolve_category_ids(self.conn, &category_ids)?;
            self.conn
                .execute(
                    "DELETE FROM article_category_links WHERE article_id = $1",
                    &[&article_id],
                )
                .map_err(db_error)?;
            for category_id in resolved {
                self.conn
                    .execute(
                        "INSERT INTO article_category_links (article_id, category_id, created_at) VALUES ($1, $2, NOW())",
                        &[&article_id, &category_id],
                    )
                    .map_err(db_error)?;
            }
        }
        if let Some(tag_ids) = input.tag_ids {
            let resolved = resolve_tag_ids(self.conn, &tag_ids)?;
            self.conn
                .execute(
                    "DELETE FROM article_tag_links WHERE article_id = $1",
                    &[&article_id],
                )
                .map_err(db_error)?;
            for tag_id in resolved {
                self.conn
                    .execute(
                        "INSERT INTO article_tag_links (article_id, tag_id, created_at) VALUES ($1, $2, NOW())",
                        &[&article_id, &tag_id],
                    )
                    .map_err(db_error)?;
            }
        }

        let article = load_articles(self.conn)?
            .into_iter()
            .find(|item| item.id == article_id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "ARTICLE_NOT_FOUND",
                    "Article was not found",
                )
            })?;
        build_article_detail(self.conn, article)
    }

    pub(crate) fn delete_article(&mut self, article_id: &str) -> Result<(), ApiError> {
        let deleted = self
            .conn
            .execute("DELETE FROM articles WHERE id = $1", &[&article_id])
            .map_err(db_error)?;
        if deleted == 0 {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "ARTICLE_NOT_FOUND",
                "Article was not found",
            ));
        }
        Ok(())
    }

    pub(crate) fn create_banner(
        &mut self,
        input: CreateBannerInput,
        admin_id: &str,
    ) -> Result<BannerItem, ApiError> {
        let title = input.title.trim().to_string();
        let image_url = input.image_url.trim().to_string();
        let link_url = input.link_url.trim().to_string();
        let position = input.position.trim().to_string();
        let link_target = input.link_target.unwrap_or_else(|| "_self".to_string());
        let status = input.status.unwrap_or_else(|| "enabled".to_string());
        let description = normalize_optional_text(input.description);
        let sort_order = input.sort_order.unwrap_or(0);
        let show_text = input.show_text.unwrap_or(true);

        if title.is_empty()
            || !validate_url(&image_url)
            || !validate_url(&link_url)
            || !matches!(
                position.as_str(),
                "home_top" | "home_sidebar" | "article_sidebar" | "footer"
            )
            || !matches!(link_target.as_str(), "_self" | "_blank")
            || !matches!(status.as_str(), "enabled" | "disabled")
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_BANNER",
                "Banner payload is invalid",
            ));
        }

        let id = Uuid::new_v4().to_string();
        self.conn
            .execute(
                "INSERT INTO banners (
                    id, title, description, image_url, link_url, link_target, position, sort_order, status, show_text, created_by, updated_by, created_at, updated_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11, NOW(), NOW()
                 )",
                &[&id, &title, &description, &image_url, &link_url, &link_target, &position, &sort_order, &status, &show_text, &admin_id],
            )
            .map_err(db_error)?;

        load_admin_banners(self.conn)?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "BANNER_NOT_FOUND",
                    "Banner was not created",
                )
            })
    }

    pub(crate) fn update_banner(
        &mut self,
        banner_id: &str,
        input: UpdateBannerInput,
        admin_id: &str,
    ) -> Result<BannerItem, ApiError> {
        let current = load_admin_banners(self.conn)?
            .into_iter()
            .find(|item| item.id == banner_id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "BANNER_NOT_FOUND",
                    "Banner was not found",
                )
            })?;

        let title = input
            .title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.title);
        let description = match input.description {
            Some(value) => normalize_optional_text(value),
            None => current.description,
        };
        let image_url = input
            .image_url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.image_url);
        let link_url = input
            .link_url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.link_url);
        let link_target = input.link_target.unwrap_or(current.link_target);
        let position = input.position.unwrap_or(current.position);
        let sort_order = input.sort_order.unwrap_or(current.sort_order);
        let status = input.status.unwrap_or(current.status);
        let show_text = input.show_text.unwrap_or(current.show_text);

        if title.is_empty()
            || !validate_url(&image_url)
            || !validate_url(&link_url)
            || !matches!(
                position.as_str(),
                "home_top" | "home_sidebar" | "article_sidebar" | "footer"
            )
            || !matches!(link_target.as_str(), "_self" | "_blank")
            || !matches!(status.as_str(), "enabled" | "disabled")
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_BANNER",
                "Banner payload is invalid",
            ));
        }

        self.conn
            .execute(
                "UPDATE banners
                 SET title = $1, description = $2, image_url = $3, link_url = $4, link_target = $5, position = $6,
                     sort_order = $7, status = $8, show_text = $9, updated_by = $10, updated_at = NOW()
                 WHERE id = $11",
                &[&title, &description, &image_url, &link_url, &link_target, &position, &sort_order, &status, &show_text, &admin_id, &banner_id],
            )
            .map_err(db_error)?;

        load_admin_banners(self.conn)?
            .into_iter()
            .find(|item| item.id == banner_id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "BANNER_NOT_FOUND",
                    "Banner was not found",
                )
            })
    }

    pub(crate) fn delete_banner(&mut self, banner_id: &str) -> Result<(), ApiError> {
        let deleted = self
            .conn
            .execute("DELETE FROM banners WHERE id = $1", &[&banner_id])
            .map_err(db_error)?;
        if deleted == 0 {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "BANNER_NOT_FOUND",
                "Banner was not found",
            ));
        }
        Ok(())
    }

    pub(crate) fn reorder_banners(&mut self, ids: &[String]) -> Result<(), ApiError> {
        for (index, id) in ids.iter().enumerate() {
            self.conn
                .execute(
                    "UPDATE banners SET sort_order = $1, updated_at = NOW() WHERE id = $2",
                    &[&(index as i32), id],
                )
                .map_err(db_error)?;
        }
        Ok(())
    }

    pub(crate) fn replace_projects(
        &mut self,
        input: UpdateProjectsInput,
    ) -> Result<Vec<ProjectItem>, ApiError> {
        let mut transaction = self.conn.transaction().map_err(db_error)?;
        transaction
            .execute("DELETE FROM projects", &[])
            .map_err(db_error)?;
        for item in input.items {
            let title = item.title.trim().to_string();
            let description = item.description.trim().to_string();
            let link = item.link.trim().to_string();
            let icon = normalize_optional_text(item.icon);
            let Ok(sort_order) = i32::try_from(item.sort_order) else {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "INVALID_PROJECT",
                    "Project payload is invalid",
                ));
            };
            if title.is_empty()
                || description.is_empty()
                || !validate_url(&link)
                || sort_order < 0
            {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "INVALID_PROJECT",
                    "Project payload is invalid",
                ));
            }
            let id = item.id.unwrap_or_else(|| Uuid::new_v4().to_string());
            transaction
                .execute(
                    "INSERT INTO projects (id, title, description, icon, link, sort_order, enabled, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())",
                    &[&id, &title, &description, &icon, &link, &sort_order, &item.enabled],
                )
                .map_err(db_error)?;
        }
        transaction.commit().map_err(db_error)?;

        load_admin_projects(self.conn)
    }

    pub(crate) fn create_media_asset(
        &mut self,
        input: CreateMediaAssetRecordInput,
    ) -> Result<MediaAssetItem, ApiError> {
        let title = default_media_asset_title(&input.original_filename, &input.filename);
        let alt_text: Option<String> = None;
        let caption: Option<String> = None;
        let description: Option<String> = None;

        self.conn
            .execute(
                "INSERT INTO media_assets (
                    id, provider, bucket, object_key, filename, original_filename, mime_type, extension, size, url, usage,
                    title, alt_text, caption, description, status, uploaded_by, created_at, updated_at, deleted_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                    $12, $13, $14, $15, 'active', $16, NOW(), NOW(), NULL
                 )",
                &[
                    &input.id,
                    &input.provider,
                    &input.bucket,
                    &input.object_key,
                    &input.filename,
                    &input.original_filename,
                    &input.mime_type,
                    &input.extension,
                    &input.size,
                    &input.url,
                    &input.usage,
                    &title,
                    &alt_text,
                    &caption,
                    &description,
                    &input.uploaded_by,
                ],
            )
            .map_err(db_error)?;

        load_media_assets(self.conn)?
            .into_iter()
            .find(|item| item.id == input.id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "MEDIA_NOT_FOUND",
                    "Media was not created",
                )
            })
    }

    pub(crate) fn find_media_storage_locator(
        &mut self,
        media_id: &str,
    ) -> Result<MediaStorageLocator, ApiError> {
        let row = self
            .conn
            .query_opt(
                "SELECT provider, bucket, object_key FROM media_assets WHERE id = $1",
                &[&media_id],
            )
            .map_err(db_error)?
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "MEDIA_NOT_FOUND",
                    "Media asset was not found",
                )
            })?;
        Ok(MediaStorageLocator {
            provider: row.get(0),
            bucket: row.get(1),
            object_key: row.get(2),
        })
    }

    pub(crate) fn mark_media_deleted(&mut self, media_id: &str) -> Result<(), ApiError> {
        self.conn
            .execute(
                "UPDATE media_assets SET status = 'deleted', deleted_at = NOW(), updated_at = NOW() WHERE id = $1",
                &[&media_id],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn update_media_asset_metadata(
        &mut self,
        media_id: &str,
        input: UpdateMediaAssetInput,
    ) -> Result<MediaAssetItem, ApiError> {
        let current = load_media_assets(self.conn)?
            .into_iter()
            .find(|item| item.id == media_id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "MEDIA_NOT_FOUND",
                    "Media asset was not found",
                )
            })?;

        let title = input
            .title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.title.clone());
        let alt_text = match input.alt_text {
            Some(value) => normalize_optional_text(value),
            None => current.alt_text.clone(),
        };
        let caption = match input.caption {
            Some(value) => normalize_optional_text(value),
            None => current.caption.clone(),
        };
        let description = match input.description {
            Some(value) => normalize_optional_text(value),
            None => current.description.clone(),
        };

        self.conn
            .execute(
                "UPDATE media_assets
                 SET title = $1, alt_text = $2, caption = $3, description = $4, updated_at = NOW()
                 WHERE id = $5",
                &[&title, &alt_text, &caption, &description, &media_id],
            )
            .map_err(db_error)?;

        load_media_assets(self.conn)?
            .into_iter()
            .find(|item| item.id == media_id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "MEDIA_NOT_FOUND",
                    "Media asset was not found",
                )
            })
    }

    pub(crate) fn update_smtp_config(
        &mut self,
        input: UpdateSmtpConfigInput,
    ) -> Result<SmtpConfigItem, ApiError> {
        let current = read_smtp_config_record(self.conn)?;

        let host = input.host.trim().to_string();
        let username = input.username.trim().to_string();
        let from_name = input.from_name.trim().to_string();
        let from_email = input.from_email.trim().to_string();
        let password = input
            .password
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.password.clone());

        if host.is_empty()
            || from_name.is_empty()
            || !validate_email(&from_email)
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_SMTP_CONFIG",
                "SMTP configuration is invalid",
            ));
        }

        self.conn
            .execute(
                "UPDATE smtp_configs
                 SET enabled = $1, host = $2, port = $3, secure = $4, username = $5, password = $6, from_name = $7,
                     from_email = $8, updated_at = NOW()
                 WHERE id = $9",
                &[
                    &input.enabled,
                    &host,
                    &input.port,
                    &input.secure,
                    &username,
                    &password,
                    &from_name,
                    &from_email,
                    &"default-smtp-config",
                ],
            )
            .map_err(db_error)?;

        Ok(to_smtp_config_item(read_smtp_config_record(self.conn)?))
    }

    pub(crate) fn update_smtp_test_status(
        &mut self,
        last_test_status: &str,
        last_error_message: Option<&str>,
    ) -> Result<(), ApiError> {
        if last_test_status != "success" && last_test_status != "failed" {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_SMTP_TEST_STATUS",
                "SMTP test status is invalid",
            ));
        }

        self.conn
            .execute(
                "UPDATE smtp_configs
                 SET last_test_at = NOW(), last_test_status = $1, last_error_message = $2, updated_at = NOW()
                 WHERE id = $3",
                &[&last_test_status, &last_error_message, &"default-smtp-config"],
            )
            .map_err(db_error)?;

        Ok(())
    }
}
