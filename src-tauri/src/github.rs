//! GitHub access through its REST API. The token lives in the OS credential
//! store (Windows Credential Manager / macOS Keychain), never in a file.
//!
//! Tokens come in two kinds:
//! - long-lived: a personal access token, or a device-flow token from an
//!   OAuth App without token expiration;
//! - short-lived: a device-flow token that expires after ~8 hours and comes
//!   with a refresh token (~6 months). GitHub issues these when the OAuth App
//!   has token expiration turned on.
//!
//! Hangar doesn't choose: it reads which kind GitHub returned and, for
//! short-lived ones, renews them automatically before they expire.

use crate::error::{msg, AppError, AppResult};
use chrono::{DateTime, Duration, Utc};
use reqwest::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub const SERVICE: &str = "io.hangar.desktop";
const ACCOUNT: &str = "github-token";
const API: &str = "https://api.github.com";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// `repo` is needed to clone private repos; `read:org` to list org memberships.
const SCOPES: &str = "repo read:org";
/// Renew this long before the access token actually expires.
const RENEW_MARGIN_MINUTES: i64 = 5;
const SIGNED_OUT: &str = "Your GitHub sign-in has ended. Sign in again in Settings → GitHub.";

pub fn client() -> Client {
    Client::builder().user_agent("Hangar-Desktop").build().expect("HTTP client")
}

/* ---------- stored token ---------- */

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StoredToken {
    pub access_token: String,
    /// The OAuth App that issued it (device flow only); needed to renew.
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub refresh_expires_at: Option<DateTime<Utc>>,
}

impl StoredToken {
    fn plain(token: &str) -> Self {
        StoredToken { access_token: token.to_string(), client_id: None, refresh_token: None, expires_at: None, refresh_expires_at: None }
    }

    /// Parse a token-endpoint response (sign-in or renewal).
    fn from_response(v: &serde_json::Value, client_id: Option<&str>, now: DateTime<Utc>) -> Option<Self> {
        let access = v.get("access_token")?.as_str()?;
        let secs = |k: &str| v.get(k).and_then(|x| x.as_i64()).map(|s| now + Duration::seconds(s));
        Some(StoredToken {
            access_token: access.to_string(),
            client_id: client_id.map(String::from),
            refresh_token: v.get("refresh_token").and_then(|r| r.as_str()).map(String::from),
            expires_at: secs("expires_in"),
            refresh_expires_at: secs("refresh_token_expires_in"),
        })
    }

    /// Older Hangar builds stored the bare token string.
    fn parse(raw: &str) -> Self {
        if raw.trim_start().starts_with('{') {
            if let Ok(t) = serde_json::from_str(raw) {
                return t;
            }
        }
        StoredToken::plain(raw)
    }

    fn needs_renewal(&self, now: DateTime<Utc>) -> bool {
        self.expires_at.is_some_and(|e| e - Duration::minutes(RENEW_MARGIN_MINUTES) <= now)
    }

    fn can_renew(&self, now: DateTime<Utc>) -> bool {
        self.client_id.is_some()
            && self.refresh_token.is_some()
            && self.refresh_expires_at.is_none_or(|e| e > now)
    }
}

fn entry() -> AppResult<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, ACCOUNT)?)
}

fn load_stored() -> AppResult<Option<StoredToken>> {
    match entry()?.get_password() {
        Ok(raw) => Ok(Some(StoredToken::parse(&raw))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn save_stored(t: &StoredToken) -> AppResult<()> {
    entry()?.set_password(&serde_json::to_string(t)?)?;
    Ok(())
}

pub fn clear_token() -> AppResult<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

enum RenewError {
    /// GitHub refused the refresh token: the user must sign in again.
    Rejected,
    Other(AppError),
}

async fn renew(client: &Client, st: &StoredToken) -> Result<StoredToken, RenewError> {
    let (Some(client_id), Some(refresh)) = (st.client_id.as_deref(), st.refresh_token.as_deref()) else {
        return Err(RenewError::Rejected);
    };
    // Device-flow tokens are renewed without a client secret.
    let resp = client
        .post(TOKEN_URL)
        .header("Accept", "application/json")
        .form(&[("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", refresh)])
        .send()
        .await
        .map_err(|e| RenewError::Other(e.into()))?;
    let v: serde_json::Value = resp.json().await.map_err(|e| RenewError::Other(e.into()))?;
    if let Some(next) = StoredToken::from_response(&v, Some(client_id), Utc::now()) {
        return Ok(next);
    }
    match v.get("error").and_then(|e| e.as_str()) {
        Some("bad_refresh_token") | Some("incorrect_client_credentials") | Some("unauthorized_client") => Err(RenewError::Rejected),
        Some(other) => Err(RenewError::Other(msg(format!("Couldn't renew the GitHub sign-in: {other}")))),
        None => Err(RenewError::Other(msg("Couldn't renew the GitHub sign-in: unexpected response from GitHub."))),
    }
}

/// The stored token, renewed first if it is about to expire.
///
/// The lock makes sure only one renewal runs at a time: a refresh token works
/// once, so two parallel renewals would sign the user out.
pub async fn valid_token(client: &Client, lock: &Mutex<()>) -> AppResult<Option<StoredToken>> {
    let _guard = lock.lock().await;
    let Some(st) = load_stored()? else { return Ok(None) };
    let now = Utc::now();
    if !st.needs_renewal(now) {
        return Ok(Some(st));
    }
    if !st.can_renew(now) {
        clear_token()?;
        return Err(msg(SIGNED_OUT));
    }
    match renew(client, &st).await {
        Ok(next) => {
            save_stored(&next)?;
            Ok(Some(next))
        }
        Err(RenewError::Rejected) => {
            clear_token()?;
            Err(msg(SIGNED_OUT))
        }
        Err(RenewError::Other(e)) => Err(e),
    }
}

/* ---------- account ---------- */

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    /// "device" (signed in through the browser) or "pat" (personal access token).
    pub kind: &'static str,
    /// When the current access token expires; `None` = doesn't expire.
    pub expires_at: Option<DateTime<Utc>>,
    /// Hangar renews it automatically.
    pub renews: bool,
    /// After this date without use, the user has to sign in again.
    pub renew_until: Option<DateTime<Utc>>,
    /// Expiry reported by GitHub for a personal access token, if it has one.
    pub pat_expires: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub orgs: Vec<String>,
    pub scopes: Option<String>,
    pub token: TokenInfo,
}

#[derive(Deserialize)]
struct UserResp {
    login: String,
    name: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct OrgResp {
    login: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GhRepo {
    pub full_name: String,
    pub owner: String,
    pub name: String,
    pub private: bool,
    pub fork: bool,
    pub archived: bool,
    pub description: Option<String>,
    pub pushed_at: Option<String>,
    pub html_url: String,
    pub stars: u64,
    pub language: Option<String>,
}

#[derive(Deserialize)]
struct OwnerResp {
    login: String,
}

#[derive(Deserialize)]
struct RepoResp {
    full_name: String,
    name: String,
    owner: OwnerResp,
    private: bool,
    fork: bool,
    #[serde(default)]
    archived: bool,
    description: Option<String>,
    pushed_at: Option<String>,
    html_url: String,
    #[serde(default)]
    stargazers_count: u64,
    #[serde(default)]
    language: Option<String>,
}

impl From<RepoResp> for GhRepo {
    fn from(r: RepoResp) -> Self {
        GhRepo {
            full_name: r.full_name,
            owner: r.owner.login,
            name: r.name,
            private: r.private,
            fork: r.fork,
            archived: r.archived,
            description: r.description,
            pushed_at: r.pushed_at,
            html_url: r.html_url,
            stars: r.stargazers_count,
            language: r.language,
        }
    }
}

#[derive(Deserialize)]
struct SearchResp {
    items: Vec<RepoResp>,
}

/// Search GitHub repos by name, description or topic, most-starred first
/// (GitHub's default "best match" ranks copies and forks above the original).
pub async fn search_repos(client: &Client, token: Option<&str>, query: &str) -> AppResult<Vec<GhRepo>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let params = [("q", q), ("sort", "stars"), ("order", "desc"), ("per_page", "25")];
    let url = reqwest::Url::parse_with_params(&format!("{API}/search/repositories"), &params)
        .map_err(|e| msg(format!("Bad search: {e}")))?;
    let mut rb = client.get(url).header("Accept", "application/vnd.github+json").header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = token {
        rb = rb.bearer_auth(t);
    }
    let resp: SearchResp = check(rb.send().await?).await?.json().await?;
    let mut out: Vec<GhRepo> = resp.items.into_iter().map(GhRepo::from).collect();
    rank(&mut out, q);
    Ok(out)
}

/// Exact name matches first, then names containing every search word, then the
/// rest; most stars first within each group.
fn rank(repos: &mut [GhRepo], query: &str) {
    let q = query.trim().to_lowercase();
    let dashed = q.split_whitespace().collect::<Vec<_>>().join("-");
    let words: Vec<&str> = q.split(|c: char| c.is_whitespace() || c == '/').filter(|w| !w.is_empty()).collect();
    let tier = |r: &GhRepo| {
        let name = r.name.to_lowercase();
        let full = r.full_name.to_lowercase();
        if name == q || name == dashed || full == q {
            0
        } else if words.iter().all(|w| full.contains(w)) {
            1
        } else {
            2
        }
    };
    repos.sort_by(|a, b| tier(a).cmp(&tier(b)).then(b.stars.cmp(&a.stars)));
}

fn get(client: &Client, token: Option<&str>, path: &str) -> RequestBuilder {
    let mut rb = client
        .get(format!("{API}{path}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = token {
        rb = rb.bearer_auth(t);
    }
    rb
}

async fn check(resp: reqwest::Response) -> AppResult<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from))
        .unwrap_or(body);
    Err(match status.as_u16() {
        401 => msg("GitHub sign-in expired or was revoked. Sign in again in Settings."),
        403 if message.to_lowercase().contains("rate limit") => {
            msg("GitHub API rate limit reached. Signing in raises the limit; otherwise wait a few minutes.")
        }
        404 => msg("Not found on GitHub. If it's private, check that your account has access."),
        code => msg(format!("GitHub error {code}: {message}")),
    })
}

pub async fn account(client: &Client, st: &StoredToken) -> AppResult<Account> {
    let token = st.access_token.as_str();
    let resp = check(get(client, Some(token), "/user").send().await?).await?;
    let header = |k: &str| resp.headers().get(k).and_then(|v| v.to_str().ok()).map(String::from);
    let scopes = header("x-oauth-scopes");
    let pat_expires = header("github-authentication-token-expiration");
    let u: UserResp = resp.json().await?;
    let orgs: Vec<OrgResp> = match get(client, Some(token), "/user/orgs?per_page=100").send().await {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
        _ => Vec::new(),
    };
    let now = Utc::now();
    Ok(Account {
        login: u.login,
        name: u.name,
        avatar_url: u.avatar_url,
        orgs: orgs.into_iter().map(|o| o.login).collect(),
        scopes,
        token: TokenInfo {
            kind: if st.client_id.is_some() { "device" } else { "pat" },
            expires_at: st.expires_at,
            renews: st.expires_at.is_some() && st.can_renew(now),
            renew_until: st.refresh_expires_at,
            pat_expires,
        },
    })
}

/// Every repo you own, collaborate on, or can see through an org membership.
pub async fn list_repos(client: &Client, token: &str) -> AppResult<Vec<GhRepo>> {
    let mut out = Vec::new();
    for page in 1..=30 {
        let path = format!(
            "/user/repos?per_page=100&page={page}&sort=pushed&affiliation=owner,collaborator,organization_member"
        );
        let batch: Vec<RepoResp> = check(get(client, Some(token), &path).send().await?).await?.json().await?;
        let n = batch.len();
        out.extend(batch.into_iter().map(GhRepo::from));
        if n < 100 {
            break;
        }
    }
    Ok(out)
}

#[derive(Deserialize)]
struct BranchResp {
    name: String,
}

/// Branch names of a repo (up to 500), for picking one to clone.
pub async fn list_branches(client: &Client, token: Option<&str>, owner: &str, repo: &str) -> AppResult<Vec<String>> {
    let mut out = Vec::new();
    for page in 1..=5 {
        let path = format!("/repos/{owner}/{repo}/branches?per_page=100&page={page}");
        let batch: Vec<BranchResp> = check(get(client, token, &path).send().await?).await?.json().await?;
        let n = batch.len();
        out.extend(batch.into_iter().map(|b| b.name));
        if n < 100 {
            break;
        }
    }
    Ok(out)
}

/// Look up one repo. `Ok(None)` means GitHub says it doesn't exist (or you can't see it).
pub async fn get_repo(client: &Client, token: Option<&str>, owner: &str, repo: &str) -> AppResult<Option<GhRepo>> {
    let resp = get(client, token, &format!("/repos/{owner}/{repo}")).send().await?;
    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    let r: RepoResp = check(resp).await?.json().await?;
    Ok(Some(r.into()))
}

/* ---------- sign-in ---------- */

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Serialize, Debug)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PollResult {
    Pending,
    SlowDown { interval: u64 },
    Done { account: Account },
    Expired,
    Denied,
}

/// Start device-code sign-in.
pub async fn start_device(client: &Client, client_id: &str) -> AppResult<DeviceCode> {
    let resp = client
        .post("https://github.com/login/device/code")
        .header("Accept", "application/json")
        .form(&[("client_id", client_id), ("scope", SCOPES)])
        .send()
        .await?;
    let status = resp.status();
    let v: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(msg(match err {
            "device_flow_disabled" => {
                "Device flow is off for this OAuth App. Turn on \"Enable Device Flow\" in the app's settings on GitHub.".to_string()
            }
            "unauthorized_client" | "incorrect_client_credentials" => "GitHub doesn't recognise this Client ID.".to_string(),
            other => format!("GitHub: {other}"),
        }));
    }
    if !status.is_success() {
        return Err(msg(format!("GitHub error {status} while starting sign-in.")));
    }
    Ok(serde_json::from_value(v)?)
}

pub async fn poll_device(client: &Client, client_id: &str, device_code: &str) -> AppResult<PollResult> {
    let v: serde_json::Value = client
        .post(TOKEN_URL)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await?
        .json()
        .await?;
    if let Some(st) = StoredToken::from_response(&v, Some(client_id), Utc::now()) {
        let acct = account(client, &st).await?;
        save_stored(&st)?;
        return Ok(PollResult::Done { account: acct });
    }
    match v.get("error").and_then(|e| e.as_str()) {
        Some("authorization_pending") => Ok(PollResult::Pending),
        Some("slow_down") => Ok(PollResult::SlowDown { interval: v.get("interval").and_then(|i| i.as_u64()).unwrap_or(10) }),
        // The docs use both spellings.
        Some("expired_token") | Some("token_expired") => Ok(PollResult::Expired),
        Some("access_denied") => Ok(PollResult::Denied),
        Some(other) => Err(msg(format!("GitHub: {other}"))),
        None => Err(msg("Unexpected response from GitHub.")),
    }
}

/// Sign in with a personal access token pasted by the user.
pub async fn sign_in_with_token(client: &Client, token: &str) -> AppResult<Account> {
    let t = token.trim();
    if t.is_empty() {
        return Err(msg("Paste a token first."));
    }
    let st = StoredToken::plain(t);
    let a = account(client, &st).await.map_err(|e| {
        if e.to_string().contains("expired or was revoked") {
            msg("GitHub didn't accept this token.")
        } else {
            e
        }
    })?;
    save_stored(&st)?;
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn t0() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-26T12:00:00Z").unwrap().with_timezone(&Utc)
    }

    #[test]
    fn parses_expiring_response() {
        let v = json!({
            "access_token": "ghu_a", "expires_in": 28800,
            "refresh_token": "ghr_b", "refresh_token_expires_in": 15897600,
            "token_type": "bearer", "scope": "repo"
        });
        let st = StoredToken::from_response(&v, Some("cid"), t0()).unwrap();
        assert_eq!(st.access_token, "ghu_a");
        assert_eq!(st.refresh_token.as_deref(), Some("ghr_b"));
        assert_eq!(st.expires_at, Some(t0() + Duration::hours(8)));
        assert_eq!(st.refresh_expires_at, Some(t0() + Duration::seconds(15897600)));
        assert!(!st.needs_renewal(t0()));
        assert!(st.needs_renewal(t0() + Duration::hours(8) - Duration::minutes(4)));
        assert!(st.can_renew(t0() + Duration::days(30)));
        assert!(!st.can_renew(t0() + Duration::days(200)));
    }

    #[test]
    fn non_expiring_response_never_needs_renewal() {
        let v = json!({ "access_token": "gho_x", "token_type": "bearer", "scope": "repo" });
        let st = StoredToken::from_response(&v, Some("cid"), t0()).unwrap();
        assert!(st.expires_at.is_none() && st.refresh_token.is_none());
        assert!(!st.needs_renewal(t0() + Duration::days(3650)));
        assert!(StoredToken::from_response(&json!({ "error": "authorization_pending" }), None, t0()).is_none());
    }

    #[test]
    fn reads_old_plain_tokens_and_roundtrips_json() {
        assert_eq!(StoredToken::parse("ghp_legacy"), StoredToken::plain("ghp_legacy"));
        let v = json!({ "access_token": "ghu_a", "expires_in": 60, "refresh_token": "ghr_b", "refresh_token_expires_in": 120 });
        let st = StoredToken::from_response(&v, Some("cid"), t0()).unwrap();
        let raw = serde_json::to_string(&st).unwrap();
        assert_eq!(StoredToken::parse(&raw), st);
    }

    fn repo(full: &str, stars: u64) -> GhRepo {
        let (owner, name) = full.split_once('/').unwrap();
        GhRepo {
            full_name: full.into(),
            owner: owner.into(),
            name: name.into(),
            private: false,
            fork: false,
            archived: false,
            description: None,
            pushed_at: None,
            html_url: String::new(),
            stars,
            language: None,
        }
    }

    #[test]
    fn ranks_name_matches_above_popular_mentions() {
        let mut v = vec![repo("clash/clash-verge", 90000), repo("someone/tauri", 3), repo("tauri-apps/tauri", 80000), repo("x/tauri-plugin", 500)];
        rank(&mut v, "tauri");
        let names: Vec<&str> = v.iter().map(|r| r.full_name.as_str()).collect();
        assert_eq!(names, ["tauri-apps/tauri", "someone/tauri", "x/tauri-plugin", "clash/clash-verge"]);
        let mut v = vec![repo("a/react-three-fiber-demo", 5), repo("pmndrs/react-three-fiber", 27000), repo("b/three", 100)];
        rank(&mut v, "react three fiber");
        assert_eq!(v[0].full_name, "pmndrs/react-three-fiber");
    }

    /// Real GitHub API calls, unauthenticated: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn searches_repos_and_lists_branches_on_github() {
        let c = client();
        let found = tauri::async_runtime::block_on(search_repos(&c, None, "tauri")).unwrap();
        let names: Vec<&str> = found.iter().map(|r| r.full_name.as_str()).collect();
        assert_eq!(names.first(), Some(&"tauri-apps/tauri"), "{names:?}");
        let branches = tauri::async_runtime::block_on(list_branches(&c, None, "octocat", "Hello-World")).unwrap();
        assert!(branches.iter().any(|b| b == "master"), "{branches:?}");
    }

    #[test]
    fn personal_tokens_cannot_renew() {
        let st = StoredToken::plain("ghp_x");
        assert!(!st.can_renew(t0()));
        assert!(!st.needs_renewal(t0()));
    }
}
