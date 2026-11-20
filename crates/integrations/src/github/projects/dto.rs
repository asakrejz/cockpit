//! The shapes of the GraphQL answers (documented schema of Projects v2). Every field is optional
//! or defaulted: GitHub leaves out what a credential may not see.

use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Envelope {
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub errors: Vec<GqlError>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct GqlError {
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub path: Vec<serde_json::Value>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RateLimit {
    pub cost: u32,
    pub remaining: u32,
    pub reset_at: String,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(default)]
pub struct Conn<T> {
    pub nodes: Vec<Option<T>>,
}

impl<T> Conn<T> {
    pub fn items(&self) -> impl Iterator<Item = &T> {
        self.nodes.iter().flatten()
    }
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct Login {
    pub login: String,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RepoName {
    pub name_with_owner: String,
}

// ---------- Projects list ----------

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct ViewerData {
    pub viewer: Option<Viewer>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Viewer {
    pub login: String,
    pub projects_v2: Option<Conn<ProjectNode>>,
    pub organizations: Option<Conn<OrgNode>>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct OrgNode {
    pub login: String,
    pub projects_v2: Option<Conn<ProjectNode>>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(default)]
pub struct ProjectNode {
    pub id: String,
    pub number: u32,
    pub title: String,
    pub closed: bool,
    pub url: String,
    pub owner: Option<Login>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RepoProjects {
    pub name_with_owner: String,
    pub projects_v2: Option<Conn<ProjectNode>>,
}

// ---------- One project ----------

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MetaData {
    pub repository_owner: Option<MetaOwner>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MetaOwner {
    pub project_v2: Option<ProjectFull>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct ProjectFull {
    pub id: String,
    pub number: u32,
    pub title: String,
    pub url: String,
    pub closed: bool,
    pub fields: Option<Conn<FieldNode>>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FieldNode {
    pub id: Option<String>,
    pub name: Option<String>,
    pub data_type: Option<String>,
    pub options: Option<Vec<OptionNode>>,
    pub configuration: Option<IterationConfig>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct OptionNode {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct IterationConfig {
    pub iterations: Vec<IterationNode>,
    pub completed_iterations: Vec<IterationNode>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct IterationNode {
    pub id: String,
    pub title: String,
    pub start_date: String,
    pub duration: u32,
}

// ---------- Items ----------

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct ItemsData {
    pub node: Option<ItemsNode>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct ItemsNode {
    pub items: Option<ItemsConn>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemsConn {
    pub page_info: PageInfo,
    pub nodes: Vec<Option<ItemNode>>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PageInfo {
    pub has_next_page: bool,
    pub end_cursor: Option<String>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemNode {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub updated_at: String,
    pub is_archived: bool,
    pub field_values: Option<Conn<FieldValueNode>>,
    pub content: Option<Content>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct FieldValueNode {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub name: Option<String>,
    pub option_id: Option<String>,
    pub title: Option<String>,
    pub iteration_id: Option<String>,
    pub start_date: Option<String>,
    pub duration: Option<u32>,
    pub number: Option<f64>,
    pub date: Option<String>,
    pub text: Option<String>,
    pub field: Option<FieldRef>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(default)]
pub struct FieldRef {
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct Content {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub id: String,
    pub number: Option<u32>,
    pub title: String,
    pub body: String,
    pub state: Option<String>,
    pub state_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub url: Option<String>,
    pub author: Option<Login>,
    pub creator: Option<Login>,
    pub repository: Option<RepoName>,
    pub assignees: Option<Conn<Login>>,
    pub labels: Option<Conn<LabelNode>>,
    pub milestone: Option<MilestoneNode>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(default)]
pub struct LabelNode {
    pub name: String,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct MilestoneNode {
    pub title: String,
    pub due_on: Option<String>,
}

// ---------- Details ----------

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct DetailsData {
    pub nodes: Vec<Option<DetailNode>>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct DetailNode {
    pub id: String,
    pub comments: Option<Conn<CommentNode>>,
    pub timeline_items: Option<Conn<TimelineNode>>,
    pub closed_by_pull_requests_references: Option<Conn<PrNode>>,
    pub issue_type: Option<NameNode>,
    pub parent: Option<IssueNode>,
    pub sub_issues: Option<Conn<IssueNode>>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct CommentNode {
    pub author: Option<Login>,
    pub body: String,
    pub created_at: String,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(default)]
pub struct NameNode {
    pub name: String,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct TimelineNode {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub subject: Option<PrNode>,
    pub source: Option<PrNode>,
    pub will_close_target: bool,
}

/// A pull request (or something else that can cross-reference: an issue). Only pull requests
/// carry a number here because the query asks for it inside `... on PullRequest`.
#[derive(Deserialize, Debug, Default, Clone)]
#[serde(default)]
pub struct PrNode {
    pub number: Option<u32>,
    pub repository: Option<RepoName>,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(default)]
pub struct IssueNode {
    pub number: u32,
    pub title: String,
    pub state: String,
    pub repository: Option<RepoName>,
}
