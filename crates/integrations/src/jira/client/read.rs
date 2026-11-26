//! Reading calls of the Jira client: tickets, the fields of the site, projects, people and
//! comments. Read only.

use cockpit_core::Item;
use cockpit_i18n::msg;

use super::super::dto::{
    AssignableUser, CommentPage, FieldInfo, Issue, JiraComment, Project, ProjectPage, SearchResult,
};
use super::super::jql::ASSIGNED_OR_WATCHED_JQL;
use super::super::map;
use super::JiraClient;
use crate::Error;

/// At most this many people come back from the search for assignable users.
pub const ASSIGNABLE_MAX: usize = 20;

/// This many projects per page and at most in total.
const PROJECTS_PAGE: usize = 50;
const MAX_PROJECTS: usize = 500;

/// All fields except the big ones (comments, work log, votes).
const FIELDS: &str = "*all,-comment,-worklog,-votes";

/// This many issues per page and at most in total.
const PAGE: usize = 50;
pub const MAX_ISSUES: usize = 200;

/// The team field among the fields of the site: the field of the Atlassian team type, otherwise
/// the one named „Team“.
pub fn find_team_field(fields: &[FieldInfo]) -> Option<String> {
    let is_team_type = |f: &FieldInfo| {
        f.schema
            .as_ref()
            .and_then(|s| s.custom.as_deref())
            .is_some_and(|c| c.ends_with(":atlassian-team") || c.ends_with("-field-team"))
    };
    fields
        .iter()
        .find(|f| is_team_type(f))
        .or_else(|| fields.iter().find(|f| f.name == "Team"))
        .map(|f| f.id.clone())
        .filter(|id| !id.is_empty())
}

/// The issues of a search and whether more matched than were fetched.
#[derive(Debug, Default)]
pub struct SearchPage {
    pub issues: Vec<Issue>,
    /// There were more issues than [`MAX_ISSUES`].
    pub truncated: bool,
}

impl JiraClient {
    /// Issues for a JQL query, page by page (`nextPageToken`), at most [`MAX_ISSUES`].
    pub async fn search(&self, jql: &str) -> Result<Vec<Issue>, Error> {
        self.search_with_sprint(jql, None).await
    }

    /// The ID of the sprint field of this site (`customfield_10020` and the like), from
    /// `GET /rest/api/3/field`: the field of the Jira Software sprint type, otherwise the one
    /// named „Sprint“. `None` if the site has none (no Jira Software). Asked once per client (the
    /// server builds one per sync); a failed question is not kept.
    pub async fn sprint_field(&self) -> Result<Option<String>, Error> {
        if let Some(known) = self.sprint_field.lock().ok().and_then(|k| k.clone()) {
            return Ok(known);
        }
        let fields: Vec<FieldInfo> = self.read_json(self.get("/rest/api/3/field")).await?;
        let is_sprint_type = |f: &FieldInfo| {
            f.schema
                .as_ref()
                .and_then(|s| s.custom.as_deref())
                .is_some_and(|c| c.ends_with(":gh-sprint"))
        };
        let found = fields
            .iter()
            .find(|f| is_sprint_type(f))
            .or_else(|| fields.iter().find(|f| f.name == "Sprint"))
            .map(|f| f.id.clone())
            .filter(|id| !id.is_empty());
        if let Ok(mut k) = self.sprint_field.lock() {
            *k = Some(found.clone());
        }
        Ok(found)
    }

    /// The ID of the team field of this site (the Atlassian „Team“ field, `customfield_10400` and
    /// the like): the one set in the configuration, otherwise from `GET /rest/api/3/field` the
    /// field of the team type, otherwise the one named „Team“. `None` if the site has none.
    /// Asked once per client; a failed question is not kept.
    pub async fn team_field(&self) -> Result<Option<String>, Error> {
        if let Some(f) = &self.team_override {
            return Ok(Some(f.clone()));
        }
        if let Some(known) = self.team_field.lock().ok().and_then(|k| k.clone()) {
            return Ok(known);
        }
        let fields: Vec<FieldInfo> = self.read_json(self.get("/rest/api/3/field")).await?;
        let found = find_team_field(&fields);
        if let Ok(mut k) = self.team_field.lock() {
            *k = Some(found.clone());
        }
        Ok(found)
    }

    /// Like [`JiraClient::search`], additionally asking for the sprint field.
    pub async fn search_with_sprint(
        &self,
        jql: &str,
        sprint_field: Option<&str>,
    ) -> Result<Vec<Issue>, Error> {
        Ok(self.search_page(jql, sprint_field).await?.issues)
    }

    /// Like [`JiraClient::search_with_sprint`], and says if the result was cut at
    /// [`MAX_ISSUES`] (Jira had a further page).
    pub async fn search_page(
        &self,
        jql: &str,
        sprint_field: Option<&str>,
    ) -> Result<SearchPage, Error> {
        self.search_page_fields(jql, &[sprint_field]).await
    }

    /// Like [`JiraClient::search_page`], naming further fields (sprint, team) the issues carry.
    pub async fn search_page_fields(
        &self,
        jql: &str,
        extra: &[Option<&str>],
    ) -> Result<SearchPage, Error> {
        // `*all` already holds them; naming them too keeps the question explicit.
        let mut fields = FIELDS.to_string();
        for f in extra.iter().flatten() {
            fields = format!("{fields},{f}");
        }
        let mut all = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut question = vec![
                ("jql", jql.to_string()),
                ("fields", fields.clone()),
                ("expand", "names".to_string()),
                ("maxResults", PAGE.to_string()),
            ];
            if let Some(t) = &token {
                question.push(("nextPageToken", t.clone()));
            }
            let res: SearchResult = self
                .read_json(self.get("/rest/api/3/search/jql").query(&question))
                .await?;
            let names = std::sync::Arc::new(res.names);
            all.extend(res.issues.into_iter().map(|mut i| {
                i.names = names.clone();
                i
            }));
            token = res.next_page_token.filter(|t| !t.is_empty());
            if token.is_none() || all.len() >= MAX_ISSUES {
                break;
            }
        }
        let truncated = token.is_some() || all.len() > MAX_ISSUES;
        all.truncate(MAX_ISSUES);
        Ok(SearchPage {
            issues: all,
            truncated,
        })
    }

    /// The projects the key may see, sorted by name (page by page, at most
    /// [`MAX_PROJECTS`]).
    pub async fn projects(&self) -> Result<Vec<Project>, Error> {
        let mut all = Vec::new();
        loop {
            let question = [
                ("startAt", all.len().to_string()),
                ("maxResults", PROJECTS_PAGE.to_string()),
                ("orderBy", "name".to_string()),
            ];
            let page: ProjectPage = self
                .read_json(self.get("/rest/api/3/project/search").query(&question))
                .await?;
            let empty = page.values.is_empty();
            all.extend(page.values);
            if page.is_last || empty || all.len() >= MAX_PROJECTS {
                break;
            }
        }
        all.truncate(MAX_PROJECTS);
        Ok(all)
    }

    /// People who can be assigned to tickets of a project and whose name matches `query`
    /// (`GET /rest/api/3/user/assignable/search`, read only). Inactive accounts and apps are
    /// left out; at most [`ASSIGNABLE_MAX`] come back.
    pub async fn assignable_users(
        &self,
        project: &str,
        query: &str,
    ) -> Result<Vec<AssignableUser>, Error> {
        if !cockpit_core::is_project_key(project) {
            return Err(Error::Coded(msg!(
                IntegrationsJiraBadProjectKey,
                project = project
            )));
        }
        let question = [
            ("project", project.to_string()),
            ("query", query.trim().to_string()),
            ("maxResults", ASSIGNABLE_MAX.to_string()),
        ];
        let users: Vec<AssignableUser> = self
            .read_json(
                self.get("/rest/api/3/user/assignable/search")
                    .query(&question),
            )
            .await?;
        Ok(users
            .into_iter()
            .filter(|u| {
                u.active
                    && !u.display_name.trim().is_empty()
                    && u.account_type.as_deref().is_none_or(|t| t == "atlassian")
            })
            .collect())
    }

    /// [`Self::assignable_users`] as people for the app: you come as [`cockpit_core::Person::me`],
    /// the others with initials from their name.
    pub async fn assignable_people(
        &self,
        project: &str,
        query: &str,
    ) -> Result<Vec<cockpit_core::Person>, Error> {
        let users = self.assignable_users(project, query).await?;
        // Without our own account ID everybody is „someone else“; that is no reason to fail.
        let me = self.me().await.ok();
        Ok(users
            .into_iter()
            .map(|u| {
                if me.as_deref() == Some(u.account_id.as_str()) {
                    cockpit_core::Person::me()
                } else {
                    crate::text::person(&u.display_name)
                }
            })
            .collect())
    }

    pub async fn comments(&self, key: &str) -> Result<Vec<JiraComment>, Error> {
        let page: CommentPage = self
            .read_json(
                self.get(&format!("/rest/api/3/issue/{key}/comment"))
                    .query(&[("orderBy", "created")]),
            )
            .await?;
        Ok(page.comments)
    }

    /// Inbox entries: open issues assigned to me or watched.
    pub async fn load_inbox(&self) -> Result<Vec<Item>, Error> {
        let me = self.me().await?;
        let issues = self.search(ASSIGNED_OR_WATCHED_JQL).await?;
        Ok(issues.iter().map(|i| map::issue_to_item(i, &me)).collect())
    }
}
