//! Client for Jira Cloud (REST v3): the connection and the one way out for an outward call,
//! [`JiraClient::execute`]. The reading calls are in `read`.

use cockpit_i18n::msg;

use serde::de::DeserializeOwned;

use super::dto::{Myself, TransitionList};
use crate::http::{base_url, client, send};
use crate::outbound::{
    Outbound, Request, choose_transition, jira_comment_request, transition_request,
};
use crate::{Error, Secret};
use cockpit_core::TicketStatus;

mod read;

pub use read::{MAX_ISSUES, SearchPage, find_team_field};

pub struct JiraClient {
    http: reqwest::Client,
    base: String,
    email: String,
    token: Secret,
    /// With OAuth: `Authorization: Bearer`, otherwise Basic with email and API token.
    oauth: bool,
    /// The sprint field of the site, asked once per client (`None` inside: the site has none).
    sprint_field: std::sync::Mutex<Option<Option<String>>>,
    /// The team field set in the configuration (`[jira] team_field`); it replaces the detection.
    team_override: Option<String>,
    /// The team field of the site, asked once per client (`None` inside: the site has none).
    team_field: std::sync::Mutex<Option<Option<String>>>,
    /// The address of the site people open in the browser (`https://acme.atlassian.net`), for
    /// links to tickets. `None` = not known.
    site: Option<String>,
}

/// A downloaded attachment.
#[derive(Debug, Clone)]
pub struct Download {
    pub bytes: Vec<u8>,
    /// The `Content-Type` Jira sent, if any.
    pub content_type: Option<String>,
}

impl std::fmt::Debug for JiraClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JiraClient")
            .field("basis", &self.base)
            .field("email", &self.email)
            .field("token", &self.token)
            .finish()
    }
}

impl JiraClient {
    /// `site_url` is e.g. `https://firma.atlassian.net`; sign-in with email and API token.
    pub fn new(site_url: &str, email: &str, token: Secret) -> Result<Self, Error> {
        Ok(Self {
            http: client()?,
            base: base_url(site_url)?,
            email: email.to_string(),
            token,
            oauth: false,
            sprint_field: Default::default(),
            team_override: None,
            team_field: Default::default(),
            site: crate::http::base_url(site_url).ok(),
        })
    }

    /// Sign-in via OAuth: `base` is `https://api.atlassian.com/ex/jira/<cloudId>`
    /// ([`crate::oauth::jira_base`]), `access` the access token.
    pub fn oauth(base: &str, access: Secret) -> Result<Self, Error> {
        Ok(Self {
            http: client()?,
            base: base_url(base)?,
            email: String::new(),
            token: access,
            oauth: true,
            sprint_field: Default::default(),
            team_override: None,
            team_field: Default::default(),
            site: None,
        })
    }

    /// Sets the ID of the team field (`customfield_10400`) for sites where the detection fails.
    /// Empty or `None` = detect it.
    pub fn with_team_field(mut self, field: Option<&str>) -> Self {
        self.team_override = field
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(str::to_string);
        self
    }

    /// Tells the client the address of the site people open in the browser (with OAuth, the
    /// base is the gateway, which no one opens). Without an `http(s)` address no links exist.
    pub fn with_site(mut self, site: Option<&str>) -> Self {
        self.site = site.and_then(|s| crate::http::base_url(s).ok());
        self
    }

    /// The page of a ticket in Jira (`<site>/browse/<KEY>`); `None` if the site is not known.
    pub fn browse_url(&self, key: &str) -> Option<String> {
        cockpit_core::details::browse_url(self.site.as_deref()?, key)
    }

    fn authed(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.oauth {
            req.bearer_auth(self.token.expose())
        } else {
            req.basic_auth(&self.email, Some(self.token.expose()))
        }
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let req = self.http.request(method, format!("{}{path}", self.base));
        self.authed(req).header("Accept", "application/json")
    }

    /// Reads an attachment (`thumbnail`: its small picture). Read only. `id` must be digits,
    /// since it goes into the address. At most `max` bytes: a larger one is [`Error::TooLarge`].
    /// The key is only sent to the site itself – a redirect to the media service (which
    /// carries its own token in the address) goes without it.
    pub async fn attachment(
        &self,
        id: &str,
        thumbnail: bool,
        max: usize,
    ) -> Result<Download, Error> {
        if id.is_empty() || !id.bytes().all(|c| c.is_ascii_digit()) {
            return Err(Error::NotFound);
        }
        let kind = if thumbnail { "thumbnail" } else { "content" };
        let req = self
            .http
            .get(format!("{}/rest/api/3/attachment/{kind}/{id}", self.base))
            .header("Accept", "*/*");
        let mut resp = self.authed(req).send().await.map_err(|e| {
            Error::Network(e.without_url().to_string()).without_key(self.token.expose())
        })?;
        let status = resp.status();
        if !status.is_success() {
            let headers = resp.headers().clone();
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::http::error_from(status, &headers, &body, 0)
                .without_key(self.token.expose()));
        }
        if resp.content_length().is_some_and(|n| n > max as u64) {
            return Err(Error::TooLarge);
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let mut bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| Error::Network(e.without_url().to_string()))?
        {
            if bytes.len() + chunk.len() > max {
                return Err(Error::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Download {
            bytes,
            content_type,
        })
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.request(reqwest::Method::GET, path)
    }

    /// Sends the request; a key that the service echoes back in a message
    /// is made unrecognizable.
    async fn text(&self, req: reqwest::RequestBuilder) -> Result<String, Error> {
        send(req)
            .await
            .map_err(|e| e.without_key(self.token.expose()))
    }

    async fn read_json<T: DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, Error> {
        let text = self.text(req).await?;
        serde_json::from_str(&text).map_err(|e| Error::Response(e.to_string()))
    }

    async fn send(&self, a: &Request) -> Result<String, Error> {
        let method = reqwest::Method::from_bytes(a.method.as_bytes())
            .map_err(|e| Error::Config(e.to_string()))?;
        let mut req = self.request(method, &a.path);
        if let Some(body) = &a.body {
            req = req.json(body);
        }
        self.text(req).await
    }

    /// `accountId` of the key holder.
    pub async fn me(&self) -> Result<String, Error> {
        Ok(self
            .read_json::<Myself>(self.get("/rest/api/3/myself"))
            .await?
            .account_id)
    }

    /// The display name of the key holder (empty if Jira names none). Read only; serves the
    /// connection test in the setup assistant.
    pub async fn my_name(&self) -> Result<String, Error> {
        Ok(self
            .read_json::<Myself>(self.get("/rest/api/3/myself"))
            .await?
            .display_name
            .unwrap_or_default())
    }

    /// Executes an outward call. Jira variants only. Every variant is named, so a new one does
    /// not compile until it says how it is sent.
    pub async fn execute(&self, a: &Outbound) -> Result<(), Error> {
        match a {
            Outbound::JiraComment { key, text } => self
                .send(&jira_comment_request(key, text))
                .await
                .map(|_| ()),
            Outbound::JiraStatus { key, status } => self.status_set(key, *status).await,
            Outbound::GitHubComment { .. }
            | Outbound::GitHubBranch { .. }
            | Outbound::GitHubReview { .. }
            | Outbound::GitHubMerge { .. }
            | Outbound::GitHubRecheck { .. }
            | Outbound::ProjectField { .. }
            | Outbound::DraftEdit { .. }
            | Outbound::Issue { .. } => Err(Error::Config("not a Jira call".into())),
        }
    }

    /// Finds the transition that leads to the target status and executes it.
    pub async fn status_set(&self, key: &str, target: TicketStatus) -> Result<(), Error> {
        let list: TransitionList = self
            .read_json(self.get(&format!("/rest/api/3/issue/{key}/transitions")))
            .await?;
        let transition = choose_transition(&list.transitions, target).ok_or_else(|| {
            Error::Coded(msg!(
                IntegrationsJiraNoTransition,
                key = key,
                name = target.name_msg()
            ))
        })?;
        self.send(&transition_request(key, &transition.id))
            .await
            .map(|_| ())
    }
}
