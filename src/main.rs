use std::{collections::HashMap, env, net::SocketAddr, path::PathBuf, sync::Arc};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as Base64Standard, Engine as _};
use chrono::{Datelike, Duration, NaiveDate, Utc};
use dotenvy::dotenv;
use hmac::{Hmac, KeyInit, Mac};
use lettre::{
    message::{header::ContentType, Mailbox, MultiPart, SinglePart},
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
    Message, SmtpTransport, Transport,
};
use postgres::{Client as PgClient, NoTls};
use reqwest::blocking::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::Sha256;
use tower_http::{
    cors::{AllowOrigin, Any, CorsLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use uuid::Uuid;

mod db;

use db::{CreateMediaAssetRecordInput, Database};

#[derive(Clone)]
struct AppState {
    config: Arc<Config>,
}

#[derive(Clone)]
struct Config {
    bind: String,
    database_url: String,
    uploads_dir: String,
    cors_origin: String,
    public_site_url: String,
    frontend_dir: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct ApiEnvelope<T: Serialize> {
    code: &'static str,
    message: &'static str,
    data: T,
    timestamp: String,
}

impl ApiEnvelope<()> {
    fn error(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            data: (),
            timestamp: Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct NavigationItemRecord {
    id: String,
    label: String,
    href: String,
    sort_order: i64,
    enabled: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FooterLinkRecord {
    id: String,
    label: String,
    href: String,
    icon_url: Option<String>,
    description: String,
    sort_order: i64,
    enabled: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AboutContactRecord {
    id: String,
    name: String,
    #[serde(default)]
    display_text: String,
    url: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StandalonePageRecord {
    id: String,
    title: String,
    slug: String,
    summary: String,
    content: String,
    sort_order: i64,
    enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SeoMeta {
    title: String,
    description: String,
    keywords: String,
    canonical_url: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct NavigationItemItem {
    id: String,
    label: String,
    href: String,
    sort_order: i64,
    enabled: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FooterLinkItem {
    id: String,
    label: String,
    href: String,
    icon_url: Option<String>,
    description: String,
    sort_order: i64,
    enabled: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicSiteSettingsItem {
    site_title: String,
    site_description: String,
    logo_url: Option<String>,
    comment_enabled: bool,
    seo: SeoMeta,
    navigation_items: Vec<NavigationItemItem>,
    footer_links: Vec<FooterLinkItem>,
    custom_head_code: Option<String>,
    custom_footer_code: Option<String>,
    icp_filing: Option<String>,
    police_filing: Option<String>,
    show_filing: bool,
    github_username: Option<String>,
    about_display_name: Option<String>,
    about_bio: Option<String>,
    about_contacts: Vec<AboutContactRecord>,
    admin_avatar_url: Option<String>,
    article_layout: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicSyncVersionItem {
    version: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StandalonePageSummaryItem {
    id: String,
    title: String,
    slug: String,
    summary: String,
    sort_order: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StandalonePageDetailItem {
    id: String,
    title: String,
    slug: String,
    summary: String,
    sort_order: i64,
    content: String,
}

#[derive(Clone)]
struct ArticleCategoryRecord {
    id: String,
    name: String,
    slug: String,
    description: String,
    is_enabled: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Clone)]
struct ArticleTagRecord {
    id: String,
    name: String,
    slug: String,
    created_at: String,
    updated_at: String,
}

#[derive(Clone)]
struct ArticleRow {
    id: String,
    title: String,
    slug: String,
    excerpt: String,
    content: String,
    cover_image_url: Option<String>,
    status: String,
    allow_comment: bool,
    published_at: Option<String>,
    created_by: String,
    updated_by: String,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ArticleTaxonomyItem {
    id: String,
    name: String,
    slug: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ArticleSummaryItem {
    id: String,
    title: String,
    slug: String,
    excerpt: String,
    cover_image_url: Option<String>,
    status: String,
    allow_comment: bool,
    published_at: Option<String>,
    created_at: String,
    updated_at: String,
    categories: Vec<ArticleTaxonomyItem>,
    tags: Vec<ArticleTaxonomyItem>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ArticleDetailItem {
    #[serde(flatten)]
    summary: ArticleSummaryItem,
    content: String,
    created_by: String,
    updated_by: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PaginatedResponse<T: Serialize> {
    list: Vec<T>,
    total: usize,
    page: usize,
    page_size: usize,
}

#[derive(Clone)]
struct CommentRecord {
    id: String,
    article_id: String,
    parent_id: Option<String>,
    nickname: String,
    email: String,
    content: String,
    status: String,
    created_at: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PublicCommentItem {
    id: String,
    parent_id: Option<String>,
    nickname: String,
    avatar_url: String,
    content: String,
    created_at: String,
    replies: Vec<PublicCommentItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BannerItem {
    id: String,
    title: String,
    description: Option<String>,
    image_url: String,
    link_url: String,
    link_target: String,
    position: String,
    sort_order: i32,
    status: String,
    show_text: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectItem {
    id: String,
    title: String,
    description: String,
    icon: Option<String>,
    link: String,
    sort_order: i64,
    enabled: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptchaPublicConfig {
    enabled: bool,
    provider: String,
    captcha_id: String,
    enabled_on_comment: bool,
    enabled_on_friend_link: bool,
    enabled_on_login: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ContributionDay {
    date: String,
    contribution_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ContributionWeek {
    contribution_days: Vec<ContributionDay>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ContributionData {
    weeks: Vec<ContributionWeek>,
    total_contributions: i64,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ArticleListQuery {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_page_size")]
    page_size: usize,
    keyword: Option<String>,
    category_slug: Option<String>,
    tag_slug: Option<String>,
    #[serde(default = "default_sort_by")]
    sort_by: String,
    #[serde(default = "default_sort_order")]
    sort_order: String,
}

#[derive(Deserialize, Default)]
struct BannerListQuery {
    position: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CommentListQuery {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_page_size")]
    page_size: usize,
}

#[derive(Deserialize, Clone)]
struct CaptchaInput {
    lot_number: String,
    captcha_output: String,
    pass_token: String,
    gen_time: String,
}

#[derive(Deserialize)]
struct GeeTestValidationResponse {
    result: String,
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateCommentInput {
    nickname: String,
    email: String,
    website: Option<String>,
    content: String,
    parent_id: Option<String>,
    captcha: Option<CaptchaInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateFriendLinkApplicationInput {
    site_name: String,
    site_url: String,
    icon_url: Option<String>,
    description: String,
    contact_name: String,
    contact_email: String,
    message: Option<String>,
    captcha: Option<CaptchaInput>,
}

#[derive(Serialize)]
struct MutationResult {
    id: String,
    status: String,
}

#[derive(Clone)]
struct PendingCommentNotification {
    article_title: String,
    article_slug: String,
    nickname: String,
    email: String,
    website: Option<String>,
    content: String,
    parent_id: Option<String>,
    status: String,
}

#[derive(Clone)]
struct PendingFriendLinkNotification {
    site_name: String,
    site_url: String,
    icon_url: Option<String>,
    description: String,
    contact_name: String,
    contact_email: String,
    message: Option<String>,
    status: String,
}

struct InternalCaptchaConfig {
    enabled: bool,
    provider: String,
    captcha_id: String,
    captcha_key: String,
    enabled_on_comment: bool,
    enabled_on_friend_link: bool,
    enabled_on_login: bool,
}

#[derive(Clone)]
struct AdminRecord {
    id: String,
    username: String,
    password_hash: String,
    display_name: String,
    email: String,
    avatar_url: Option<String>,
    status: String,
    last_login_at: Option<String>,
}

#[derive(Clone)]
struct AdminSessionRecord {
    id: String,
    admin_id: String,
    refresh_token_hash: String,
    status: String,
    expires_at: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AdminProfileItem {
    id: String,
    username: String,
    display_name: String,
    email: String,
    avatar_url: Option<String>,
    status: String,
    last_login_at: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminAuthResult {
    access_token: String,
    refresh_token: String,
    access_token_expires_at: String,
    refresh_token_expires_at: String,
    admin: AdminProfileItem,
}

#[derive(Clone)]
struct AdminAuthContext {
    admin: AdminRecord,
    session_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminLoginInput {
    username: String,
    password: String,
    captcha: Option<CaptchaInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminRefreshInput {
    refresh_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAdminProfileInput {
    username: Option<String>,
    email: Option<String>,
    display_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChangeAdminPasswordInput {
    current_password: String,
    new_password: String,
}

#[derive(Clone)]
struct StorageConfigRecord {
    id: String,
    enabled: bool,
    driver: String,
    endpoint: Option<String>,
    region: Option<String>,
    bucket: Option<String>,
    access_key_id: String,
    secret_access_key: String,
    public_base_url: String,
    base_folder: String,
    force_path_style: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StorageConfigItem {
    id: String,
    enabled: bool,
    driver: String,
    endpoint: Option<String>,
    region: Option<String>,
    bucket: Option<String>,
    access_key_id: String,
    secret_access_key_configured: bool,
    public_base_url: String,
    base_folder: String,
    force_path_style: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStorageConfigInput {
    enabled: bool,
    driver: String,
    endpoint: Option<Option<String>>,
    region: Option<Option<String>>,
    bucket: Option<Option<String>>,
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    public_base_url: String,
    base_folder: String,
    force_path_style: bool,
}

#[derive(Clone)]
struct SmtpConfigRecord {
    id: String,
    enabled: bool,
    host: String,
    port: i32,
    secure: bool,
    username: String,
    password: String,
    from_name: String,
    from_email: String,
    created_at: String,
    updated_at: String,
    last_test_at: Option<String>,
    last_test_status: String,
    last_error_message: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SmtpConfigItem {
    id: String,
    enabled: bool,
    host: String,
    port: i32,
    secure: bool,
    username: String,
    password_configured: bool,
    from_name: String,
    from_email: String,
    created_at: String,
    updated_at: String,
    last_test_at: Option<String>,
    last_test_status: String,
    last_error_message: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateSmtpConfigInput {
    enabled: bool,
    host: String,
    port: i32,
    secure: bool,
    username: String,
    password: Option<String>,
    from_name: String,
    from_email: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendTestEmailInput {
    to_email: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminSiteSettingsItem {
    public_config: PublicSiteSettingsItem,
    smtp_config: SmtpConfigItem,
    storage_config: StorageConfigItem,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptchaAdminConfigItem {
    id: String,
    enabled: bool,
    provider: String,
    captcha_id: String,
    captcha_key_configured: bool,
    enabled_on_comment: bool,
    enabled_on_friend_link: bool,
    enabled_on_login: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdatePublicSiteSettingsInput {
    site_title: Option<String>,
    site_description: Option<String>,
    logo_url: Option<Option<String>>,
    comment_enabled: Option<bool>,
    seo_keywords: Option<String>,
    custom_head_code: Option<String>,
    custom_footer_code: Option<String>,
    icp_filing: Option<Option<String>>,
    police_filing: Option<Option<String>>,
    show_filing: Option<bool>,
    github_username: Option<Option<String>>,
    about_display_name: Option<Option<String>>,
    about_bio: Option<Option<String>>,
    about_contacts: Option<Vec<UpdateAboutContactInput>>,
    article_layout: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAboutContactInput {
    id: Option<String>,
    name: String,
    display_text: Option<String>,
    url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateNavigationItemsEnvelope {
    items: Vec<UpdateNavigationItemInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateNavigationItemInput {
    id: Option<String>,
    label: String,
    href: String,
    sort_order: i64,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateFooterLinksEnvelope {
    items: Vec<UpdateFooterLinkInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateFooterLinkInput {
    id: Option<String>,
    label: String,
    href: String,
    icon_url: Option<String>,
    description: Option<String>,
    sort_order: i64,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStandalonePagesEnvelope {
    items: Vec<UpdateStandalonePageInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStandalonePageInput {
    id: Option<String>,
    title: String,
    slug: String,
    summary: String,
    content: String,
    sort_order: i64,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCaptchaConfigInput {
    enabled: Option<bool>,
    captcha_id: Option<String>,
    captcha_key: Option<String>,
    enabled_on_comment: Option<bool>,
    enabled_on_friend_link: Option<bool>,
    enabled_on_login: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminCommentCaptchaDebugInput {
    slug: String,
    captcha: Option<CaptchaInput>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptchaDebugPayloadItem {
    lot_number_preview: String,
    captcha_output_preview: String,
    pass_token_preview: String,
    gen_time: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommentCaptchaDebugItem {
    scene: String,
    dry_run: bool,
    provider: String,
    article_slug: String,
    article_title: Option<String>,
    article_found: bool,
    article_published: bool,
    article_allow_comment: bool,
    site_comment_enabled: bool,
    captcha_enabled: bool,
    captcha_id_configured: bool,
    captcha_key_configured: bool,
    enabled_on_comment: bool,
    captcha_required: bool,
    validation_attempted: bool,
    validation_passed: bool,
    comment_would_be_accepted: bool,
    code: String,
    message: String,
    payload: Option<CaptchaDebugPayloadItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminCategoryItem {
    id: String,
    name: String,
    slug: String,
    description: String,
    is_enabled: bool,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminTagItem {
    id: String,
    name: String,
    slug: String,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ArticleEditorOptions {
    categories: Vec<ArticleTaxonomyItem>,
    tags: Vec<ArticleTaxonomyItem>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AdminArticleListQuery {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_page_size")]
    page_size: usize,
    keyword: Option<String>,
    status: Option<String>,
    category_id: Option<String>,
    tag_id: Option<String>,
    allow_comment: Option<String>,
    #[serde(default = "default_admin_article_sort_by")]
    sort_by: String,
    #[serde(default = "default_sort_order")]
    sort_order: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateArticleInput {
    title: String,
    slug: Option<String>,
    excerpt: String,
    content: String,
    cover_image_url: Option<String>,
    category_ids: Option<Vec<String>>,
    tag_ids: Option<Vec<String>>,
    status: Option<String>,
    allow_comment: Option<bool>,
    published_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateArticleInput {
    title: Option<String>,
    slug: Option<String>,
    excerpt: Option<String>,
    content: Option<String>,
    cover_image_url: Option<Option<String>>,
    category_ids: Option<Vec<String>>,
    tag_ids: Option<Vec<String>>,
    status: Option<String>,
    allow_comment: Option<bool>,
    published_at: Option<Option<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateCategoryInput {
    name: String,
    slug: String,
    description: String,
    is_enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCategoryInput {
    name: Option<String>,
    slug: Option<String>,
    description: Option<String>,
    is_enabled: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTagInput {
    name: String,
    slug: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateTagInput {
    name: Option<String>,
    slug: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AdminCommentArticleRef {
    id: String,
    title: String,
    slug: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AdminCommentParentRef {
    id: String,
    nickname: String,
    status: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AdminCommentItem {
    id: String,
    article: AdminCommentArticleRef,
    parent: Option<AdminCommentParentRef>,
    nickname: String,
    email: String,
    website: Option<String>,
    content: String,
    status: String,
    reviewed_by: Option<String>,
    reviewed_at: Option<String>,
    reject_reason: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AdminCommentListQuery {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_page_size")]
    page_size: usize,
    status: Option<String>,
    article_id: Option<String>,
    keyword: Option<String>,
    #[serde(default = "default_admin_comment_sort_by")]
    sort_by: String,
    #[serde(default = "default_sort_order")]
    sort_order: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewCommentInput {
    status: String,
    reject_reason: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AdminFriendLinkApplicationItem {
    id: String,
    site_name: String,
    site_url: String,
    icon_url: Option<String>,
    description: String,
    contact_name: String,
    contact_email: String,
    message: Option<String>,
    status: String,
    review_note: Option<String>,
    reviewed_by: Option<String>,
    reviewed_at: Option<String>,
    linked_footer_link_id: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReviewFriendLinkApplicationInput {
    status: String,
    review_note: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AdminBannerListQuery {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_page_size")]
    page_size: usize,
    keyword: Option<String>,
    position: Option<String>,
    status: Option<String>,
    #[serde(default = "default_admin_banner_sort_by")]
    sort_by: String,
    #[serde(default = "default_admin_banner_sort_order")]
    sort_order: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateBannerInput {
    title: String,
    description: Option<String>,
    image_url: String,
    link_url: String,
    link_target: Option<String>,
    position: String,
    sort_order: Option<i32>,
    status: Option<String>,
    show_text: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateBannerInput {
    title: Option<String>,
    description: Option<Option<String>>,
    image_url: Option<String>,
    link_url: Option<String>,
    link_target: Option<String>,
    position: Option<String>,
    sort_order: Option<i32>,
    status: Option<String>,
    show_text: Option<bool>,
}

#[derive(Deserialize)]
struct ReorderBannersInput {
    ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaAssetItem {
    id: String,
    provider: String,
    filename: String,
    original_filename: String,
    mime_type: String,
    size: i64,
    url: String,
    usage: String,
    status: String,
    title: String,
    alt_text: Option<String>,
    caption: Option<String>,
    description: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AdminMediaListQuery {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_page_size")]
    page_size: usize,
    keyword: Option<String>,
    mime_type: Option<String>,
    usage: Option<String>,
    status: Option<String>,
    #[serde(default = "default_admin_media_sort_by")]
    sort_by: String,
    #[serde(default = "default_sort_order")]
    sort_order: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMediaAssetInput {
    title: Option<String>,
    alt_text: Option<Option<String>>,
    caption: Option<Option<String>>,
    description: Option<Option<String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProjectsInput {
    items: Vec<ProjectUpsertInput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectUpsertInput {
    id: Option<String>,
    title: String,
    description: String,
    icon: Option<String>,
    link: String,
    sort_order: i64,
    enabled: bool,
}

#[tokio::main]
async fn main() {
    load_environment();

    let config = Arc::new(Config {
        bind: env::var("RUST_API_BIND").unwrap_or_else(|_| "0.0.0.0:4000".to_string()),
        database_url: env::var("RUST_API_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@127.0.0.1:5432/aksrtblog".to_string()),
        uploads_dir: env::var("RUST_API_UPLOADS_DIR")
            .unwrap_or_else(|_| "storage/uploads".to_string()),
        cors_origin: env::var("RUST_API_CORS_ORIGIN").unwrap_or_else(|_| "*".to_string()),
        public_site_url: env::var("RUST_API_PUBLIC_SITE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string()),
        frontend_dir: env::var("RUST_API_FRONTEND_DIR")
            .unwrap_or_else(|_| "frontend/.output/public".to_string()),
    });

    let state = AppState {
        config: config.clone(),
    };

    // Run synchronous database initialization in spawn_blocking to avoid runtime conflict
    let config_clone = config.clone();
    let database_init = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut conn = PgClient::connect(&config_clone.database_url, NoTls)
            .map_err(|error| format_database_startup_error(&config_clone.database_url, &error))?;
        ensure_default_records(&mut conn, &config_clone).map_err(|error| {
            format!(
                "Failed to initialize default records: [{}] {}",
                error.code, error.message
            )
        })?;
        Ok(())
    })
    .await;

    match database_init {
        Ok(Ok(())) => {}
        Ok(Err(message)) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
        Err(error) => {
            eprintln!("Database initialization task failed: {error}");
            std::process::exit(1);
        }
    }

    let cors = if state.config.cors_origin.trim() == "*" {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins = state
            .config
            .cors_origin
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(|origin| HeaderValue::from_str(origin).expect("invalid cors origin"))
            .collect::<Vec<_>>();

        assert!(!origins.is_empty(), "RUST_API_CORS_ORIGIN cannot be empty");

        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods(Any)
            .allow_headers(Any)
    };

    let app = Router::new()
        .route("/api/v1/health", get(health))
        .route(
            "/api/v1/public/site-settings",
            get(get_public_site_settings),
        )
        .route(
            "/api/v1/public/site-settings/navigation",
            get(get_public_navigation),
        )
        .route(
            "/api/v1/public/site-settings/footer-links",
            get(get_public_footer_links),
        )
        .route(
            "/api/v1/public/site-settings/standalone-pages",
            get(get_public_standalone_pages),
        )
        .route(
            "/api/v1/public/site-settings/standalone-pages/:slug",
            get(get_public_standalone_page),
        )
        .route(
            "/api/v1/public/site-settings/captcha",
            get(get_public_captcha_config),
        )
        .route("/api/v1/public/sync-version", get(get_public_sync_version))
        .route("/api/v1/public/articles", get(list_public_articles))
        .route(
            "/api/v1/public/articles/meta/categories",
            get(list_public_categories),
        )
        .route(
            "/api/v1/public/articles/:slug/comments",
            get(list_public_comments).post(submit_public_comment),
        )
        .route("/api/v1/public/articles/:slug", get(get_public_article))
        .route("/api/v1/public/banners", get(list_public_banners))
        .route("/api/v1/public/projects", get(list_public_projects))
        .route(
            "/api/v1/public/activity-stats/contributions",
            get(get_activity_stats),
        )
        .route(
            "/api/v1/public/friend-link-applications",
            post(submit_public_friend_link_application),
        )
        .route("/api/v1/admin/auth/login", post(admin_login))
        .route("/api/v1/admin/auth/refresh", post(admin_refresh))
        .route(
            "/api/v1/admin/auth/me",
            get(admin_get_me).patch(admin_update_me),
        )
        .route(
            "/api/v1/admin/auth/change-password",
            post(admin_change_password),
        )
        .route("/api/v1/admin/site-settings", get(admin_get_site_settings))
        .route(
            "/api/v1/admin/site-settings/public",
            get(admin_get_public_settings).put(admin_update_public_settings),
        )
        .route(
            "/api/v1/admin/site-settings/navigation",
            get(admin_get_navigation_items).put(admin_update_navigation_items),
        )
        .route(
            "/api/v1/admin/site-settings/footer-links",
            get(admin_get_footer_links).put(admin_update_footer_links),
        )
        .route(
            "/api/v1/admin/site-settings/standalone-pages",
            get(admin_get_standalone_pages).put(admin_update_standalone_pages),
        )
        .route(
            "/api/v1/admin/site-settings/storage",
            get(admin_get_storage_config).put(admin_update_storage_config),
        )
        .route(
            "/api/v1/admin/site-settings/captcha",
            get(admin_get_captcha_config).put(admin_update_captcha_config),
        )
        .route(
            "/api/v1/admin/site-settings/captcha/debug/comment",
            post(admin_debug_comment_captcha),
        )
        .route("/api/v1/admin/comments", get(admin_list_comments))
        .route(
            "/api/v1/admin/comments/:id/review",
            axum::routing::patch(admin_review_comment),
        )
        .route(
            "/api/v1/admin/comments/:id",
            axum::routing::delete(admin_delete_comment),
        )
        .route(
            "/api/v1/admin/friend-link-applications",
            get(admin_list_friend_link_applications),
        )
        .route(
            "/api/v1/admin/friend-link-applications/:id/review",
            axum::routing::patch(admin_review_friend_link_application),
        )
        .route(
            "/api/v1/admin/articles/meta/options",
            get(admin_get_article_editor_options),
        )
        .route(
            "/api/v1/admin/articles/meta/categories",
            get(admin_list_categories).post(admin_create_category),
        )
        .route(
            "/api/v1/admin/articles/meta/categories/:id",
            axum::routing::patch(admin_update_category).delete(admin_delete_category),
        )
        .route(
            "/api/v1/admin/articles/meta/tags",
            get(admin_list_tags).post(admin_create_tag),
        )
        .route(
            "/api/v1/admin/articles/meta/tags/:id",
            axum::routing::patch(admin_update_tag).delete(admin_delete_tag),
        )
        .route(
            "/api/v1/admin/articles",
            get(admin_list_articles).post(admin_create_article),
        )
        .route(
            "/api/v1/admin/articles/:id",
            get(admin_get_article)
                .patch(admin_update_article)
                .delete(admin_delete_article),
        )
        .route(
            "/api/v1/admin/banners",
            get(admin_list_banners).post(admin_create_banner),
        )
        .route(
            "/api/v1/admin/banners/reorder",
            axum::routing::put(admin_reorder_banners),
        )
        .route(
            "/api/v1/admin/banners/:id",
            get(admin_get_banner)
                .patch(admin_update_banner)
                .delete(admin_delete_banner),
        )
        .route(
            "/api/v1/admin/projects",
            get(admin_list_projects).put(admin_update_projects),
        )
        .route("/api/v1/admin/media", get(admin_list_media))
        .route(
            "/api/v1/admin/media/upload",
            post(admin_upload_media).layer(DefaultBodyLimit::max(12 * 1024 * 1024)),
        )
        .route(
            "/api/v1/admin/media/:id",
            get(admin_get_media)
                .patch(admin_update_media)
                .delete(admin_delete_media),
        )
        .route(
            "/api/v1/admin/smtp/config",
            get(admin_get_smtp_config).put(admin_update_smtp_config),
        )
        .route("/api/v1/admin/smtp/test", post(admin_send_test_email))
        .route("/robots.txt", get(get_robots))
        .route("/sitemap.xml", get(get_sitemap))
        .nest_service("/uploads", ServeDir::new(state.config.uploads_dir.clone()));

    // Only serve frontend static files if FRONTEND_DIR is set
    let app = if state.config.frontend_dir.is_empty() {
        println!("Frontend static files disabled (FRONTEND_DIR is empty)");
        app.fallback(|| async { (
            StatusCode::NOT_FOUND,
            axum::Json(ApiEnvelope::<()>::error("NOT_FOUND", "Frontend not served by API")),
        )})
    } else {
        println!("Serving frontend from: {}", state.config.frontend_dir);
        app.nest_service("/", ServeDir::new(state.config.frontend_dir.clone()).fallback(
            ServeFile::new(format!("{}/index.html", state.config.frontend_dir)),
        ))
    };

    let app = app
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    let addr: SocketAddr = state.config.bind.parse().expect("invalid bind address");
    println!("Rust API listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind listener");

    axum::serve(listener, app).await.expect("server error");
}

async fn health() -> impl IntoResponse {
    ok(json!({ "status": "up" }))
}

async fn run_blocking<T, F>(task: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ApiError> + Send + 'static,
{
    tokio::task::spawn_blocking(task).await.map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "BLOCKING_TASK_FAILED",
            format!("Blocking task failed: {error}"),
        )
    })?
}

fn ok<T: Serialize>(data: T) -> Json<ApiEnvelope<T>> {
    Json(ApiEnvelope {
        code: "OK",
        message: "OK",
        data,
        timestamp: Utc::now().to_rfc3339(),
    })
}

fn created<T: Serialize>(data: T) -> (StatusCode, Json<ApiEnvelope<T>>) {
    (StatusCode::CREATED, ok(data))
}

fn default_page() -> usize {
    1
}

fn default_page_size() -> usize {
    20
}

fn default_sort_by() -> String {
    "publishedAt".to_string()
}

fn default_sort_order() -> String {
    "desc".to_string()
}

fn default_admin_article_sort_by() -> String {
    "updatedAt".to_string()
}

fn default_admin_comment_sort_by() -> String {
    "createdAt".to_string()
}

fn default_admin_banner_sort_by() -> String {
    "sortOrder".to_string()
}

fn default_admin_banner_sort_order() -> String {
    "asc".to_string()
}

fn default_admin_media_sort_by() -> String {
    "createdAt".to_string()
}

fn load_environment() {
    dotenv().ok();

    let backend_env_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env");
    if backend_env_path.is_file() {
        let _ = dotenvy::from_path(&backend_env_path);
    }
}

fn format_database_startup_error(database_url: &str, error: &postgres::Error) -> String {
    let mut message = format!("Failed to connect PostgreSQL using RUST_API_DATABASE_URL: {error}");

    if has_unescaped_hash_in_database_password(database_url) {
        message.push_str(
            " Hint: the password portion of the URL contains `#`. PostgreSQL URLs require reserved characters in passwords to be percent-encoded, so replace `#` with `%23`.",
        );
    }

    message
}

fn has_unescaped_hash_in_database_password(database_url: &str) -> bool {
    let Some(scheme_index) = database_url.find("://") else {
        return false;
    };

    let authority_start = scheme_index + 3;
    let authority_end = database_url[authority_start..]
        .find(&['/', '?'][..])
        .map(|offset| authority_start + offset)
        .unwrap_or(database_url.len());
    let authority = &database_url[authority_start..authority_end];

    let Some(at_index) = authority.rfind('@') else {
        return false;
    };
    let credentials = &authority[..at_index];

    let Some(colon_index) = credentials.find(':') else {
        return false;
    };
    let password = &credentials[colon_index + 1..];

    password.contains('#')
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value.and_then(|item| {
        let trimmed = item.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn htmlize_multiline_text(value: &str) -> String {
    escape_html(value).replace('\n', "<br />")
}

fn site_url(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

fn default_media_asset_title(original_filename: &str, filename: &str) -> String {
    let source = if original_filename.trim().is_empty() {
        filename
    } else {
        original_filename
    };
    let stem = source
        .rsplit_once('.')
        .map(|(value, _)| value)
        .unwrap_or(source)
        .replace(['_', '-'], " ");
    let normalized = stem.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        "Untitled media".to_string()
    } else {
        normalized
    }
}

fn require_length(
    value: &str,
    min: usize,
    max: usize,
    code: &'static str,
    message: &'static str,
) -> Result<(), ApiError> {
    let length = value.chars().count();
    if length < min || length > max {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, code, message));
    }
    Ok(())
}

fn parse_duration_seconds(raw: &str, fallback: i64) -> i64 {
    let trimmed = raw.trim();
    let split_at = trimmed
        .find(|char: char| !char.is_ascii_digit())
        .unwrap_or(trimmed.len());
    let (value_part, unit_part) = trimmed.split_at(split_at);
    let value = value_part.parse::<i64>().unwrap_or(0);

    if value <= 0 {
        return fallback;
    }

    match unit_part {
        "s" => value,
        "m" => value * 60,
        "h" => value * 60 * 60,
        "d" => value * 60 * 60 * 24,
        _ => fallback,
    }
}

fn sha256_hex(value: &str) -> String {
    hex_lower(&Sha256::digest(value.as_bytes()))
}

fn hash_password(value: &str) -> String {
    format!("sha256:{}", sha256_hex(value))
}

fn verify_password(plain_text: &str, stored_hash: &str) -> bool {
    if let Some(expected) = stored_hash.strip_prefix("sha256:") {
        return sha256_hex(plain_text) == expected;
    }

    stored_hash == plain_text
}

fn access_secret() -> String {
    env::var("JWT_ACCESS_SECRET")
        .or_else(|_| env::var("RUST_ADMIN_ACCESS_SECRET"))
        .unwrap_or_else(|_| "aksrtblog-rust-access-secret-change-me-1234567890".to_string())
}

fn refresh_ttl_seconds() -> i64 {
    parse_duration_seconds(
        &env::var("JWT_REFRESH_TTL").unwrap_or_else(|_| "7d".to_string()),
        7 * 24 * 60 * 60,
    )
}

fn access_ttl_seconds() -> i64 {
    parse_duration_seconds(
        &env::var("JWT_ACCESS_TTL").unwrap_or_else(|_| "15m".to_string()),
        15 * 60,
    )
}

fn build_access_token(admin_id: &str, session_id: &str, expires_at: i64) -> String {
    let signature_input = format!("access|{}|{}|{}", session_id, admin_id, expires_at);
    let mut mac =
        Hmac::<Sha256>::new_from_slice(access_secret().as_bytes()).expect("invalid access secret");
    mac.update(signature_input.as_bytes());
    let signature = hex_lower(&mac.finalize().into_bytes());
    format!(
        "aksrt.access.{}.{}.{}.{}",
        session_id, admin_id, expires_at, signature
    )
}

fn issue_refresh_token() -> String {
    format!("aksrt.refresh.{}.{}", Uuid::new_v4(), Uuid::new_v4())
}

fn is_valid_slug(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 160
        && trimmed
            .chars()
            .all(|char| char.is_ascii_lowercase() || char.is_ascii_digit() || char == '-')
        && !trimmed.starts_with('-')
        && !trimmed.ends_with('-')
        && !trimmed.contains("--")
}

fn validate_email(value: &str) -> bool {
    let trimmed = value.trim();
    let mut parts = trimmed.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    !local.is_empty()
        && !domain.is_empty()
        && parts.next().is_none()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}

fn validate_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn validate_contact_url(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("file:")
    {
        return false;
    }

    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
    {
        return true;
    }

    // Allow custom schemes such as tencent://, weixin://, tg://, etc.
    match lower.split_once(':') {
        Some((scheme, _)) => {
            let mut chars = scheme.chars();
            match chars.next() {
                Some(first) if first.is_ascii_alphabetic() => {}
                _ => return false,
            }

            chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '.' || c == '-')
        }
        None => false,
    }
}

fn extract_client_meta(headers: &HeaderMap) -> (Option<String>, Option<String>) {
    let ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(',').next().unwrap_or(value).trim().to_string())
        .filter(|value| !value.is_empty());

    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    (ip, user_agent)
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push_str(&format!("{:02x}", byte));
    }
    output
}

fn sha256_bytes_hex(value: &[u8]) -> String {
    hex_lower(&Sha256::digest(value))
}

fn sha1_bytes_hex(value: &[u8]) -> String {
    hex_lower(&Sha1::digest(value))
}

fn hmac_sha256_bytes(key: &[u8], value: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("invalid hmac sha256 key");
    mac.update(value.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn hmac_sha256_hex(key: &[u8], value: &str) -> String {
    hex_lower(&hmac_sha256_bytes(key, value))
}

fn hmac_sha1_hex(key: &[u8], value: &str) -> String {
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("invalid hmac sha1 key");
    mac.update(value.as_bytes());
    hex_lower(&mac.finalize().into_bytes())
}

fn hmac_sha1_base64(key: &[u8], value: &str) -> String {
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("invalid hmac sha1 key");
    mac.update(value.as_bytes());
    Base64Standard.encode(mac.finalize().into_bytes())
}

fn storage_config_error(message: impl Into<String>) -> ApiError {
    ApiError::new(
        StatusCode::BAD_REQUEST,
        "INVALID_STORAGE_CONFIG",
        message,
    )
}

fn storage_request_error(
    code: &'static str,
    driver: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> ApiError {
    let body = body.trim();
    let message = if body.is_empty() {
        format!("{driver} returned HTTP {status}")
    } else {
        let truncated = if body.len() > 240 {
            format!("{}...", &body[..240])
        } else {
            body.to_string()
        };
        format!("{driver} returned HTTP {status}: {truncated}")
    };

    ApiError::new(StatusCode::BAD_GATEWAY, code, message)
}

fn storage_bucket_name(
    storage: &StorageConfigRecord,
    bucket_override: Option<&str>,
) -> Result<String, ApiError> {
    bucket_override
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            storage
                .bucket
                .as_ref()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .ok_or_else(|| storage_config_error("Bucket is required for cloud storage"))
}

fn storage_access_key_id(storage: &StorageConfigRecord) -> Result<String, ApiError> {
    let trimmed = storage.access_key_id.trim();
    if trimmed.is_empty() {
        return Err(storage_config_error(
            "Access Key ID is required for cloud storage",
        ));
    }

    Ok(trimmed.to_string())
}

fn storage_secret_access_key(storage: &StorageConfigRecord) -> Result<String, ApiError> {
    let trimmed = storage.secret_access_key.trim();
    if trimmed.is_empty() {
        return Err(storage_config_error(
            "Secret Access Key is required for cloud storage",
        ));
    }

    Ok(trimmed.to_string())
}

fn storage_signing_region(storage: &StorageConfigRecord) -> String {
    if let Some(region) = storage
        .region
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return region;
    }

    if storage
        .endpoint
        .as_ref()
        .map(|value| value.contains("r2.cloudflarestorage.com"))
        .unwrap_or(false)
    {
        return "auto".to_string();
    }

    "us-east-1".to_string()
}

fn build_storage_request_url(
    storage: &StorageConfigRecord,
    bucket_override: Option<&str>,
    object_key: &str,
) -> Result<reqwest::Url, ApiError> {
    let endpoint = storage
        .endpoint
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| storage_config_error("Endpoint is required for cloud storage"))?;
    let bucket = storage_bucket_name(storage, bucket_override)?;
    let mut url = reqwest::Url::parse(&endpoint)
        .map_err(|_| storage_config_error("Endpoint is not a valid URL"))?;

    if storage.force_path_style {
        let base_path = url.path().trim_end_matches('/');
        let next_path = if base_path.is_empty() || base_path == "/" {
            format!("/{bucket}/{object_key}")
        } else {
            format!("{base_path}/{bucket}/{object_key}")
        };
        url.set_path(&next_path);
    } else {
        let host = url
            .host_str()
            .ok_or_else(|| storage_config_error("Endpoint host is invalid"))?;
        let bucket_host = if host.starts_with(&format!("{bucket}.")) {
            host.to_string()
        } else {
            format!("{bucket}.{host}")
        };
        url.set_host(Some(&bucket_host))
            .map_err(|_| storage_config_error("Bucket and endpoint cannot form a valid host"))?;

        let base_path = url.path().trim_end_matches('/');
        let next_path = if base_path.is_empty() || base_path == "/" {
            format!("/{object_key}")
        } else {
            format!("{base_path}/{object_key}")
        };
        url.set_path(&next_path);
    }

    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn storage_host_header(url: &reqwest::Url) -> Result<String, ApiError> {
    let host = url
        .host_str()
        .ok_or_else(|| storage_config_error("Endpoint host is invalid"))?;

    Ok(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

fn write_local_storage_file(
    uploads_dir: &str,
    object_key: &str,
    bytes: &[u8],
) -> Result<(), ApiError> {
    let target_path =
        std::path::Path::new(uploads_dir).join(object_key.replace('/', "\\"));

    if let Some(parent) = target_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "MEDIA_UPLOAD_FAILED",
                error.to_string(),
            )
        })?;
    }

    std::fs::write(target_path, bytes).map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "MEDIA_UPLOAD_FAILED",
            error.to_string(),
        )
    })?;

    Ok(())
}

fn delete_local_storage_file(uploads_dir: &str, object_key: &str) {
    let target_path =
        std::path::Path::new(uploads_dir).join(object_key.replace('/', "\\"));
    let _ = std::fs::remove_file(target_path);
}

fn upload_to_s3_compatible_storage(
    storage: &StorageConfigRecord,
    bucket_override: Option<&str>,
    object_key: &str,
    mime_type: &str,
    bytes: &[u8],
) -> Result<(), ApiError> {
    let access_key_id = storage_access_key_id(storage)?;
    let secret_access_key = storage_secret_access_key(storage)?;
    let region = storage_signing_region(storage);
    let url = build_storage_request_url(storage, bucket_override, object_key)?;
    let host = storage_host_header(&url)?;
    let payload_hash = sha256_bytes_hex(bytes);
    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let canonical_headers = format!(
        "host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n"
    );
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";
    let canonical_request = format!(
        "PUT\n{}\n\n{}{}\n{}",
        url.path(),
        canonical_headers,
        signed_headers,
        payload_hash
    );
    let credential_scope = format!("{date_stamp}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(&canonical_request)
    );
    let signing_key = {
        let k_date = hmac_sha256_bytes(format!("AWS4{secret_access_key}").as_bytes(), &date_stamp);
        let k_region = hmac_sha256_bytes(&k_date, &region);
        let k_service = hmac_sha256_bytes(&k_region, "s3");
        hmac_sha256_bytes(&k_service, "aws4_request")
    };
    let signature = hmac_sha256_hex(&signing_key, &string_to_sign);
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access_key_id}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );

    let response = HttpClient::new()
        .put(url)
        .header("Authorization", authorization)
        .header("x-amz-content-sha256", payload_hash)
        .header("x-amz-date", amz_date)
        .header("Content-Type", mime_type)
        .body(bytes.to_vec())
        .send()
        .map_err(|error| {
            ApiError::new(StatusCode::BAD_GATEWAY, "MEDIA_UPLOAD_FAILED", error.to_string())
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(storage_request_error(
            "MEDIA_UPLOAD_FAILED",
            &storage.driver,
            status,
            &body,
        ));
    }

    Ok(())
}

fn delete_from_s3_compatible_storage(
    storage: &StorageConfigRecord,
    bucket_override: Option<&str>,
    object_key: &str,
) -> Result<(), ApiError> {
    let access_key_id = storage_access_key_id(storage)?;
    let secret_access_key = storage_secret_access_key(storage)?;
    let region = storage_signing_region(storage);
    let url = build_storage_request_url(storage, bucket_override, object_key)?;
    let host = storage_host_header(&url)?;
    let payload_hash = sha256_hex("");
    let now = Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let canonical_headers = format!(
        "host:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n"
    );
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";
    let canonical_request = format!(
        "DELETE\n{}\n\n{}{}\n{}",
        url.path(),
        canonical_headers,
        signed_headers,
        payload_hash
    );
    let credential_scope = format!("{date_stamp}/{region}/s3/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(&canonical_request)
    );
    let signing_key = {
        let k_date = hmac_sha256_bytes(format!("AWS4{secret_access_key}").as_bytes(), &date_stamp);
        let k_region = hmac_sha256_bytes(&k_date, &region);
        let k_service = hmac_sha256_bytes(&k_region, "s3");
        hmac_sha256_bytes(&k_service, "aws4_request")
    };
    let signature = hmac_sha256_hex(&signing_key, &string_to_sign);
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access_key_id}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );

    let response = HttpClient::new()
        .delete(url)
        .header("Authorization", authorization)
        .header("x-amz-content-sha256", payload_hash)
        .header("x-amz-date", amz_date)
        .send()
        .map_err(|error| {
            ApiError::new(StatusCode::BAD_GATEWAY, "MEDIA_DELETE_FAILED", error.to_string())
        })?;

    if !(response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND) {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(storage_request_error(
            "MEDIA_DELETE_FAILED",
            &storage.driver,
            status,
            &body,
        ));
    }

    Ok(())
}

fn upload_to_aliyun_oss(
    storage: &StorageConfigRecord,
    bucket_override: Option<&str>,
    object_key: &str,
    mime_type: &str,
    bytes: &[u8],
) -> Result<(), ApiError> {
    let access_key_id = storage_access_key_id(storage)?;
    let secret_access_key = storage_secret_access_key(storage)?;
    let bucket = storage_bucket_name(storage, bucket_override)?;
    let url = build_storage_request_url(storage, bucket_override, object_key)?;
    let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    let canonical_resource = format!("/{bucket}/{object_key}");
    let string_to_sign = format!("PUT\n\n{mime_type}\n{date}\n{canonical_resource}");
    let authorization = format!(
        "OSS {}:{}",
        access_key_id,
        hmac_sha1_base64(secret_access_key.as_bytes(), &string_to_sign)
    );

    let response = HttpClient::new()
        .put(url)
        .header("Authorization", authorization)
        .header("Date", date)
        .header("Content-Type", mime_type)
        .body(bytes.to_vec())
        .send()
        .map_err(|error| {
            ApiError::new(StatusCode::BAD_GATEWAY, "MEDIA_UPLOAD_FAILED", error.to_string())
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(storage_request_error(
            "MEDIA_UPLOAD_FAILED",
            &storage.driver,
            status,
            &body,
        ));
    }

    Ok(())
}

fn delete_from_aliyun_oss(
    storage: &StorageConfigRecord,
    bucket_override: Option<&str>,
    object_key: &str,
) -> Result<(), ApiError> {
    let access_key_id = storage_access_key_id(storage)?;
    let secret_access_key = storage_secret_access_key(storage)?;
    let bucket = storage_bucket_name(storage, bucket_override)?;
    let url = build_storage_request_url(storage, bucket_override, object_key)?;
    let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    let canonical_resource = format!("/{bucket}/{object_key}");
    let string_to_sign = format!("DELETE\n\n\n{date}\n{canonical_resource}");
    let authorization = format!(
        "OSS {}:{}",
        access_key_id,
        hmac_sha1_base64(secret_access_key.as_bytes(), &string_to_sign)
    );

    let response = HttpClient::new()
        .delete(url)
        .header("Authorization", authorization)
        .header("Date", date)
        .send()
        .map_err(|error| {
            ApiError::new(StatusCode::BAD_GATEWAY, "MEDIA_DELETE_FAILED", error.to_string())
        })?;

    if !(response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND) {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(storage_request_error(
            "MEDIA_DELETE_FAILED",
            &storage.driver,
            status,
            &body,
        ));
    }

    Ok(())
}

fn build_tencent_cos_authorization(
    storage: &StorageConfigRecord,
    url: &reqwest::Url,
    method: &str,
) -> Result<String, ApiError> {
    let access_key_id = storage_access_key_id(storage)?;
    let secret_access_key = storage_secret_access_key(storage)?;
    let host = storage_host_header(url)?.to_lowercase();
    let sign_start = Utc::now().timestamp();
    let sign_end = sign_start + 600;
    let key_time = format!("{sign_start};{sign_end}");
    let sign_key = hmac_sha1_hex(secret_access_key.as_bytes(), &key_time);
    let http_string = format!(
        "{}\n{}\n\nhost={}\n",
        method.to_lowercase(),
        url.path(),
        host
    );
    let string_to_sign = format!(
        "sha1\n{}\n{}\n",
        key_time,
        sha1_bytes_hex(http_string.as_bytes())
    );
    let signature = hmac_sha1_hex(sign_key.as_bytes(), &string_to_sign);

    Ok(format!(
        "q-sign-algorithm=sha1&q-ak={}&q-sign-time={}&q-key-time={}&q-header-list=host&q-url-param-list=&q-signature={}",
        access_key_id, key_time, key_time, signature
    ))
}

fn upload_to_tencent_cos(
    storage: &StorageConfigRecord,
    bucket_override: Option<&str>,
    object_key: &str,
    mime_type: &str,
    bytes: &[u8],
) -> Result<(), ApiError> {
    let url = build_storage_request_url(storage, bucket_override, object_key)?;
    let authorization = build_tencent_cos_authorization(storage, &url, "PUT")?;

    let response = HttpClient::new()
        .put(url)
        .header("Authorization", authorization)
        .header("Content-Type", mime_type)
        .body(bytes.to_vec())
        .send()
        .map_err(|error| {
            ApiError::new(StatusCode::BAD_GATEWAY, "MEDIA_UPLOAD_FAILED", error.to_string())
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(storage_request_error(
            "MEDIA_UPLOAD_FAILED",
            &storage.driver,
            status,
            &body,
        ));
    }

    Ok(())
}

fn delete_from_tencent_cos(
    storage: &StorageConfigRecord,
    bucket_override: Option<&str>,
    object_key: &str,
) -> Result<(), ApiError> {
    let url = build_storage_request_url(storage, bucket_override, object_key)?;
    let authorization = build_tencent_cos_authorization(storage, &url, "DELETE")?;

    let response = HttpClient::new()
        .delete(url)
        .header("Authorization", authorization)
        .send()
        .map_err(|error| {
            ApiError::new(StatusCode::BAD_GATEWAY, "MEDIA_DELETE_FAILED", error.to_string())
        })?;

    if !(response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND) {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(storage_request_error(
            "MEDIA_DELETE_FAILED",
            &storage.driver,
            status,
            &body,
        ));
    }

    Ok(())
}

fn upload_media_to_storage(
    uploads_dir: &str,
    storage: &StorageConfigRecord,
    bucket_override: Option<&str>,
    object_key: &str,
    mime_type: &str,
    bytes: &[u8],
) -> Result<(), ApiError> {
    match storage.driver.as_str() {
        "local" => write_local_storage_file(uploads_dir, object_key, bytes),
        "s3-compatible" => {
            upload_to_s3_compatible_storage(storage, bucket_override, object_key, mime_type, bytes)
        }
        "aliyun-oss" => upload_to_aliyun_oss(storage, bucket_override, object_key, mime_type, bytes),
        "tencent-cos" => upload_to_tencent_cos(storage, bucket_override, object_key, mime_type, bytes),
        _ => Err(storage_config_error("Unsupported storage driver")),
    }
}

fn delete_media_from_storage(
    uploads_dir: &str,
    storage: &StorageConfigRecord,
    bucket_override: Option<&str>,
    object_key: &str,
) -> Result<(), ApiError> {
    match storage.driver.as_str() {
        "local" => {
            delete_local_storage_file(uploads_dir, object_key);
            Ok(())
        }
        "s3-compatible" => delete_from_s3_compatible_storage(storage, bucket_override, object_key),
        "aliyun-oss" => delete_from_aliyun_oss(storage, bucket_override, object_key),
        "tencent-cos" => delete_from_tencent_cos(storage, bucket_override, object_key),
        _ => Err(storage_config_error("Unsupported storage driver")),
    }
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "code": self.code,
            "message": self.message,
            "data": Value::Null,
            "timestamp": Utc::now().to_rfc3339(),
        }));

        (self.status, body).into_response()
    }
}

fn open_connection(state: &AppState) -> Result<PgClient, ApiError> {
    PgClient::connect(&state.config.database_url, NoTls).map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_OPEN_FAILED",
            format!("Failed to open PostgreSQL database: {}", error),
        )
    })
}

fn db_error(error: postgres::Error) -> ApiError {
    ApiError::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "DATABASE_QUERY_FAILED",
        error.to_string(),
    )
}

fn ensure_default_records(conn: &mut PgClient, config: &Config) -> Result<(), ApiError> {
    Database::new(conn).ensure_default_records(config)
}

fn load_admin_by_username(
    conn: &mut PgClient,
    username: &str,
) -> Result<Option<AdminRecord>, ApiError> {
    Ok(conn
        .query_opt(
            "SELECT id, username, password_hash, display_name, email, avatar_url, status, last_login_at::text
             FROM admins
             WHERE lower(username) = lower($1)
             LIMIT 1",
            &[&username],
        )
        .map_err(db_error)?
        .map(|row| AdminRecord {
            id: row.get(0),
            username: row.get(1),
            password_hash: row.get(2),
            display_name: row.get(3),
            email: row.get(4),
            avatar_url: row.get(5),
            status: row.get(6),
            last_login_at: row.get(7),
        }))
}

fn load_admin_by_id(conn: &mut PgClient, admin_id: &str) -> Result<Option<AdminRecord>, ApiError> {
    Ok(conn
        .query_opt(
            "SELECT id, username, password_hash, display_name, email, avatar_url, status, last_login_at::text
             FROM admins
             WHERE id = $1
             LIMIT 1",
            &[&admin_id],
        )
        .map_err(db_error)?
        .map(|row| AdminRecord {
            id: row.get(0),
            username: row.get(1),
            password_hash: row.get(2),
            display_name: row.get(3),
            email: row.get(4),
            avatar_url: row.get(5),
            status: row.get(6),
            last_login_at: row.get(7),
        }))
}

fn load_active_admin_emails(conn: &mut PgClient) -> Result<Vec<String>, ApiError> {
    Ok(conn
        .query(
            "SELECT email FROM admins WHERE status = 'active' AND email IS NOT NULL AND email != ''",
            &[],
        )
        .map_err(db_error)?
        .iter()
        .map(|row| row.get::<usize, String>(0))
        .filter(|email| !email.trim().is_empty())
        .collect())
}

fn load_public_admin_avatar_url(conn: &mut PgClient) -> Result<Option<String>, ApiError> {
    let row = conn
        .query_opt(
            "SELECT email, avatar_url
             FROM admins
             WHERE status = 'active'
             ORDER BY created_at ASC
             LIMIT 1",
            &[],
        )
        .map_err(db_error)?;

    let Some(row) = row else {
        return Ok(None);
    };

    let email: String = row.get(0);
    let avatar_url: Option<String> = row.get(1);

    if let Some(value) = normalize_optional_text(avatar_url) {
        return Ok(Some(value));
    }

    let trimmed_email = email.trim();
    if trimmed_email.is_empty() {
        return Ok(None);
    }

    Ok(Some(cravatar_url_with_size(trimmed_email, 256)))
}

fn load_admin_session_by_id(
    conn: &mut PgClient,
    session_id: &str,
) -> Result<Option<AdminSessionRecord>, ApiError> {
    Ok(conn
        .query_opt(
            "SELECT id, admin_id, refresh_token_hash, status, expires_at::text
             FROM admin_sessions
             WHERE id = $1
             LIMIT 1",
            &[&session_id],
        )
        .map_err(db_error)?
        .map(|row| AdminSessionRecord {
            id: row.get(0),
            admin_id: row.get(1),
            refresh_token_hash: row.get(2),
            status: row.get(3),
            expires_at: row.get(4),
        }))
}

fn load_admin_session_by_refresh_hash(
    conn: &mut PgClient,
    refresh_hash: &str,
) -> Result<Option<AdminSessionRecord>, ApiError> {
    Ok(conn
        .query_opt(
            "SELECT id, admin_id, refresh_token_hash, status, expires_at::text
             FROM admin_sessions
             WHERE refresh_token_hash = $1
             ORDER BY created_at DESC
             LIMIT 1",
            &[&refresh_hash],
        )
        .map_err(db_error)?
        .map(|row| AdminSessionRecord {
            id: row.get(0),
            admin_id: row.get(1),
            refresh_token_hash: row.get(2),
            status: row.get(3),
            expires_at: row.get(4),
        }))
}

fn to_admin_profile(admin: &AdminRecord) -> AdminProfileItem {
    AdminProfileItem {
        id: admin.id.clone(),
        username: admin.username.clone(),
        display_name: admin.display_name.clone(),
        email: admin.email.clone(),
        avatar_url: admin.avatar_url.clone(),
        status: admin.status.clone(),
        last_login_at: admin.last_login_at.clone(),
    }
}

fn issue_admin_auth_result(
    conn: &mut PgClient,
    admin: &AdminRecord,
    ip: Option<String>,
    user_agent: Option<String>,
    update_last_login_at: bool,
) -> Result<AdminAuthResult, ApiError> {
    Database::new(conn).issue_admin_auth_result(admin, ip, user_agent, update_last_login_at)
}

fn parse_access_token(token: &str) -> Result<(String, String, i64), ApiError> {
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 6 || parts[0] != "aksrt" || parts[1] != "access" {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "INVALID_TOKEN",
            "Access token is invalid",
        ));
    }

    let session_id = parts[2].to_string();
    let admin_id = parts[3].to_string();
    let expires_at = parts[4].parse::<i64>().map_err(|_| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "INVALID_TOKEN",
            "Access token is invalid",
        )
    })?;
    let signature = parts[5];

    let signature_input = format!("access|{}|{}|{}", session_id, admin_id, expires_at);
    let mut mac =
        Hmac::<Sha256>::new_from_slice(access_secret().as_bytes()).expect("invalid access secret");
    mac.update(signature_input.as_bytes());
    let expected = hex_lower(&mac.finalize().into_bytes());

    if signature != expected || expires_at <= Utc::now().timestamp() {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "INVALID_TOKEN",
            "Access token is invalid or expired",
        ));
    }

    Ok((session_id, admin_id, expires_at))
}

fn require_admin_auth(state: &AppState, headers: &HeaderMap) -> Result<AdminAuthContext, ApiError> {
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Authorization header is required",
            )
        })?;

    let token = authorization.strip_prefix("Bearer ").ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "Authorization header is invalid",
        )
    })?;

    let (session_id, admin_id, _) = parse_access_token(token)?;
    let mut conn = open_connection(state)?;

    let session = load_admin_session_by_id(&mut conn, &session_id)?.ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "Admin session is unavailable",
        )
    })?;

    if session.admin_id != admin_id || session.status != "active" {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "Admin session is unavailable",
        ));
    }

    let admin = load_admin_by_id(&mut conn, &admin_id)?.ok_or_else(|| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "Admin is unavailable",
        )
    })?;

    if admin.status != "active" {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
            "Admin is unavailable",
        ));
    }

    Ok(AdminAuthContext { admin, session_id })
}

fn parse_json_records<T>(raw: &str) -> Result<Vec<T>, ApiError>
where
    T: for<'de> Deserialize<'de>,
{
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    serde_json::from_str(raw).map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INVALID_JSON",
            format!("Failed to parse JSON payload: {}", error),
        )
    })
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn cravatar_url(email: &str) -> String {
    let normalized = email.trim().to_lowercase();
    let hash = format!("{:x}", md5::compute(normalized.as_bytes()));
    format!("https://cravatar.cn/avatar/{}?s=80&d=identicon", hash)
}

fn cravatar_url_with_size(email: &str, size: u32) -> String {
    let normalized = email.trim().to_lowercase();
    let hash = format!("{:x}", md5::compute(normalized.as_bytes()));
    format!(
        "https://cravatar.cn/avatar/{}?s={}&d=identicon",
        hash, size
    )
}

async fn get_public_site_settings(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        Ok(ok(read_public_site_settings(
            &mut conn,
            &state.config.public_site_url,
        )?))
    })
    .await
}

async fn get_public_navigation(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let settings = read_public_site_settings(&mut conn, &state.config.public_site_url)?;
        Ok(ok(settings.navigation_items))
    })
    .await
}

async fn get_public_footer_links(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let settings = read_public_site_settings(&mut conn, &state.config.public_site_url)?;
        Ok(ok(settings.footer_links))
    })
    .await
}

async fn get_public_standalone_pages(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let pages = read_enabled_standalone_pages(&mut conn)?;
        Ok(ok(pages
            .into_iter()
            .map(|item| StandalonePageSummaryItem {
                id: item.id,
                title: item.title,
                slug: item.slug,
                summary: item.summary,
                sort_order: item.sort_order,
            })
            .collect::<Vec<_>>()))
    })
    .await
}

async fn get_public_standalone_page(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let page = read_enabled_standalone_pages(&mut conn)?
            .into_iter()
            .find(|item| item.slug == slug)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "NOT_FOUND",
                    "Standalone page not found",
                )
            })?;

        Ok(ok(StandalonePageDetailItem {
            id: page.id,
            title: page.title,
            slug: page.slug,
            summary: page.summary,
            sort_order: page.sort_order,
            content: page.content,
        }))
    })
    .await
}

async fn get_public_captcha_config(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let config = conn
            .query_opt(
                "SELECT enabled, provider, captcha_id, enabled_on_comment, enabled_on_friend_link, enabled_on_login FROM captcha_configs WHERE id = $1",
                &[&"default-captcha-config"],
            )
            .map_err(db_error)?
            .map(|row| CaptchaPublicConfig {
                enabled: row.get(0),
                provider: row.get(1),
                captcha_id: row.get(2),
                enabled_on_comment: row.get(3),
                enabled_on_friend_link: row.get(4),
                enabled_on_login: row.get(5),
            })
            .unwrap_or(CaptchaPublicConfig {
                enabled: false,
                provider: "geetest".to_string(),
                captcha_id: String::new(),
                enabled_on_comment: false,
                enabled_on_friend_link: false,
                enabled_on_login: false,
            });

        Ok(ok(config))
    })
    .await
}

fn read_internal_captcha_config(conn: &mut PgClient) -> Result<InternalCaptchaConfig, ApiError> {
    let row = conn
        .query_opt(
            "SELECT enabled, provider, captcha_id, captcha_key, enabled_on_comment, enabled_on_friend_link, enabled_on_login
             FROM captcha_configs
             WHERE id = $1",
            &[&"default-captcha-config"],
        )
        .map_err(db_error)?;

    Ok(row
        .map(|row| InternalCaptchaConfig {
            enabled: row.get(0),
            provider: row.get(1),
            captcha_id: row.get(2),
            captcha_key: row.get(3),
            enabled_on_comment: row.get(4),
            enabled_on_friend_link: row.get(5),
            enabled_on_login: row.get(6),
        })
        .unwrap_or(InternalCaptchaConfig {
            enabled: false,
            provider: "geetest".to_string(),
            captcha_id: String::new(),
            captcha_key: String::new(),
            enabled_on_comment: false,
            enabled_on_friend_link: false,
            enabled_on_login: false,
        }))
}

fn validate_geetest_captcha(
    _state: &AppState,
    conn: &mut PgClient,
    scene: &'static str,
    captcha: Option<&CaptchaInput>,
) -> Result<(), ApiError> {
    let config = read_internal_captcha_config(conn)?;

    if !config.enabled {
        return Ok(());
    }

    if config.captcha_id.trim().is_empty() || config.captcha_key.trim().is_empty() {
        return Ok(());
    }

    let scene_enabled = match scene {
        "comment" => config.enabled_on_comment,
        "friendLink" => config.enabled_on_friend_link,
        "login" => config.enabled_on_login,
        _ => false,
    };

    if !scene_enabled {
        return Ok(());
    }

    let captcha = captcha.ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "CAPTCHA_REQUIRED",
            "Please complete captcha verification",
        )
    })?;

    if captcha.lot_number.trim().is_empty()
        || captcha.captcha_output.trim().is_empty()
        || captcha.pass_token.trim().is_empty()
        || captcha.gen_time.trim().is_empty()
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "INVALID_CAPTCHA",
            "Captcha payload is incomplete",
        ));
    }

    let mut mac = Hmac::<Sha256>::new_from_slice(config.captcha_key.as_bytes()).map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "CAPTCHA_VALIDATION_ERROR",
            format!("Failed to initialize captcha signing: {error}"),
        )
    })?;
    mac.update(captcha.lot_number.as_bytes());
    let sign_token = hex_lower(&mac.finalize().into_bytes());

    let response = HttpClient::new()
        .post("https://gcaptcha4.geetest.com/validate")
        .form(&[
            ("lot_number", captcha.lot_number.as_str()),
            ("captcha_output", captcha.captcha_output.as_str()),
            ("pass_token", captcha.pass_token.as_str()),
            ("gen_time", captcha.gen_time.as_str()),
            ("captcha_id", config.captcha_id.as_str()),
            ("sign_token", sign_token.as_str()),
        ])
        .send()
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "CAPTCHA_VALIDATION_ERROR",
                format!("Captcha validation service is unavailable: {error}"),
            )
        })?;

    if !response.status().is_success() {
        return Err(ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "CAPTCHA_VALIDATION_ERROR",
            format!("Captcha validation service returned HTTP {}", response.status()),
        ));
    }

    let result = response
        .json::<GeeTestValidationResponse>()
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "CAPTCHA_VALIDATION_ERROR",
                format!("Failed to parse captcha validation response: {error}"),
            )
        })?;

    if result.result != "success" {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "CAPTCHA_VALIDATION_FAILED",
            result
                .reason
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "Captcha verification failed".to_string()),
        ));
    }

    Ok(())
}

fn load_comment_submission_article(
    conn: &mut PgClient,
    fallback_site_url: &str,
    slug: &str,
) -> Result<ArticleRow, ApiError> {
    let article = load_articles(conn)?
        .into_iter()
        .find(|item| item.slug == slug && item.status == "published")
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "ARTICLE_NOT_FOUND",
                "Article was not found",
            )
        })?;

    if !article.allow_comment {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "COMMENT_DISABLED",
            "Comments are disabled for this article",
        ));
    }

    let settings = read_public_site_settings(conn, fallback_site_url)?;
    if !settings.comment_enabled {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "COMMENT_DISABLED",
            "Comments are disabled for this site",
        ));
    }

    Ok(article)
}

fn preview_captcha_value(value: &str) -> String {
    if value.trim().is_empty() {
        return "(empty)".to_string();
    }

    let preview = value.chars().take(12).collect::<String>();
    if value.chars().count() <= 12 {
        format!("{preview} (len={})", value.len())
    } else {
        format!("{preview}... (len={})", value.len())
    }
}

fn summarize_captcha_input(captcha: Option<&CaptchaInput>) -> Option<CaptchaDebugPayloadItem> {
    captcha.map(|value| CaptchaDebugPayloadItem {
        lot_number_preview: preview_captcha_value(&value.lot_number),
        captcha_output_preview: preview_captcha_value(&value.captcha_output),
        pass_token_preview: preview_captcha_value(&value.pass_token),
        gen_time: value.gen_time.clone(),
    })
}

fn read_public_site_settings(
    conn: &mut PgClient,
    fallback_site_url: &str,
) -> Result<PublicSiteSettingsItem, ApiError> {
    let settings = read_public_settings_data(conn, fallback_site_url)?;
    let admin_avatar_url = load_public_admin_avatar_url(conn)?;

    let mut navigation_items = settings
        .navigation_items
        .iter()
        .filter(|item| item.enabled)
        .map(|item| NavigationItemItem {
            id: item.id.clone(),
            label: item.label.clone(),
            href: item.href.clone(),
            sort_order: item.sort_order,
            enabled: item.enabled,
        })
        .collect::<Vec<_>>();
    navigation_items.sort_by_key(|item| item.sort_order);

    let mut footer_links = settings
        .footer_links
        .iter()
        .filter(|item| item.enabled)
        .map(|item| FooterLinkItem {
            id: item.id.clone(),
            label: item.label.clone(),
            href: item.href.clone(),
            icon_url: item.icon_url.clone(),
            description: item.description.clone(),
            sort_order: item.sort_order,
            enabled: item.enabled,
        })
        .collect::<Vec<_>>();
    footer_links.sort_by_key(|item| item.sort_order);

    Ok(PublicSiteSettingsItem {
        site_title: settings.site_title.clone(),
        site_description: settings.site_description.clone(),
        logo_url: settings.logo_url,
        comment_enabled: settings.comment_enabled,
        seo: SeoMeta {
            title: settings.seo_title,
            description: settings.seo_description,
            keywords: settings.seo_keywords,
            canonical_url: settings.seo_canonical_url,
        },
        navigation_items,
        footer_links,
        custom_head_code: settings.custom_head_code,
        custom_footer_code: settings.custom_footer_code,
        icp_filing: settings.icp_filing,
        police_filing: settings.police_filing,
        show_filing: settings.show_filing,
        github_username: settings.github_username,
        about_display_name: settings.about_display_name,
        about_bio: settings.about_bio,
        about_contacts: settings.about_contacts,
        admin_avatar_url,
        article_layout: settings.article_layout,
    })
}

#[derive(Clone)]
struct PublicSettingsData {
    site_title: String,
    site_description: String,
    logo_url: Option<String>,
    comment_enabled: bool,
    seo_title: String,
    seo_description: String,
    seo_keywords: String,
    seo_canonical_url: String,
    navigation_items: Vec<NavigationItemRecord>,
    footer_links: Vec<FooterLinkRecord>,
    standalone_pages: Vec<StandalonePageRecord>,
    custom_head_code: Option<String>,
    custom_footer_code: Option<String>,
    icp_filing: Option<String>,
    police_filing: Option<String>,
    show_filing: bool,
    github_username: Option<String>,
    about_display_name: Option<String>,
    about_bio: Option<String>,
    about_contacts: Vec<AboutContactRecord>,
    article_layout: String,
}

fn read_public_settings_data(
    conn: &mut PgClient,
    fallback_site_url: &str,
) -> Result<PublicSettingsData, ApiError> {
    let row = conn
        .query_opt(
            "SELECT site_title, site_description, logo_url, comment_enabled, seo_title, seo_description, seo_keywords, seo_canonical_url, navigation_items_json::text, footer_links_json::text, standalone_pages_json::text, custom_head_code, custom_footer_code, icp_filing, police_filing, show_filing, github_username, about_display_name, about_bio, about_contacts_json::text, article_layout
             FROM public_site_settings
             WHERE id = $1",
            &[&"default-public-settings"],
        )
        .map_err(db_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "SETTINGS_NOT_FOUND", "Public site settings not found"))?;

    let site_title: String = row.get(0);
    let site_description: String = row.get(1);
    let logo_url: Option<String> = row.get(2);
    let comment_enabled: bool = row.get(3);
    let seo_title: String = row.get(4);
    let seo_description: String = row.get(5);
    let seo_keywords: String = row.get(6);
    let seo_canonical_url: String = row.get(7);
    let navigation_items_json: String = row.get(8);
    let footer_links_json: String = row.get(9);
    let standalone_pages_json: String = row.get(10);
    let custom_head_code: Option<String> = row.get(11);
    let custom_footer_code: Option<String> = row.get(12);
    let icp_filing: Option<String> = row.get(13);
    let police_filing: Option<String> = row.get(14);
    let show_filing: bool = row.get(15);
    let github_username: Option<String> = row.get(16);
    let about_display_name: Option<String> = row.get(17);
    let about_bio: Option<String> = row.get(18);
    let about_contacts_json: String = row.get(19);
    let article_layout: String = row.get(20);

    let mut navigation_items = parse_json_records::<NavigationItemRecord>(&navigation_items_json)?;
    navigation_items.sort_by_key(|item| item.sort_order);

    let mut footer_links = parse_json_records::<FooterLinkRecord>(&footer_links_json)?;
    footer_links.sort_by_key(|item| item.sort_order);

    let mut standalone_pages = parse_json_records::<StandalonePageRecord>(&standalone_pages_json)?;
    standalone_pages.sort_by_key(|item| item.sort_order);
    let about_contacts = parse_json_records::<AboutContactRecord>(&about_contacts_json)?;

    Ok(PublicSettingsData {
        site_title: site_title.clone(),
        site_description: site_description.clone(),
        logo_url,
        comment_enabled,
        seo_title: if seo_title.trim().is_empty() {
            site_title
        } else {
            seo_title
        },
        seo_description: if seo_description.trim().is_empty() {
            site_description
        } else {
            seo_description
        },
        seo_keywords,
        seo_canonical_url: if seo_canonical_url.trim().is_empty() {
            fallback_site_url.to_string()
        } else {
            seo_canonical_url
        },
        navigation_items,
        footer_links,
        standalone_pages,
        custom_head_code,
        custom_footer_code,
        icp_filing,
        police_filing,
        show_filing,
        github_username,
        about_display_name,
        about_bio,
        about_contacts,
        article_layout,
    })
}

fn read_enabled_standalone_pages(
    conn: &mut PgClient,
) -> Result<Vec<StandalonePageRecord>, ApiError> {
    let mut pages = read_public_settings_data(conn, "")?
        .standalone_pages
        .into_iter()
        .filter(|item| item.enabled)
        .collect::<Vec<_>>();
    pages.sort_by_key(|item| item.sort_order);
    Ok(pages)
}

fn read_storage_config_record(conn: &mut PgClient) -> Result<StorageConfigRecord, ApiError> {
    let row = conn
        .query_opt(
            "SELECT id, enabled, driver, endpoint, region, bucket, access_key_id, secret_access_key, public_base_url, base_folder, force_path_style, created_at::text, updated_at::text
             FROM storage_configs
             WHERE id = $1",
            &[&"default-storage-config"],
        )
        .map_err(db_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "STORAGE_CONFIG_NOT_FOUND", "Storage config not found"))?;

    Ok(StorageConfigRecord {
        id: row.get(0),
        enabled: row.get(1),
        driver: row.get(2),
        endpoint: row.get(3),
        region: row.get(4),
        bucket: row.get(5),
        access_key_id: row.get(6),
        secret_access_key: row.get(7),
        public_base_url: row.get(8),
        base_folder: row.get(9),
        force_path_style: row.get(10),
        created_at: row.get(11),
        updated_at: row.get(12),
    })
}

fn to_storage_config_item(record: StorageConfigRecord) -> StorageConfigItem {
    StorageConfigItem {
        id: record.id,
        enabled: record.enabled,
        driver: record.driver,
        endpoint: record.endpoint,
        region: record.region,
        bucket: record.bucket,
        access_key_id: record.access_key_id,
        secret_access_key_configured: !record.secret_access_key.trim().is_empty(),
        public_base_url: record.public_base_url,
        base_folder: record.base_folder,
        force_path_style: record.force_path_style,
        created_at: record.created_at,
        updated_at: record.updated_at,
    }
}

fn read_smtp_config_record(conn: &mut PgClient) -> Result<SmtpConfigRecord, ApiError> {
    let row = conn
        .query_opt(
            "SELECT id, enabled, host, port, secure, username, password, from_name, from_email, created_at::text, updated_at::text, last_test_at::text, last_test_status, last_error_message
             FROM smtp_configs
             WHERE id = $1",
            &[&"default-smtp-config"],
        )
        .map_err(db_error)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "SMTP_CONFIG_NOT_FOUND", "SMTP config not found"))?;

    Ok(SmtpConfigRecord {
        id: row.get(0),
        enabled: row.get(1),
        host: row.get(2),
        port: row.get(3),
        secure: row.get(4),
        username: row.get(5),
        password: row.get(6),
        from_name: row.get(7),
        from_email: row.get(8),
        created_at: row.get(9),
        updated_at: row.get(10),
        last_test_at: row.get(11),
        last_test_status: row.get(12),
        last_error_message: row.get(13),
    })
}

fn to_smtp_config_item(record: SmtpConfigRecord) -> SmtpConfigItem {
    SmtpConfigItem {
        id: record.id,
        enabled: record.enabled,
        host: record.host,
        port: record.port,
        secure: record.secure,
        username: record.username,
        password_configured: !record.password.trim().is_empty(),
        from_name: record.from_name,
        from_email: record.from_email,
        created_at: record.created_at,
        updated_at: record.updated_at,
        last_test_at: record.last_test_at,
        last_test_status: record.last_test_status,
        last_error_message: record.last_error_message,
    }
}

fn smtp_mailbox(
    address: &str,
    display_name: Option<&str>,
    label: &'static str,
) -> Result<Mailbox, ApiError> {
    let trimmed = address.trim();
    if !validate_email(trimmed) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "INVALID_SMTP_CONFIG",
            format!("{label} email is invalid"),
        ));
    }

    let parsed_address = trimmed.parse().map_err(|error| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "INVALID_SMTP_CONFIG",
            format!("{label} email is invalid: {error}"),
        )
    })?;

    Ok(Mailbox::new(
        display_name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        parsed_address,
    ))
}

fn ensure_smtp_send_ready(config: &SmtpConfigRecord) -> Result<(), ApiError> {
    if config.host.trim().is_empty()
        || config.username.trim().is_empty()
        || config.password.trim().is_empty()
        || config.from_email.trim().is_empty()
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "SMTP_INCOMPLETE",
            "SMTP configuration is incomplete",
        ));
    }

    Ok(())
}

fn build_smtp_transport(config: &SmtpConfigRecord) -> Result<SmtpTransport, ApiError> {
    ensure_smtp_send_ready(config)?;
    let port = u16::try_from(config.port).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "INVALID_SMTP_CONFIG",
            "SMTP port is invalid",
        )
    })?;
    let credentials = Credentials::new(config.username.clone(), config.password.clone());
    let host = config.host.trim().to_string();

    let mailer = if config.secure {
        SmtpTransport::relay(&host)
            .map_err(|error| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "SMTP_SEND_FAILED",
                    format!("Failed to prepare secure SMTP transport: {error}"),
                )
            })?
            .port(port)
            .credentials(credentials)
            .build()
    } else {
        let tls_parameters = TlsParameters::new(host.clone()).map_err(|error| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "SMTP_SEND_FAILED",
                format!("Failed to prepare SMTP TLS settings: {error}"),
            )
        })?;

        SmtpTransport::builder_dangerous(&host)
            .port(port)
            .tls(Tls::Opportunistic(tls_parameters))
            .credentials(credentials)
            .build()
    };

    Ok(mailer)
}

fn send_smtp_email(
    config: &SmtpConfigRecord,
    to_email: &str,
    subject: &str,
    text_body: String,
    html_body: String,
) -> Result<(), ApiError> {
    if !validate_email(to_email) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "INVALID_EMAIL",
            "Recipient email is invalid",
        ));
    }

    let from = smtp_mailbox(&config.from_email, Some(&config.from_name), "From")?;
    let to = smtp_mailbox(to_email, None, "Recipient")?;

    let message_builder = Message::builder().from(from).to(to).subject(subject);

    let message = message_builder
        .multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(text_body),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html_body),
                ),
        )
        .map_err(|error| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "SMTP_SEND_FAILED",
                format!("Failed to build SMTP email: {error}"),
            )
        })?;

    let mailer = build_smtp_transport(config)?;

    mailer.send(&message).map_err(|error| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "SMTP_SEND_FAILED",
            format!("Failed to send SMTP email: {error}"),
        )
    })?;

    Ok(())
}

fn send_smtp_test_email(config: &SmtpConfigRecord, to_email: &str) -> Result<String, ApiError> {
    let sent_at = Utc::now().to_rfc3339();
    let html_body = format!(
        "<h2>SMTP test successful</h2><p>This message confirms the current SMTP configuration is able to send mail.</p><p>Sent at: {sent_at}</p>"
    );
    let text_body = format!("SMTP test successful\nSent at: {sent_at}");

    send_smtp_email(
        config,
        to_email,
        "[Blog] SMTP test email",
        text_body,
        html_body,
    )?;

    Ok(Uuid::new_v4().to_string())
}

fn send_notification_emails(
    config: &SmtpConfigRecord,
    subject: &str,
    text_body: String,
    html_body: String,
    recipient_emails: &[String],
) -> Result<bool, ApiError> {
    if !config.enabled || recipient_emails.is_empty() {
        return Ok(false);
    }

    for to_email in recipient_emails {
        if !to_email.trim().is_empty() {
            let _ = send_smtp_email(config, to_email.trim(), subject, text_body.clone(), html_body.clone());
        }
    }

    Ok(true)
}

fn try_send_pending_comment_notification(
    conn: &mut PgClient,
    public_site_url: &str,
    article: &ArticleRow,
    notification: &PendingCommentNotification,
) {
    let article_url = site_url(public_site_url, &format!("/articles/{}", article.slug));
    let admin_url = site_url(public_site_url, "/admin/comments");
    let website_line = notification
        .website
        .as_deref()
        .map(|value| format!("\nWebsite: {value}"))
        .unwrap_or_default();
    let reply_line = notification
        .parent_id
        .as_deref()
        .map(|value| format!("\nReply to comment: {value}"))
        .unwrap_or_default();
    let website_html = notification
        .website
        .as_deref()
        .map(|value| format!("<p><strong>Website:</strong> {}</p>", escape_html(value)))
        .unwrap_or_default();
    let reply_html = notification
        .parent_id
        .as_deref()
        .map(|value| format!("<p><strong>Reply to comment:</strong> {}</p>", escape_html(value)))
        .unwrap_or_default();

    let text_body = format!(
        "A new comment is waiting for review.\n\nArticle: {}\nArticle URL: {}\nAdmin review: {}\nStatus: {}\nNickname: {}\nEmail: {}{}{}\n\nContent:\n{}",
        article.title,
        article_url,
        admin_url,
        notification.status,
        notification.nickname,
        notification.email,
        website_line,
        reply_line,
        notification.content,
    );
    let html_body = format!(
        "<h2>New comment pending review</h2>\
         <p><strong>Article:</strong> {}</p>\
         <p><strong>Article URL:</strong> <a href=\"{article_url}\">{article_url}</a></p>\
         <p><strong>Admin review:</strong> <a href=\"{admin_url}\">{admin_url}</a></p>\
         <p><strong>Status:</strong> {}</p>\
         <p><strong>Nickname:</strong> {}</p>\
         <p><strong>Email:</strong> {}</p>\
         {}{}\
         <p><strong>Content:</strong></p>\
         <p>{}</p>",
        escape_html(&notification.article_title),
        escape_html(&notification.status),
        escape_html(&notification.nickname),
        escape_html(&notification.email),
        website_html,
        reply_html,
        htmlize_multiline_text(&notification.content),
    );

    match read_smtp_config_record(conn).and_then(|config| {
        let mut recipients = vec![notification.email.clone()];
        if let Ok(admin_emails) = load_active_admin_emails(conn) {
            for admin_email in admin_emails {
                if admin_email != notification.email {
                    recipients.push(admin_email);
                }
            }
        }
        send_notification_emails(
            &config,
            &format!("[Blog] New comment pending: {}", notification.article_title),
            text_body,
            html_body,
            &recipients,
        )
    }) {
        Ok(_) => {}
        Err(error) => {
            eprintln!(
                "Failed to send pending comment notification for article {}: {}",
                notification.article_slug, error.message
            );
        }
    }
}

fn try_send_pending_friend_link_notification(
    conn: &mut PgClient,
    public_site_url: &str,
    notification: &PendingFriendLinkNotification,
) {
    let admin_url = site_url(public_site_url, "/admin/friend-links");
    let icon_line = notification
        .icon_url
        .as_deref()
        .map(|value| format!("\nIcon URL: {value}"))
        .unwrap_or_default();
    let message_line = notification
        .message
        .as_deref()
        .map(|value| format!("\nMessage:\n{value}"))
        .unwrap_or_default();
    let icon_html = notification
        .icon_url
        .as_deref()
        .map(|value| format!("<p><strong>Icon URL:</strong> {}</p>", escape_html(value)))
        .unwrap_or_default();
    let message_html = notification
        .message
        .as_deref()
        .map(|value| {
            format!(
                "<p><strong>Message:</strong></p><p>{}</p>",
                htmlize_multiline_text(value)
            )
        })
        .unwrap_or_default();

    let text_body = format!(
        "A new friend link application is waiting for review.\n\nSite: {}\nSite URL: {}\nAdmin review: {}\nStatus: {}\nContact: {}\nContact email: {}{}\n\nDescription:\n{}{}",
        notification.site_name,
        notification.site_url,
        admin_url,
        notification.status,
        notification.contact_name,
        notification.contact_email,
        icon_line,
        notification.description,
        message_line,
    );
    let html_body = format!(
        "<h2>New friend link application pending review</h2>\
         <p><strong>Site:</strong> {}</p>\
         <p><strong>Site URL:</strong> <a href=\"{}\">{}</a></p>\
         <p><strong>Admin review:</strong> <a href=\"{admin_url}\">{admin_url}</a></p>\
         <p><strong>Status:</strong> {}</p>\
         <p><strong>Contact:</strong> {}</p>\
         <p><strong>Contact email:</strong> {}</p>\
         {}\
         <p><strong>Description:</strong></p>\
         <p>{}</p>\
         {}",
        escape_html(&notification.site_name),
        escape_html(&notification.site_url),
        escape_html(&notification.site_url),
        escape_html(&notification.status),
        escape_html(&notification.contact_name),
        escape_html(&notification.contact_email),
        icon_html,
        htmlize_multiline_text(&notification.description),
        message_html,
    );

    match read_smtp_config_record(conn).and_then(|config| {
        let mut recipients = vec![notification.contact_email.clone()];
        if let Ok(admin_emails) = load_active_admin_emails(conn) {
            for admin_email in admin_emails {
                if admin_email != notification.contact_email {
                    recipients.push(admin_email);
                }
            }
        }
        send_notification_emails(
            &config,
            &format!("[Blog] New friend link pending: {}", notification.site_name),
            text_body,
            html_body,
            &recipients,
        )
    }) {
        Ok(_) => {}
        Err(error) => {
            eprintln!(
                "Failed to send pending friend link notification for site {}: {}",
                notification.site_name, error.message
            );
        }
    }
}

fn try_send_comment_review_notification(
    conn: &mut PgClient,
    public_site_url: &str,
    comment: &AdminCommentItem,
) {
    let status_label = match comment.status.as_str() {
        "approved" => "已通过",
        "rejected" => "已拒绝",
        _ => return,
    };

    let article_url = site_url(public_site_url, &format!("/articles/{}", comment.article.slug));
    let reject_line = comment
        .reject_reason
        .as_deref()
        .filter(|r| !r.trim().is_empty())
        .map(|r| format!("\n拒绝原因: {r}"))
        .unwrap_or_default();
    let reject_html = comment
        .reject_reason
        .as_deref()
        .filter(|r| !r.trim().is_empty())
        .map(|r| format!("<p><strong>拒绝原因:</strong> {}</p>", escape_html(r)))
        .unwrap_or_default();

    let text_body = format!(
        "您在文章「{}」下的评论审核结果: {}\n\n文章链接: {}\n评论内容: {}\n{}",
        comment.article.title, status_label, article_url, comment.content, reject_line,
    );
    let html_body = format!(
        "<h2>评论审核结果通知</h2>\
         <p><strong>文章:</strong> {}</p>\
         <p><strong>文章链接:</strong> <a href=\"{article_url}\">{article_url}</a></p>\
         <p><strong>审核结果:</strong> {}</p>\
         <p><strong>评论内容:</strong> {}</p>\
         {}",
        escape_html(&comment.article.title),
        status_label,
        htmlize_multiline_text(&comment.content),
        reject_html,
    );

    match read_smtp_config_record(conn).and_then(|config| {
        let recipients = vec![comment.email.clone()];
        send_notification_emails(
            &config,
            &format!("[Blog] 评论审核结果: {}", status_label),
            text_body,
            html_body,
            &recipients,
        )
    }) {
        Ok(_) => {}
        Err(error) => {
            eprintln!(
                "Failed to send comment review notification for comment {}: {}",
                comment.id, error.message
            );
        }
    }
}

fn try_send_friend_link_review_notification(
    conn: &mut PgClient,
    public_site_url: &str,
    application: &AdminFriendLinkApplicationItem,
) {
    let status_label = match application.status.as_str() {
        "approved" => "已通过",
        "rejected" => "已拒绝",
        _ => return,
    };

    let note_line = application
        .review_note
        .as_deref()
        .filter(|n| !n.trim().is_empty())
        .map(|n| format!("\n审核备注: {n}"))
        .unwrap_or_default();
    let note_html = application
        .review_note
        .as_deref()
        .filter(|n| !n.trim().is_empty())
        .map(|n| format!("<p><strong>审核备注:</strong> {}</p>", escape_html(n)))
        .unwrap_or_default();

    let site_url_link = site_url(public_site_url, "/");

    let text_body = format!(
        "您的友链申请「{}」审核结果: {}\n\n站点地址: {}\n{}",
        application.site_name, status_label, application.site_url, note_line,
    );
    let html_body = format!(
        "<h2>友链申请审核结果通知</h2>\
         <p><strong>站点名称:</strong> {}</p>\
         <p><strong>站点地址:</strong> <a href=\"{}\">{}</a></p>\
         <p><strong>审核结果:</strong> {}</p>\
         <p><strong>我们的站点:</strong> <a href=\"{site_url_link}\">{site_url_link}</a></p>\
         {}",
        escape_html(&application.site_name),
        escape_html(&application.site_url),
        escape_html(&application.site_url),
        status_label,
        note_html,
    );

    match read_smtp_config_record(conn).and_then(|config| {
        let recipients = vec![application.contact_email.clone()];
        send_notification_emails(
            &config,
            &format!("[Blog] 友链申请审核结果: {}", status_label),
            text_body,
            html_body,
            &recipients,
        )
    }) {
        Ok(_) => {}
        Err(error) => {
            eprintln!(
                "Failed to send friend link review notification for application {}: {}",
                application.id, error.message
            );
        }
    }
}

async fn list_public_articles(
    State(state): State<AppState>,
    Query(query): Query<ArticleListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        Ok(ok(list_public_articles_impl(&mut conn, query)?))
    })
    .await
}

async fn list_public_categories(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let mut categories = load_categories(&mut conn)?
            .into_values()
            .filter(|item| item.is_enabled)
            .map(|item| ArticleTaxonomyItem {
                id: item.id,
                name: item.name,
                slug: item.slug,
            })
            .collect::<Vec<_>>();
        categories.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(ok(categories))
    })
    .await
}

async fn get_public_article(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let article = load_articles(&mut conn)?
            .into_iter()
            .find(|item| item.slug == slug && item.status == "published")
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "ARTICLE_NOT_FOUND",
                    "Article was not found",
                )
            })?;

        Ok(ok(build_article_detail(&mut conn, article)?))
    })
    .await
}

async fn list_public_comments(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Query(query): Query<CommentListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let article = load_articles(&mut conn)?
            .into_iter()
            .find(|item| item.slug == slug && item.status == "published")
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "ARTICLE_NOT_FOUND",
                    "Article was not found",
                )
            })?;

        let comments = load_public_comments(&mut conn, &article.id)?;
        let total = comments.len();
        let start = (query.page.saturating_sub(1)) * query.page_size;
        let list = comments
            .into_iter()
            .skip(start)
            .take(query.page_size)
            .collect::<Vec<_>>();

        Ok(ok(PaginatedResponse {
            list,
            total,
            page: query.page,
            page_size: query.page_size,
        }))
    })
    .await
}

async fn submit_public_comment(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(input): Json<CreateCommentInput>,
) -> Result<impl IntoResponse, ApiError> {
    let public_site_url = state.config.public_site_url.clone();
    let database_url = state.config.database_url.clone();
    let result = run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let article =
            load_comment_submission_article(&mut conn, &state.config.public_site_url, &slug)?;
        validate_geetest_captcha(&state, &mut conn, "comment", input.captcha.as_ref())?;
        let notification = PendingCommentNotification {
            article_title: article.title.clone(),
            article_slug: article.slug.clone(),
            nickname: input.nickname.trim().to_string(),
            email: input.email.trim().to_string(),
            website: normalize_optional_text(input.website.clone()),
            content: input.content.trim().to_string(),
            parent_id: input.parent_id.clone(),
            status: "pending".to_string(),
        };
        let (ip, user_agent) = extract_client_meta(&headers);
        let result = Database::new(&mut conn).create_public_comment(
            &article.id,
            input,
            ip,
            user_agent,
        )?;
        Ok((result, article, notification))
    })
    .await?;

    let (_, article, notification) = &result;
    let article = article.clone();
    let notification = notification.clone();
    tokio::spawn(async move {
        tokio::task::spawn_blocking(move || {
            if let Ok(mut conn) = PgClient::connect(&database_url, NoTls) {
                try_send_pending_comment_notification(
                    &mut conn,
                    &public_site_url,
                    &article,
                    &notification,
                );
            }
        })
        .await
        .ok();
    });

    Ok(created(result.0))
}

async fn submit_public_friend_link_application(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateFriendLinkApplicationInput>,
) -> Result<impl IntoResponse, ApiError> {
    let public_site_url = state.config.public_site_url.clone();
    let database_url = state.config.database_url.clone();
    let result = run_blocking(move || {
        let mut conn = open_connection(&state)?;
        validate_geetest_captcha(&state, &mut conn, "friendLink", input.captcha.as_ref())?;
        let notification = PendingFriendLinkNotification {
            site_name: input.site_name.trim().to_string(),
            site_url: input.site_url.trim().to_string(),
            icon_url: normalize_optional_text(input.icon_url.clone()),
            description: input.description.trim().to_string(),
            contact_name: input.contact_name.trim().to_string(),
            contact_email: input.contact_email.trim().to_string(),
            message: normalize_optional_text(input.message.clone()),
            status: "pending".to_string(),
        };
        let (ip, user_agent) = extract_client_meta(&headers);
        let result = Database::new(&mut conn).create_friend_link_application(input, ip, user_agent)?;
        Ok((result, notification))
    })
    .await?;

    let notification = result.1.clone();
    tokio::spawn(async move {
        tokio::task::spawn_blocking(move || {
            if let Ok(mut conn) = PgClient::connect(&database_url, NoTls) {
                try_send_pending_friend_link_notification(
                    &mut conn,
                    &public_site_url,
                    &notification,
                );
            }
        })
        .await
        .ok();
    });

    Ok(created(result.0))
}

async fn list_public_banners(
    State(state): State<AppState>,
    Query(query): Query<BannerListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let rows = conn
            .query(
                "SELECT id, title, description, image_url, link_url, link_target, position, sort_order, status, show_text, created_at::text, updated_at::text
                 FROM banners
                 WHERE status = 'enabled'
                 ORDER BY sort_order ASC, updated_at DESC",
                &[],
            )
            .map_err(db_error)?
            .into_iter()
            .map(|row| BannerItem {
                id: row.get(0),
                title: row.get(1),
                description: row.get(2),
                image_url: row.get(3),
                link_url: row.get(4),
                link_target: row.get(5),
                position: row.get(6),
                sort_order: row.get(7),
                status: row.get(8),
                show_text: row.get(9),
                created_at: row.get(10),
                updated_at: row.get(11),
            })
            .collect::<Vec<_>>();

        let list = if let Some(position) = query.position {
            rows.into_iter()
                .filter(|item| item.position == position)
                .collect::<Vec<_>>()
        } else {
            rows
        };

        Ok(ok(list))
    })
    .await
}

async fn list_public_projects(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let list = conn
            .query(
                "SELECT id, title, description, icon, link, sort_order, enabled, created_at::text, updated_at::text
                 FROM projects
                 WHERE enabled = TRUE
                 ORDER BY sort_order ASC",
                &[],
            )
            .map_err(db_error)?
            .into_iter()
            .map(|row| {
                let sort_order: i32 = row.get(5);
                ProjectItem {
                    id: row.get(0),
                    title: row.get(1),
                    description: row.get(2),
                    icon: row.get(3),
                    link: row.get(4),
                    sort_order: i64::from(sort_order),
                    enabled: row.get(6),
                    created_at: row.get(7),
                    updated_at: row.get(8),
                }
            })
            .collect::<Vec<_>>();

        Ok(ok(list))
    })
    .await
}

async fn get_activity_stats(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        Ok(ok(build_activity_stats(&mut conn)?))
    })
    .await
}

fn read_public_sync_version(conn: &mut PgClient) -> Result<String, ApiError> {
    let row = conn
        .query_one(
            "SELECT GREATEST(
                COALESCE((SELECT MAX(updated_at) FROM public_site_settings), TIMESTAMPTZ '1970-01-01 00:00:00+00'),
                COALESCE((SELECT MAX(updated_at) FROM articles), TIMESTAMPTZ '1970-01-01 00:00:00+00'),
                COALESCE((SELECT MAX(updated_at) FROM article_categories), TIMESTAMPTZ '1970-01-01 00:00:00+00'),
                COALESCE((SELECT MAX(updated_at) FROM article_tags), TIMESTAMPTZ '1970-01-01 00:00:00+00'),
                COALESCE((SELECT MAX(created_at) FROM article_category_links), TIMESTAMPTZ '1970-01-01 00:00:00+00'),
                COALESCE((SELECT MAX(created_at) FROM article_tag_links), TIMESTAMPTZ '1970-01-01 00:00:00+00'),
                COALESCE((SELECT MAX(updated_at) FROM banners), TIMESTAMPTZ '1970-01-01 00:00:00+00'),
                COALESCE((SELECT MAX(updated_at) FROM projects), TIMESTAMPTZ '1970-01-01 00:00:00+00'),
                COALESCE((SELECT MAX(updated_at) FROM comments), TIMESTAMPTZ '1970-01-01 00:00:00+00'),
                COALESCE((SELECT MAX(updated_at) FROM friend_link_applications), TIMESTAMPTZ '1970-01-01 00:00:00+00')
            )::text",
            &[],
        )
        .map_err(db_error)?;

    let version: String = row.get(0);
    Ok(version)
}

async fn get_public_sync_version(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        Ok(ok(PublicSyncVersionItem {
            version: read_public_sync_version(&mut conn)?,
        }))
    })
    .await
}

fn serialize_json_value<T: Serialize>(value: &T) -> Result<serde_json::Value, ApiError> {
    serde_json::to_value(value).map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "JSON_SERIALIZE_FAILED",
            error.to_string(),
        )
    })
}

async fn admin_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AdminLoginInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        validate_geetest_captcha(&state, &mut conn, "login", input.captcha.as_ref())?;

        let username = input.username.trim().to_string();
        let password = input.password;

        require_length(
            &username,
            3,
            50,
            "INVALID_USERNAME",
            "Username must be between 3 and 50 characters",
        )?;
        require_length(
            &password,
            8,
            128,
            "INVALID_PASSWORD",
            "Password must be between 8 and 128 characters",
        )?;

        let admin = load_admin_by_username(&mut conn, &username)?.ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "INVALID_CREDENTIALS",
                "Username or password is incorrect",
            )
        })?;

        if admin.status != "active" {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "ACCOUNT_DISABLED",
                "Admin account is disabled",
            ));
        }

        if !verify_password(&password, &admin.password_hash) {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "INVALID_CREDENTIALS",
                "Username or password is incorrect",
            ));
        }

        let (ip, user_agent) = extract_client_meta(&headers);
        Ok(ok(issue_admin_auth_result(
            &mut conn, &admin, ip, user_agent, true,
        )?))
    })
    .await
}

async fn admin_refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AdminRefreshInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let refresh_token = input.refresh_token.trim().to_string();
        require_length(
            &refresh_token,
            20,
            512,
            "INVALID_REFRESH_TOKEN",
            "Refresh token is invalid",
        )?;

        let refresh_hash = sha256_hex(&refresh_token);
        let session =
            load_admin_session_by_refresh_hash(&mut conn, &refresh_hash)?.ok_or_else(|| {
                ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "INVALID_REFRESH_TOKEN",
                    "Refresh token is invalid or expired",
                )
            })?;

        if session.status != "active" || session.refresh_token_hash != refresh_hash {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "INVALID_REFRESH_TOKEN",
                "Refresh token is invalid or expired",
            ));
        }

        let expires_at =
            chrono::DateTime::parse_from_rfc3339(&session.expires_at).map_err(|_| {
                ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "INVALID_REFRESH_TOKEN",
                    "Refresh token is invalid or expired",
                )
            })?;

        if expires_at.with_timezone(&Utc) <= Utc::now() {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "INVALID_REFRESH_TOKEN",
                "Refresh token is invalid or expired",
            ));
        }

        let admin = load_admin_by_id(&mut conn, &session.admin_id)?.ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "INVALID_REFRESH_TOKEN",
                "Admin is unavailable",
            )
        })?;

        if admin.status != "active" {
            return Err(ApiError::new(
                StatusCode::UNAUTHORIZED,
                "INVALID_REFRESH_TOKEN",
                "Admin is unavailable",
            ));
        }

        Database::new(&mut conn).revoke_admin_session(&session.id)?;

        let (ip, user_agent) = extract_client_meta(&headers);
        Ok(ok(issue_admin_auth_result(
            &mut conn, &admin, ip, user_agent, false,
        )?))
    })
    .await
}

async fn admin_get_me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let auth = require_admin_auth(&state, &headers)?;
        let _ = &auth.session_id;
        Ok(ok(to_admin_profile(&auth.admin)))
    })
    .await
}

async fn admin_update_me(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateAdminProfileInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;

        let current = load_admin_by_id(&mut conn, &auth.admin.id)?.ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Admin is unavailable",
            )
        })?;

        let username = input
            .username
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.username.clone());
        let email = input
            .email
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.email.clone());
        let display_name = input
            .display_name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(current.display_name.clone());

        require_length(
            &username,
            3,
            50,
            "INVALID_USERNAME",
            "Username must be between 3 and 50 characters",
        )?;
        require_length(
            &email,
            3,
            255,
            "INVALID_EMAIL",
            "Email must be between 3 and 255 characters",
        )?;
        require_length(
            &display_name,
            1,
            100,
            "INVALID_DISPLAY_NAME",
            "Display name is invalid",
        )?;

        if !validate_email(&email) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_EMAIL",
                "Email format is invalid",
            ));
        }

        if username != current.username {
            if let Some(existing) = load_admin_by_username(&mut conn, &username)? {
                if existing.id != current.id {
                    return Err(ApiError::new(
                        StatusCode::CONFLICT,
                        "USERNAME_EXISTS",
                        "Username already exists",
                    ));
                }
            }
        }

        Ok(ok(Database::new(&mut conn).update_admin_profile(
            &current.id,
            username,
            email,
            display_name,
        )?))
    })
    .await
}

async fn admin_change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ChangeAdminPasswordInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let admin = load_admin_by_id(&mut conn, &auth.admin.id)?.ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "Admin is unavailable",
            )
        })?;

        require_length(
            &input.current_password,
            8,
            128,
            "INVALID_PASSWORD",
            "Current password is invalid",
        )?;
        require_length(
            &input.new_password,
            8,
            128,
            "INVALID_PASSWORD",
            "New password must be between 8 and 128 characters",
        )?;

        if !verify_password(&input.current_password, &admin.password_hash) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_PASSWORD",
                "Current password is incorrect",
            ));
        }

        if input.current_password == input.new_password {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_PASSWORD",
                "New password must be different from current password",
            ));
        }

        Database::new(&mut conn).change_admin_password(&admin.id, &input.new_password)?;

        Ok(ok(json!({ "message": "Password changed successfully" })))
    })
    .await
}

async fn admin_get_site_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(AdminSiteSettingsItem {
            public_config: read_public_site_settings(&mut conn, &state.config.public_site_url)?,
            smtp_config: to_smtp_config_item(read_smtp_config_record(&mut conn)?),
            storage_config: to_storage_config_item(read_storage_config_record(&mut conn)?),
        }))
    })
    .await
}

async fn admin_get_public_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(read_public_site_settings(
            &mut conn,
            &state.config.public_site_url,
        )?))
    })
    .await
}

async fn admin_update_public_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdatePublicSiteSettingsInput>,
) -> Result<impl IntoResponse, ApiError> {
    let public_site_url = state.config.public_site_url.clone();
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(
            Database::new(&mut conn).update_public_site_settings(&public_site_url, input)?
        ))
    })
    .await
}

async fn admin_get_navigation_items(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(read_public_settings_data(
            &mut conn,
            &state.config.public_site_url,
        )?
        .navigation_items))
    })
    .await
}

async fn admin_update_navigation_items(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateNavigationItemsEnvelope>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).replace_navigation_items(input)?))
    })
    .await
}

async fn admin_get_footer_links(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(read_public_settings_data(
            &mut conn,
            &state.config.public_site_url,
        )?
        .footer_links))
    })
    .await
}

async fn admin_update_footer_links(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateFooterLinksEnvelope>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).replace_footer_links(input)?))
    })
    .await
}

async fn admin_get_standalone_pages(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(read_public_settings_data(
            &mut conn,
            &state.config.public_site_url,
        )?
        .standalone_pages))
    })
    .await
}

async fn admin_update_standalone_pages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateStandalonePagesEnvelope>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).replace_standalone_pages(input)?))
    })
    .await
}

async fn admin_get_storage_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(to_storage_config_item(read_storage_config_record(
            &mut conn,
        )?)))
    })
    .await
}

async fn admin_update_storage_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateStorageConfigInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).update_storage_config(input)?))
    })
    .await
}

async fn admin_get_captcha_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let config = read_internal_captcha_config(&mut conn)?;
        Ok(ok(CaptchaAdminConfigItem {
            id: "default-captcha-config".to_string(),
            enabled: config.enabled,
            provider: config.provider,
            captcha_id: config.captcha_id,
            captcha_key_configured: !config.captcha_key.trim().is_empty(),
            enabled_on_comment: config.enabled_on_comment,
            enabled_on_friend_link: config.enabled_on_friend_link,
            enabled_on_login: config.enabled_on_login,
        }))
    })
    .await
}

async fn admin_update_captcha_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateCaptchaConfigInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).update_captcha_config(input)?))
    })
    .await
}

async fn admin_debug_comment_captcha(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<AdminCommentCaptchaDebugInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let config = read_internal_captcha_config(&mut conn)?;
        let payload = summarize_captcha_input(input.captcha.as_ref());
        let articles = load_articles(&mut conn)?;
        let article = articles.into_iter().find(|item| item.slug == input.slug);
        let site_settings = read_public_site_settings(&mut conn, &state.config.public_site_url)?;

        let article_found = article.is_some();
        let article_published = article
            .as_ref()
            .map(|item| item.status == "published")
            .unwrap_or(false);
        let article_allow_comment = article
            .as_ref()
            .map(|item| item.allow_comment)
            .unwrap_or(false);
        let article_title = article.as_ref().map(|item| item.title.clone());
        let site_comment_enabled = site_settings.comment_enabled;
        let captcha_id_configured = !config.captcha_id.trim().is_empty();
        let captcha_key_configured = !config.captcha_key.trim().is_empty();
        let captcha_required =
            config.enabled && config.enabled_on_comment && captcha_id_configured && captcha_key_configured;

        let (validation_attempted, validation_passed, code, message) = if !article_found
            || !article_published
        {
            (
                false,
                false,
                "ARTICLE_NOT_FOUND".to_string(),
                "Article was not found".to_string(),
            )
        } else if !article_allow_comment {
            (
                false,
                false,
                "COMMENT_DISABLED".to_string(),
                "Comments are disabled for this article".to_string(),
            )
        } else if !site_comment_enabled {
            (
                false,
                false,
                "COMMENT_DISABLED".to_string(),
                "Comments are disabled for this site".to_string(),
            )
        } else if captcha_required {
            match validate_geetest_captcha(&state, &mut conn, "comment", input.captcha.as_ref()) {
                Ok(()) => (
                    true,
                    true,
                    "OK".to_string(),
                    "GeeTest validation passed for the comment dry run. No comment was created."
                        .to_string(),
                ),
                Err(error) => (true, false, error.code.to_string(), error.message),
            }
        } else if !config.enabled {
            (
                false,
                false,
                "CAPTCHA_NOT_REQUIRED".to_string(),
                "Captcha is disabled globally in the saved config, so comment submissions currently bypass validation."
                    .to_string(),
            )
        } else if !config.enabled_on_comment {
            (
                false,
                false,
                "CAPTCHA_NOT_REQUIRED".to_string(),
                "Captcha is enabled globally, but the comment scene is disabled in the saved config."
                    .to_string(),
            )
        } else {
            (
                false,
                false,
                "CAPTCHA_NOT_REQUIRED".to_string(),
                "The saved captcha ID or key is incomplete, so comment submissions currently bypass validation."
                    .to_string(),
            )
        };

        let comment_would_be_accepted = article_found
            && article_published
            && article_allow_comment
            && site_comment_enabled
            && (validation_passed || !captcha_required);

        Ok(ok(CommentCaptchaDebugItem {
            scene: "comment".to_string(),
            dry_run: true,
            provider: config.provider,
            article_slug: input.slug,
            article_title,
            article_found,
            article_published,
            article_allow_comment,
            site_comment_enabled,
            captcha_enabled: config.enabled,
            captcha_id_configured,
            captcha_key_configured,
            enabled_on_comment: config.enabled_on_comment,
            captcha_required,
            validation_attempted,
            validation_passed,
            comment_would_be_accepted,
            code,
            message,
            payload,
        }))
    })
    .await
}

fn load_admin_comment_items(conn: &mut PgClient) -> Result<Vec<AdminCommentItem>, ApiError> {
    let rows = conn
        .query(
            "SELECT c.id, c.article_id, a.title, a.slug, c.parent_id, c.nickname, c.email, c.website, c.content, c.status,
                    c.reviewed_by, c.reviewed_at::text, c.reject_reason, c.created_at::text, c.updated_at::text
             FROM comments c
             JOIN articles a ON a.id = c.article_id
             ORDER BY c.created_at DESC",
            &[],
        )
        .map_err(db_error)?;

    let parent_rows = conn
        .query("SELECT id, nickname, status FROM comments", &[])
        .map_err(db_error)?;
    let mut parent_map = HashMap::<String, AdminCommentParentRef>::new();
    for row in parent_rows {
        parent_map.insert(
            row.get::<usize, String>(0),
            AdminCommentParentRef {
                id: row.get(0),
                nickname: row.get(1),
                status: row.get(2),
            },
        );
    }

    Ok(rows
        .into_iter()
        .map(|row| {
            let parent_id: Option<String> = row.get(4);
            AdminCommentItem {
                id: row.get(0),
                article: AdminCommentArticleRef {
                    id: row.get(1),
                    title: row.get(2),
                    slug: row.get(3),
                },
                parent: parent_id
                    .as_ref()
                    .and_then(|value| parent_map.get(value))
                    .cloned(),
                nickname: row.get(5),
                email: row.get(6),
                website: row.get(7),
                content: row.get(8),
                status: row.get(9),
                reviewed_by: row.get(10),
                reviewed_at: row.get(11),
                reject_reason: row.get(12),
                created_at: row.get(13),
                updated_at: row.get(14),
            }
        })
        .collect())
}

fn load_admin_comment_item(
    conn: &mut PgClient,
    comment_id: &str,
) -> Result<AdminCommentItem, ApiError> {
    load_admin_comment_items(conn)?
        .into_iter()
        .find(|item| item.id == comment_id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "COMMENT_NOT_FOUND",
                "Comment was not found",
            )
        })
}

async fn admin_list_comments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminCommentListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let mut items = load_admin_comment_items(&mut conn)?;

        if let Some(status) = query
            .status
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            items.retain(|item| item.status == *status);
        }
        if let Some(article_id) = query
            .article_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            items.retain(|item| item.article.id == *article_id);
        }
        if let Some(keyword) = query
            .keyword
            .as_ref()
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
        {
            items.retain(|item| {
                format!(
                    "{} {} {} {}",
                    item.nickname, item.email, item.content, item.article.title
                )
                .to_lowercase()
                .contains(&keyword)
            });
        }

        match query.sort_by.as_str() {
            "updatedAt" => items.sort_by(|left, right| left.updated_at.cmp(&right.updated_at)),
            _ => items.sort_by(|left, right| left.created_at.cmp(&right.created_at)),
        }
        if query.sort_order != "asc" {
            items.reverse();
        }

        let total = items.len();
        let start = (query.page.saturating_sub(1)) * query.page_size;
        let list = items
            .into_iter()
            .skip(start)
            .take(query.page_size)
            .collect::<Vec<_>>();

        Ok(ok(PaginatedResponse {
            list,
            total,
            page: query.page,
            page_size: query.page_size,
        }))
    })
    .await
}

async fn admin_review_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ReviewCommentInput>,
) -> Result<impl IntoResponse, ApiError> {
    let public_site_url = state.config.public_site_url.clone();
    let database_url = state.config.database_url.clone();
    let result = run_blocking(move || {
        let auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let result = Database::new(&mut conn).review_comment(
            &id,
            input,
            &auth.admin.id,
        )?;
        Ok(result)
    })
    .await?;

    let comment = result.clone();
    let public_site_url_clone = public_site_url.clone();
    tokio::spawn(async move {
        tokio::task::spawn_blocking(move || {
            if let Ok(mut conn) = PgClient::connect(&database_url, NoTls) {
                try_send_comment_review_notification(&mut conn, &public_site_url_clone, &comment);
            }
        })
        .await
        .ok();
    });

    Ok(ok(result))
}

async fn admin_delete_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Database::new(&mut conn).delete_comment(&id)?;
        Ok(ok(json!({})))
    })
    .await
}

fn load_friend_link_applications(
    conn: &mut PgClient,
) -> Result<Vec<AdminFriendLinkApplicationItem>, ApiError> {
    Ok(conn
        .query(
            "SELECT id, site_name, site_url, icon_url, description, contact_name, contact_email, message, status,
                    review_note, reviewed_by, reviewed_at::text, linked_footer_link_id, created_at::text, updated_at::text
             FROM friend_link_applications
             ORDER BY created_at DESC",
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|row| AdminFriendLinkApplicationItem {
            id: row.get(0),
            site_name: row.get(1),
            site_url: row.get(2),
            icon_url: row.get(3),
            description: row.get(4),
            contact_name: row.get(5),
            contact_email: row.get(6),
            message: row.get(7),
            status: row.get(8),
            review_note: row.get(9),
            reviewed_by: row.get(10),
            reviewed_at: row.get(11),
            linked_footer_link_id: row.get(12),
            created_at: row.get(13),
            updated_at: row.get(14),
        })
        .collect())
}

fn sync_footer_link_for_application(
    settings: &mut PublicSettingsData,
    application: &AdminFriendLinkApplicationItem,
    next_status: &str,
) -> String {
    if next_status == "approved" {
        let linked_id = application
            .linked_footer_link_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        if let Some(existing) = settings
            .footer_links
            .iter_mut()
            .find(|item| item.id == linked_id)
        {
            existing.label = application.site_name.clone();
            existing.href = application.site_url.clone();
            existing.icon_url = application.icon_url.clone();
            existing.description = application.description.clone();
            existing.enabled = true;
        } else {
            let sort_order = settings
                .footer_links
                .iter()
                .map(|item| item.sort_order)
                .max()
                .unwrap_or(-1)
                + 1;
            settings.footer_links.push(FooterLinkRecord {
                id: linked_id.clone(),
                label: application.site_name.clone(),
                href: application.site_url.clone(),
                icon_url: application.icon_url.clone(),
                description: application.description.clone(),
                sort_order,
                enabled: true,
            });
        }

        settings.footer_links.sort_by_key(|item| item.sort_order);
        return linked_id;
    }

    if let Some(linked_id) = application.linked_footer_link_id.clone() {
        if let Some(existing) = settings
            .footer_links
            .iter_mut()
            .find(|item| item.id == linked_id)
        {
            existing.enabled = false;
        }
        return linked_id;
    }

    String::new()
}

async fn admin_list_friend_link_applications(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(load_friend_link_applications(&mut conn)?))
    })
    .await
}

async fn admin_review_friend_link_application(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ReviewFriendLinkApplicationInput>,
) -> Result<impl IntoResponse, ApiError> {
    let public_site_url = state.config.public_site_url.clone();
    let database_url = state.config.database_url.clone();
    let public_site_url_for_closure = public_site_url.clone();
    let result = run_blocking(move || {
        let auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let result = Database::new(&mut conn)
            .review_friend_link_application(
                &id,
                input,
                &auth.admin.id,
                &public_site_url_for_closure,
            )?;
        Ok(result)
    })
    .await?;

    let application = result.clone();
    tokio::spawn(async move {
        tokio::task::spawn_blocking(move || {
            if let Ok(mut conn) = PgClient::connect(&database_url, NoTls) {
                try_send_friend_link_review_notification(&mut conn, &public_site_url, &application);
            }
        })
        .await
        .ok();
    });

    Ok(ok(result))
}

fn build_admin_category_item(record: &ArticleCategoryRecord) -> AdminCategoryItem {
    AdminCategoryItem {
        id: record.id.clone(),
        name: record.name.clone(),
        slug: record.slug.clone(),
        description: record.description.clone(),
        is_enabled: record.is_enabled,
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
    }
}

fn build_admin_tag_item(record: &ArticleTagRecord) -> AdminTagItem {
    AdminTagItem {
        id: record.id.clone(),
        name: record.name.clone(),
        slug: record.slug.clone(),
        created_at: record.created_at.clone(),
        updated_at: record.updated_at.clone(),
    }
}

fn generate_next_article_slug(conn: &mut PgClient) -> Result<String, ApiError> {
    let rows = conn
        .query("SELECT slug FROM articles", &[])
        .map_err(db_error)?
        .into_iter()
        .map(|row| row.get::<usize, String>(0))
        .collect::<Vec<_>>();

    let next = rows
        .into_iter()
        .filter_map(|slug| {
            slug.strip_prefix("article-")
                .and_then(|value| value.parse::<i64>().ok())
        })
        .max()
        .unwrap_or(0)
        + 1;

    Ok(format!("article-{}", next))
}

fn resolve_publication(
    status: &str,
    published_at: Option<String>,
    current: Option<&ArticleRow>,
) -> Result<Option<String>, ApiError> {
    if status == "draft" {
        return Ok(None);
    }

    if let Some(value) = published_at {
        if value.trim().is_empty() {
            return Ok(Some(Utc::now().to_rfc3339()));
        }
        chrono::DateTime::parse_from_rfc3339(&value).map_err(|_| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_PUBLISHED_AT",
                "publishedAt is invalid",
            )
        })?;
        return Ok(Some(value));
    }

    Ok(current
        .and_then(|item| item.published_at.clone())
        .or_else(|| Some(Utc::now().to_rfc3339())))
}

fn ensure_unique_category_slug(
    conn: &mut PgClient,
    slug: &str,
    ignore_id: Option<&str>,
) -> Result<(), ApiError> {
    let existing = conn
        .query_opt(
            "SELECT id FROM article_categories WHERE slug = $1",
            &[&slug],
        )
        .map_err(db_error)?;
    if let Some(row) = existing {
        let existing_id: String = row.get(0);
        if Some(existing_id.as_str()) != ignore_id {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "CATEGORY_SLUG_EXISTS",
                "Category slug already exists",
            ));
        }
    }
    Ok(())
}

fn ensure_unique_tag_slug(
    conn: &mut PgClient,
    slug: &str,
    ignore_id: Option<&str>,
) -> Result<(), ApiError> {
    let existing = conn
        .query_opt("SELECT id FROM article_tags WHERE slug = $1", &[&slug])
        .map_err(db_error)?;
    if let Some(row) = existing {
        let existing_id: String = row.get(0);
        if Some(existing_id.as_str()) != ignore_id {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "TAG_SLUG_EXISTS",
                "Tag slug already exists",
            ));
        }
    }
    Ok(())
}

fn ensure_unique_article_slug(
    conn: &mut PgClient,
    slug: &str,
    ignore_id: Option<&str>,
) -> Result<(), ApiError> {
    let existing = conn
        .query_opt("SELECT id FROM articles WHERE slug = $1", &[&slug])
        .map_err(db_error)?;
    if let Some(row) = existing {
        let existing_id: String = row.get(0);
        if Some(existing_id.as_str()) != ignore_id {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "ARTICLE_SLUG_EXISTS",
                "Article slug already exists",
            ));
        }
    }
    Ok(())
}

fn resolve_category_ids(
    conn: &mut PgClient,
    category_ids: &[String],
) -> Result<Vec<String>, ApiError> {
    let categories = load_categories(conn)?;
    for category_id in category_ids {
        let category = categories.get(category_id).ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_CATEGORIES",
                "One or more categories are invalid",
            )
        })?;
        if !category.is_enabled {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_CATEGORIES",
                "One or more categories are invalid or disabled",
            ));
        }
    }
    Ok(category_ids.to_vec())
}

fn resolve_tag_ids(conn: &mut PgClient, tag_ids: &[String]) -> Result<Vec<String>, ApiError> {
    let tags = load_tags(conn)?;
    for tag_id in tag_ids {
        if !tags.contains_key(tag_id) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "INVALID_TAGS",
                "One or more tags are invalid",
            ));
        }
    }
    Ok(tag_ids.to_vec())
}

async fn admin_get_article_editor_options(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let mut categories = load_categories(&mut conn)?
            .into_values()
            .filter(|item| item.is_enabled)
            .map(|item| ArticleTaxonomyItem {
                id: item.id,
                name: item.name,
                slug: item.slug,
            })
            .collect::<Vec<_>>();
        categories.sort_by(|left, right| left.name.cmp(&right.name));

        let mut tags = load_tags(&mut conn)?
            .into_values()
            .map(|item| ArticleTaxonomyItem {
                id: item.id,
                name: item.name,
                slug: item.slug,
            })
            .collect::<Vec<_>>();
        tags.sort_by(|left, right| left.name.cmp(&right.name));

        Ok(ok(ArticleEditorOptions { categories, tags }))
    })
    .await
}

async fn admin_list_categories(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let mut items = load_categories(&mut conn)?
            .into_values()
            .map(|item| build_admin_category_item(&item))
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(ok(items))
    })
    .await
}

async fn admin_create_category(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateCategoryInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(created(Database::new(&mut conn).create_category(input)?))
    })
    .await
}

async fn admin_update_category(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateCategoryInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).update_category(&id, input)?))
    })
    .await
}

async fn admin_delete_category(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Database::new(&mut conn).delete_category(&id)?;
        Ok(ok(json!({})))
    })
    .await
}

async fn admin_list_tags(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let mut items = load_tags(&mut conn)?
            .into_values()
            .map(|item| build_admin_tag_item(&item))
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(ok(items))
    })
    .await
}

async fn admin_create_tag(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateTagInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(created(Database::new(&mut conn).create_tag(input)?))
    })
    .await
}

async fn admin_update_tag(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateTagInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).update_tag(&id, input)?))
    })
    .await
}

async fn admin_delete_tag(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Database::new(&mut conn).delete_tag(&id)?;
        Ok(ok(json!({})))
    })
    .await
}

async fn admin_list_articles(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminArticleListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let mut items = load_articles(&mut conn)?
            .into_iter()
            .map(|item| build_article_summary(&mut conn, item))
            .collect::<Result<Vec<_>, _>>()?;

        if let Some(keyword) = query
            .keyword
            .as_ref()
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
        {
            items.retain(|item| {
                format!("{} {} {}", item.title, item.excerpt, item.slug)
                    .to_lowercase()
                    .contains(&keyword)
            });
        }
        if let Some(status) = query
            .status
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            items.retain(|item| item.status == *status);
        }
        if let Some(category_id) = query
            .category_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            items.retain(|item| {
                item.categories
                    .iter()
                    .any(|category| category.id == *category_id)
            });
        }
        if let Some(tag_id) = query
            .tag_id
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            items.retain(|item| item.tags.iter().any(|tag| tag.id == *tag_id));
        }
        if let Some(allow_comment) = query.allow_comment.as_ref() {
            let wanted = allow_comment == "true";
            items.retain(|item| item.allow_comment == wanted);
        }

        match query.sort_by.as_str() {
            "title" => items.sort_by(|left, right| left.title.cmp(&right.title)),
            "createdAt" => items.sort_by(|left, right| left.created_at.cmp(&right.created_at)),
            "publishedAt" => {
                items.sort_by(|left, right| left.published_at.cmp(&right.published_at))
            }
            _ => items.sort_by(|left, right| left.updated_at.cmp(&right.updated_at)),
        }
        if query.sort_order != "asc" {
            items.reverse();
        }

        let total = items.len();
        let start = (query.page.saturating_sub(1)) * query.page_size;
        let list = items
            .into_iter()
            .skip(start)
            .take(query.page_size)
            .collect::<Vec<_>>();
        Ok(ok(PaginatedResponse {
            list,
            total,
            page: query.page,
            page_size: query.page_size,
        }))
    })
    .await
}

async fn admin_get_article(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let article = load_articles(&mut conn)?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "ARTICLE_NOT_FOUND",
                    "Article was not found",
                )
            })?;
        Ok(ok(build_article_detail(&mut conn, article)?))
    })
    .await
}

async fn admin_create_article(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateArticleInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(created(
            Database::new(&mut conn).create_article(input, &auth.admin.id)?,
        ))
    })
    .await
}

async fn admin_update_article(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateArticleInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).update_article(
            &id,
            input,
            &auth.admin.id,
        )?))
    })
    .await
}

async fn admin_delete_article(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Database::new(&mut conn).delete_article(&id)?;
        Ok(ok(json!({})))
    })
    .await
}

fn load_admin_banners(conn: &mut PgClient) -> Result<Vec<BannerItem>, ApiError> {
    Ok(conn
        .query(
            "SELECT id, title, description, image_url, link_url, link_target, position, sort_order, status, show_text, created_at::text, updated_at::text
             FROM banners",
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|row| BannerItem {
            id: row.get(0),
            title: row.get(1),
            description: row.get(2),
            image_url: row.get(3),
            link_url: row.get(4),
            link_target: row.get(5),
            position: row.get(6),
            sort_order: row.get(7),
            status: row.get(8),
            show_text: row.get(9),
            created_at: row.get(10),
            updated_at: row.get(11),
        })
        .collect())
}

async fn admin_list_banners(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminBannerListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let mut items = load_admin_banners(&mut conn)?;

        if let Some(keyword) = query
            .keyword
            .as_ref()
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
        {
            items.retain(|item| {
                format!("{} {}", item.title, item.link_url)
                    .to_lowercase()
                    .contains(&keyword)
            });
        }
        if let Some(position) = query
            .position
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            items.retain(|item| item.position == *position);
        }
        if let Some(status) = query
            .status
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            items.retain(|item| item.status == *status);
        }

        match query.sort_by.as_str() {
            "createdAt" => items.sort_by(|left, right| left.created_at.cmp(&right.created_at)),
            "updatedAt" => items.sort_by(|left, right| left.updated_at.cmp(&right.updated_at)),
            "title" => items.sort_by(|left, right| left.title.cmp(&right.title)),
            _ => items.sort_by(|left, right| left.sort_order.cmp(&right.sort_order)),
        }
        if query.sort_order != "asc" {
            items.reverse();
        }

        let total = items.len();
        let start = (query.page.saturating_sub(1)) * query.page_size;
        let list = items
            .into_iter()
            .skip(start)
            .take(query.page_size)
            .collect::<Vec<_>>();
        Ok(ok(PaginatedResponse {
            list,
            total,
            page: query.page,
            page_size: query.page_size,
        }))
    })
    .await
}

async fn admin_create_banner(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateBannerInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(created(
            Database::new(&mut conn).create_banner(input, &auth.admin.id)?,
        ))
    })
    .await
}

async fn admin_get_banner(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let item = load_admin_banners(&mut conn)?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "BANNER_NOT_FOUND",
                    "Banner was not found",
                )
            })?;
        Ok(ok(item))
    })
    .await
}

async fn admin_update_banner(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateBannerInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).update_banner(
            &id,
            input,
            &auth.admin.id,
        )?))
    })
    .await
}

async fn admin_delete_banner(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Database::new(&mut conn).delete_banner(&id)?;
        Ok(ok(json!({})))
    })
    .await
}

async fn admin_reorder_banners(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ReorderBannersInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Database::new(&mut conn).reorder_banners(&input.ids)?;
        Ok(ok(json!({})))
    })
    .await
}

fn load_admin_projects(conn: &mut PgClient) -> Result<Vec<ProjectItem>, ApiError> {
    Ok(conn
        .query(
            "SELECT id, title, description, icon, link, sort_order, enabled, created_at::text, updated_at::text
             FROM projects
             ORDER BY sort_order ASC, created_at ASC",
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|row| {
            let sort_order: i32 = row.get(5);
            ProjectItem {
                id: row.get(0),
                title: row.get(1),
                description: row.get(2),
                icon: row.get(3),
                link: row.get(4),
                sort_order: i64::from(sort_order),
                enabled: row.get(6),
                created_at: row.get(7),
                updated_at: row.get(8),
            }
        })
        .collect())
}

async fn admin_list_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(load_admin_projects(&mut conn)?))
    })
    .await
}

async fn admin_update_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateProjectsInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).replace_projects(input)?))
    })
    .await
}

fn sanitize_file_extension(filename: &str) -> String {
    let ext = filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    if ext.len() > 10 || !ext.chars().all(|char| char.is_ascii_alphanumeric()) {
        String::new()
    } else if ext.is_empty() {
        String::new()
    } else {
        format!(".{}", ext)
    }
}

fn sanitize_folder_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || matches!(char, '/' | '_' | '-') {
                char
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('/')
        .to_string();

    if sanitized.is_empty() {
        "misc".to_string()
    } else {
        sanitized
    }
}

fn load_media_assets(conn: &mut PgClient) -> Result<Vec<MediaAssetItem>, ApiError> {
    Ok(conn
        .query(
            "SELECT id, provider, filename, original_filename, mime_type, size, url, usage, status,
                    COALESCE(NULLIF(title, ''), REGEXP_REPLACE(COALESCE(NULLIF(original_filename, ''), filename), '\\.[^.]+$', '')),
                    alt_text, caption, description,
                    created_at::text, updated_at::text
             FROM media_assets",
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|row| MediaAssetItem {
            id: row.get(0),
            provider: row.get(1),
            filename: row.get(2),
            original_filename: row.get(3),
            mime_type: row.get(4),
            size: row.get(5),
            url: row.get(6),
            usage: row.get(7),
            status: row.get(8),
            title: row.get(9),
            alt_text: row.get(10),
            caption: row.get(11),
            description: row.get(12),
            created_at: row.get(13),
            updated_at: row.get(14),
        })
        .collect())
}

async fn admin_list_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AdminMediaListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let mut items = load_media_assets(&mut conn)?;

        if let Some(keyword) = query
            .keyword
            .as_ref()
            .map(|value| value.trim().to_lowercase())
            .filter(|value| !value.is_empty())
        {
            items.retain(|item| {
                format!("{} {}", item.filename, item.original_filename)
                    .to_lowercase()
                    .contains(&keyword)
            });
        }
        if let Some(mime_type) = query
            .mime_type
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            items.retain(|item| item.mime_type == *mime_type);
        }
        if let Some(usage) = query
            .usage
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            items.retain(|item| item.usage == *usage);
        }
        if let Some(status) = query
            .status
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            items.retain(|item| item.status == *status);
        }

        match query.sort_by.as_str() {
            "filename" => items.sort_by(|left, right| left.filename.cmp(&right.filename)),
            "size" => items.sort_by(|left, right| left.size.cmp(&right.size)),
            _ => items.sort_by(|left, right| left.created_at.cmp(&right.created_at)),
        }
        if query.sort_order != "asc" {
            items.reverse();
        }

        let total = items.len();
        let start = (query.page.saturating_sub(1)) * query.page_size;
        let list = items
            .into_iter()
            .skip(start)
            .take(query.page_size)
            .collect::<Vec<_>>();
        Ok(ok(PaginatedResponse {
            list,
            total,
            page: query.page,
            page_size: query.page_size,
        }))
    })
    .await
}

async fn admin_upload_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ApiError> {
    let auth = {
        let state = state.clone();
        let headers = headers.clone();
        run_blocking(move || require_admin_auth(&state, &headers)).await?
    };
    let mut usage = "misc".to_string();
    let mut file_name: Option<String> = None;
    let mut mime_type: Option<String> = None;
    let mut bytes: Option<Vec<u8>> = None;

    while let Some(field) = multipart.next_field().await.map_err(|error| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "INVALID_MULTIPART",
            error.to_string(),
        )
    })? {
        match field.name() {
            Some("usage") => {
                usage = field.text().await.map_err(|error| {
                    ApiError::new(
                        StatusCode::BAD_REQUEST,
                        "INVALID_MULTIPART",
                        error.to_string(),
                    )
                })?;
            }
            Some("file") => {
                file_name = field.file_name().map(|value| value.to_string());
                mime_type = field.content_type().map(|value| value.to_string());
                bytes = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|error| {
                            ApiError::new(
                                StatusCode::BAD_REQUEST,
                                "INVALID_MULTIPART",
                                error.to_string(),
                            )
                        })?
                        .to_vec(),
                );
            }
            _ => {}
        }
    }

    let file_name = file_name.ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "FILE_REQUIRED",
            "Upload file is required",
        )
    })?;
    let mime_type = mime_type.unwrap_or_else(|| "application/octet-stream".to_string());
    let bytes = bytes.ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "FILE_REQUIRED",
            "Upload file is required",
        )
    })?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "FILE_TOO_LARGE",
            "Upload file exceeds size limit",
        ));
    }
    if !matches!(
        mime_type.as_str(),
        "image/jpeg" | "image/png" | "image/webp" | "image/gif" | "image/svg+xml"
    ) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "UNSUPPORTED_FILE_TYPE",
            "Only image uploads are supported",
        ));
    }

    let usage = usage.trim().to_string();
    if !matches!(
        usage.as_str(),
        "article_cover" | "article_content" | "banner" | "site_asset" | "misc"
    ) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "INVALID_USAGE",
            "Media usage is invalid",
        ));
    }

    let storage = {
        let state = state.clone();
        run_blocking(move || {
            let mut conn = open_connection(&state)?;
            let storage = read_storage_config_record(&mut conn)?;
            if !storage.enabled {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "STORAGE_DISABLED",
                    "Object storage is disabled",
                ));
            }
            Ok(storage)
        })
        .await?
    };

    let extension = sanitize_file_extension(&file_name);
    let generated_filename = format!("{}{}", Uuid::new_v4(), extension);
    let object_key = format!(
        "{}/{}",
        sanitize_folder_name(&format!("{}/{}", storage.base_folder, usage)),
        generated_filename
    );
    {
        let uploads_dir = state.config.uploads_dir.clone();
        let storage = storage.clone();
        let object_key = object_key.clone();
        let mime_type = mime_type.clone();
        let bytes = bytes.clone();

        run_blocking(move || {
            upload_media_to_storage(
                &uploads_dir,
                &storage,
                None,
                &object_key,
                &mime_type,
                &bytes,
            )
        })
        .await?;
    }

    let asset_id = Uuid::new_v4().to_string();
    let url = format!(
        "{}/{}",
        storage.public_base_url.trim_end_matches('/'),
        object_key
    );
    let item = {
        let state = state.clone();
        let provider = storage.driver.clone();
        let bucket = storage.bucket.clone();
        let object_key = object_key.clone();
        let generated_filename = generated_filename.clone();
        let original_file_name = file_name.clone();
        let mime_type = mime_type.clone();
        let normalized_extension =
            normalize_optional_text(Some(extension.trim_start_matches('.').to_string()));
        let size = bytes.len() as i64;
        let usage = usage.clone();
        let uploaded_by = auth.admin.id.clone();
        let url = url.clone();

        run_blocking(move || {
            let mut conn = open_connection(&state)?;
            Database::new(&mut conn).create_media_asset(CreateMediaAssetRecordInput {
                id: asset_id,
                provider,
                bucket,
                object_key,
                filename: generated_filename,
                original_filename: original_file_name,
                mime_type,
                extension: normalized_extension,
                size,
                url,
                usage,
                uploaded_by,
            })
        })
        .await
    };

    let item = match item {
        Ok(item) => item,
        Err(error) => {
            let uploads_dir = state.config.uploads_dir.clone();
            let storage = storage.clone();
            let object_key = object_key.clone();

            if let Err(cleanup_error) = run_blocking(move || {
                delete_media_from_storage(&uploads_dir, &storage, None, &object_key)
            })
            .await
            {
                eprintln!(
                    "Failed to clean up media object after DB insert failure: {}",
                    cleanup_error.message
                );
            }

            return Err(error);
        }
    };
    Ok(created(item))
}

async fn admin_get_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let item = load_media_assets(&mut conn)?
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::NOT_FOUND,
                    "MEDIA_NOT_FOUND",
                    "Media asset was not found",
                )
            })?;
        Ok(ok(item))
    })
    .await
}

async fn admin_update_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateMediaAssetInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let item = Database::new(&mut conn).update_media_asset_metadata(&id, input)?;
        Ok(ok(item))
    })
    .await
}

async fn admin_delete_media(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    {
        let state = state.clone();
        let headers = headers.clone();
        let _auth = run_blocking(move || require_admin_auth(&state, &headers)).await?;
    }

    let locator = {
        let state = state.clone();
        let id = id.clone();
        run_blocking(move || {
            let mut conn = open_connection(&state)?;
            Database::new(&mut conn).find_media_storage_locator(&id)
        })
        .await?
    };

    let storage = {
        let state = state.clone();
        run_blocking(move || {
            let mut conn = open_connection(&state)?;
            read_storage_config_record(&mut conn)
        })
        .await?
    };

    let cleanup_storage = StorageConfigRecord {
        driver: locator.provider.clone(),
        bucket: locator.bucket.clone().or(storage.bucket.clone()),
        ..storage.clone()
    };
    let uploads_dir = state.config.uploads_dir.clone();
    let object_key = locator.object_key.clone();
    let bucket = locator.bucket.clone();

    if let Err(error) = run_blocking(move || {
        delete_media_from_storage(
            &uploads_dir,
            &cleanup_storage,
            bucket.as_deref(),
            &object_key,
        )
    })
    .await
    {
        eprintln!("Failed to delete media object from storage: {}", error.message);
    }

    {
        let state = state.clone();
        let id = id.clone();
        run_blocking(move || {
            let mut conn = open_connection(&state)?;
            Database::new(&mut conn).mark_media_deleted(&id)
        })
        .await?;
    }
    Ok(ok(json!({})))
}

async fn admin_get_smtp_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(to_smtp_config_item(read_smtp_config_record(&mut conn)?)))
    })
    .await
}

async fn admin_update_smtp_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateSmtpConfigInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        Ok(ok(Database::new(&mut conn).update_smtp_config(input)?))
    })
    .await
}

async fn admin_send_test_email(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SendTestEmailInput>,
) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let _auth = require_admin_auth(&state, &headers)?;
        let mut conn = open_connection(&state)?;
        let config = read_smtp_config_record(&mut conn)?;

        match send_smtp_test_email(&config, input.to_email.trim()) {
            Ok(message_id) => {
                Database::new(&mut conn).update_smtp_test_status("success", None)?;
                Ok(ok(json!({
                    "success": true,
                    "messageId": message_id,
                })))
            }
            Err(error) => {
                if let Err(status_error) = Database::new(&mut conn)
                    .update_smtp_test_status("failed", Some(error.message.as_str()))
                {
                    return Err(ApiError::new(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "SMTP_SEND_FAILED",
                        format!(
                            "{}; additionally failed to save SMTP test status: {}",
                            error.message, status_error.message
                        ),
                    ));
                }

                Err(error)
            }
        }
    })
    .await
}

fn list_public_articles_impl(
    conn: &mut PgClient,
    query: ArticleListQuery,
) -> Result<PaginatedResponse<ArticleSummaryItem>, ApiError> {
    let mut items = load_articles(conn)?
        .into_iter()
        .filter(|item| item.status == "published")
        .map(|item| build_article_summary(conn, item))
        .collect::<Result<Vec<_>, _>>()?;

    if let Some(keyword) = query
        .keyword
        .as_ref()
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
    {
        items.retain(|item| {
            let haystack = format!(
                "{} {} {} {}",
                item.title,
                item.excerpt,
                item.categories
                    .iter()
                    .map(|value| value.name.clone())
                    .collect::<Vec<_>>()
                    .join(" "),
                item.tags
                    .iter()
                    .map(|value| value.name.clone())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase();
            haystack.contains(&keyword)
        });
    }

    if let Some(category_slug) = query
        .category_slug
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        items.retain(|item| {
            item.categories
                .iter()
                .any(|category| category.slug == *category_slug)
        });
    }

    if let Some(tag_slug) = query.tag_slug.as_ref().filter(|value| !value.is_empty()) {
        items.retain(|item| item.tags.iter().any(|tag| tag.slug == *tag_slug));
    }

    match query.sort_by.as_str() {
        "title" => items.sort_by(|left, right| left.title.cmp(&right.title)),
        "createdAt" => items.sort_by(|left, right| left.created_at.cmp(&right.created_at)),
        _ => items.sort_by(|left, right| left.published_at.cmp(&right.published_at)),
    }

    if query.sort_order != "asc" {
        items.reverse();
    }

    let total = items.len();
    let start = (query.page.saturating_sub(1)) * query.page_size;
    let list = items
        .into_iter()
        .skip(start)
        .take(query.page_size)
        .collect::<Vec<_>>();

    Ok(PaginatedResponse {
        list,
        total,
        page: query.page,
        page_size: query.page_size,
    })
}

fn load_articles(conn: &mut PgClient) -> Result<Vec<ArticleRow>, ApiError> {
    let rows = conn.query(
        "SELECT id, title, slug, excerpt, content, cover_image_url, status, allow_comment, published_at::text, created_by, updated_by, created_at::text, updated_at::text FROM articles",
        &[],
    )
    .map_err(db_error)?
    .into_iter()
    .map(|row| ArticleRow {
        id: row.get(0),
        title: row.get(1),
        slug: row.get(2),
        excerpt: row.get(3),
        content: row.get(4),
        cover_image_url: row.get(5),
        status: row.get(6),
        allow_comment: row.get(7),
        published_at: row.get(8),
        created_by: row.get(9),
        updated_by: row.get(10),
        created_at: row.get(11),
        updated_at: row.get(12),
    })
    .collect::<Vec<_>>();

    Ok(rows)
}

fn load_categories(
    conn: &mut PgClient,
) -> Result<HashMap<String, ArticleCategoryRecord>, ApiError> {
    let rows = conn
        .query(
            "SELECT id, name, slug, description, is_enabled, created_at::text, updated_at::text FROM article_categories",
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|row| ArticleCategoryRecord {
            id: row.get(0),
            name: row.get(1),
            slug: row.get(2),
            description: row.get(3),
            is_enabled: row.get(4),
            created_at: row.get(5),
            updated_at: row.get(6),
        })
        .collect::<Vec<_>>();

    Ok(rows
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect())
}

fn load_tags(conn: &mut PgClient) -> Result<HashMap<String, ArticleTagRecord>, ApiError> {
    let rows = conn
        .query(
            "SELECT id, name, slug, created_at::text, updated_at::text FROM article_tags",
            &[],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|row| ArticleTagRecord {
            id: row.get(0),
            name: row.get(1),
            slug: row.get(2),
            created_at: row.get(3),
            updated_at: row.get(4),
        })
        .collect::<Vec<_>>();

    Ok(rows
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect())
}

fn load_relation_links(
    conn: &mut PgClient,
    table: &str,
    relation_column: &str,
) -> Result<HashMap<String, Vec<String>>, ApiError> {
    let sql = format!(
        "SELECT article_id, {} FROM {} ORDER BY created_at ASC",
        relation_column, table
    );
    let rows = conn
        .query(&sql, &[])
        .map_err(db_error)?
        .into_iter()
        .map(|row| (row.get::<usize, String>(0), row.get::<usize, String>(1)))
        .collect::<Vec<_>>();

    let mut map = HashMap::<String, Vec<String>>::new();
    for (article_id, relation_id) in rows {
        map.entry(article_id).or_default().push(relation_id);
    }
    Ok(map)
}

fn build_article_summary(
    conn: &mut PgClient,
    article: ArticleRow,
) -> Result<ArticleSummaryItem, ApiError> {
    let categories = load_categories(conn)?;
    let tags = load_tags(conn)?;
    let category_links = load_relation_links(conn, "article_category_links", "category_id")?;
    let tag_links = load_relation_links(conn, "article_tag_links", "tag_id")?;

    Ok(ArticleSummaryItem {
        id: article.id.clone(),
        title: article.title,
        slug: article.slug.clone(),
        excerpt: article.excerpt,
        cover_image_url: article.cover_image_url,
        status: article.status,
        allow_comment: article.allow_comment,
        published_at: article.published_at,
        created_at: article.created_at,
        updated_at: article.updated_at,
        categories: category_links
            .get(&article.id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| categories.get(&id))
            .map(|item| ArticleTaxonomyItem {
                id: item.id.clone(),
                name: item.name.clone(),
                slug: item.slug.clone(),
            })
            .collect(),
        tags: tag_links
            .get(&article.id)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| tags.get(&id))
            .map(|item| ArticleTaxonomyItem {
                id: item.id.clone(),
                name: item.name.clone(),
                slug: item.slug.clone(),
            })
            .collect(),
    })
}

fn build_article_detail(
    conn: &mut PgClient,
    article: ArticleRow,
) -> Result<ArticleDetailItem, ApiError> {
    let summary = build_article_summary(conn, article.clone())?;

    Ok(ArticleDetailItem {
        summary,
        content: article.content,
        created_by: article.created_by,
        updated_by: article.updated_by,
    })
}

fn load_public_comments(
    conn: &mut PgClient,
    article_id: &str,
) -> Result<Vec<PublicCommentItem>, ApiError> {
    let rows = conn
        .query(
            "SELECT id, article_id, parent_id, nickname, email, content, status, created_at::text
             FROM comments
             WHERE article_id = $1 AND status = 'approved'
             ORDER BY created_at ASC",
            &[&article_id],
        )
        .map_err(db_error)?
        .into_iter()
        .map(|row| CommentRecord {
            id: row.get(0),
            article_id: row.get(1),
            parent_id: row.get(2),
            nickname: row.get(3),
            email: row.get(4),
            content: row.get(5),
            status: row.get(6),
            created_at: row.get(7),
        })
        .collect::<Vec<_>>();

    let mut mapped = HashMap::<String, PublicCommentItem>::new();
    for comment in &rows {
        let _ = (&comment.article_id, &comment.status);
        mapped.insert(
            comment.id.clone(),
            PublicCommentItem {
                id: comment.id.clone(),
                parent_id: comment.parent_id.clone(),
                nickname: comment.nickname.clone(),
                avatar_url: cravatar_url(&comment.email),
                content: comment.content.clone(),
                created_at: comment.created_at.clone(),
                replies: Vec::new(),
            },
        );
    }

    let mut roots = Vec::new();
    for comment in rows {
        if let Some(current) = mapped.get(&comment.id).cloned() {
            if let Some(parent_id) = comment.parent_id {
                if let Some(parent) = mapped.get_mut(&parent_id) {
                    parent.replies.push(current);
                }
            } else {
                roots.push(current);
            }
        }
    }

    Ok(roots)
}

fn build_activity_stats(conn: &mut PgClient) -> Result<ContributionData, ApiError> {
    let today = Utc::now().date_naive();
    let one_year_ago = today - Duration::days(365);
    let weekday_offset = one_year_ago.weekday().num_days_from_sunday() as i64;
    let start = one_year_ago - Duration::days(weekday_offset);

    let mut activity_map = HashMap::<NaiveDate, i64>::new();
    let mut cursor = start;
    while cursor <= today {
        activity_map.insert(cursor, 0);
        cursor += Duration::days(1);
    }

    for table in [
        "articles",
        "comments",
        "media_assets",
        "friend_link_applications",
        "page_views",
    ] {
        if !table_has_created_at(conn, table)? {
            continue;
        }

        let sql = format!(
            "SELECT created_at::date::text AS date, COUNT(*) AS count
             FROM {}
             WHERE created_at::date >= DATE '{}'
             GROUP BY created_at::date",
            table,
            start.format("%Y-%m-%d")
        );
        let rows = conn
            .query(&sql, &[])
            .map_err(db_error)?
            .into_iter()
            .map(|row| {
                let date: String = row.get(0);
                let count: i64 = row.get(1);
                (date, count)
            })
            .collect::<Vec<_>>();

        for (date, count) in rows {
            if let Ok(parsed) = NaiveDate::parse_from_str(&date, "%Y-%m-%d") {
                *activity_map.entry(parsed).or_insert(0) += count;
            }
        }
    }

    let mut weeks = Vec::new();
    let mut current_week = Vec::new();
    let mut running = start;

    while running <= today {
        current_week.push(ContributionDay {
            date: running.to_string(),
            contribution_count: *activity_map.get(&running).unwrap_or(&0),
        });

        if current_week.len() == 7 {
            weeks.push(ContributionWeek {
                contribution_days: current_week,
            });
            current_week = Vec::new();
        }

        running += Duration::days(1);
    }

    if !current_week.is_empty() {
        weeks.push(ContributionWeek {
            contribution_days: current_week,
        });
    }

    Ok(ContributionData {
        weeks,
        total_contributions: activity_map.values().sum(),
    })
}

fn table_has_created_at(conn: &mut PgClient, table: &str) -> Result<bool, ApiError> {
    let sql = format!(
        "SELECT EXISTS (
            SELECT 1
            FROM information_schema.columns
            WHERE table_schema = 'public' AND table_name = '{}' AND column_name = 'created_at'
        )",
        table
    );

    let row = conn
        .query_one(&sql, &[])
        .map_err(db_error)?;

    Ok(row.get(0))
}

async fn get_robots(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
        let mut conn = open_connection(&state)?;
        let settings = read_public_site_settings(&mut conn, &state.config.public_site_url)?;
        let body = format!(
            "User-agent: *\nAllow: /\n\nSitemap: {}/sitemap.xml\n\nDisallow: /admin/\nDisallow: /api/v1/admin/",
            settings.seo.canonical_url
        );

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain; charset=utf-8")
            .body(Body::from(body))
            .unwrap())
    })
    .await
}

async fn get_sitemap(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    run_blocking(move || {
    let mut conn = open_connection(&state)?;
    let settings = read_public_site_settings(&mut conn, &state.config.public_site_url)?;
    let base_url = settings.seo.canonical_url;

    let articles = list_public_articles_impl(
        &mut conn,
        ArticleListQuery {
            page: 1,
            page_size: 10_000,
            keyword: None,
            category_slug: None,
            tag_slug: None,
            sort_by: "publishedAt".to_string(),
            sort_order: "desc".to_string(),
        },
    )?;

    let categories = load_categories(&mut conn)?
        .into_values()
        .filter(|item| item.is_enabled)
        .collect::<Vec<_>>();
    let pages = read_enabled_standalone_pages(&mut conn)?;

    let mut urls = vec![
        (format!("{}/", base_url), None, "daily", "1.0"),
        (format!("{}/articles", base_url), None, "daily", "0.9"),
        (format!("{}/archive", base_url), None, "weekly", "0.7"),
        (format!("{}/about", base_url), None, "monthly", "0.7"),
        (format!("{}/links", base_url), None, "weekly", "0.7"),
    ];

    for category in categories {
        urls.push((
            format!("{}/categories/{}", base_url, category.slug),
            None,
            "weekly",
            "0.6",
        ));
    }

    for page in pages {
        urls.push((
            format!("{}/pages/{}", base_url, page.slug),
            None,
            "monthly",
            "0.6",
        ));
    }

    for article in articles.list {
        urls.push((
            format!("{}/articles/{}", base_url, article.slug),
            Some(
                article
                    .updated_at
                    .split('T')
                    .next()
                    .unwrap_or("")
                    .to_string(),
            ),
            "weekly",
            "0.8",
        ));
    }

    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n{}\n</urlset>",
        urls.into_iter()
            .map(|(loc, lastmod, changefreq, priority)| {
                format!(
                    "  <url>\n    <loc>{}</loc>\n{}    <changefreq>{}</changefreq>\n    <priority>{}</priority>\n  </url>",
                    escape_xml(&loc),
                    lastmod
                        .map(|value| format!("    <lastmod>{}</lastmod>\n", value))
                        .unwrap_or_default(),
                    changefreq,
                    priority
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    );

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/xml; charset=utf-8")
        .body(Body::from(body))
        .unwrap())
    })
    .await
}
