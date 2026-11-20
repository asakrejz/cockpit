//! The GraphQL documents. One place, so that a change of the schema touches one file.

/// Fields every project query asks for.
pub const PROJECT_FIELDS: &str = "id number title closed url shortDescription \
    owner { __typename ... on User { login } ... on Organization { login } }";

/// The rate limit block every query ends with.
pub const RATE: &str = "rateLimit { cost remaining resetAt }";

/// Projects of the signed-in user and of the organizations they belong to.
pub fn list_viewer() -> String {
    format!(
        "query {{ viewer {{ login \
           projectsV2(first: 100) {{ nodes {{ {PROJECT_FIELDS} }} }} \
           organizations(first: 50) {{ nodes {{ login \
             projectsV2(first: 100) {{ nodes {{ {PROJECT_FIELDS} }} }} }} }} }} {RATE} }}"
    )
}

/// Projects linked to repos (`repos`: `(owner, name)`), one alias per repo; the variables are
/// `o0, n0, o1, n1 …`.
pub fn list_repos(count: usize) -> String {
    let mut vars = String::new();
    let mut body = String::new();
    for i in 0..count {
        vars.push_str(&format!("$o{i}: String!, $n{i}: String!, "));
        body.push_str(&format!(
            "r{i}: repository(owner: $o{i}, name: $n{i}) {{ nameWithOwner \
               projectsV2(first: 30) {{ nodes {{ {PROJECT_FIELDS} }} }} }} "
        ));
    }
    format!("query({}) {{ {body} {RATE} }}", vars.trim_end_matches(", "))
}

/// One project by owner and number, with its fields. `repositoryOwner` finds users and
/// organizations alike.
pub fn project_meta() -> String {
    format!(
        "query($login: String!, $number: Int!) {{ repositoryOwner(login: $login) {{ \
           ... on ProjectV2Owner {{ projectV2(number: $number) {{ {PROJECT_FIELDS} \
             fields(first: 60) {{ nodes {{ __typename \
               ... on ProjectV2Field {{ id name dataType }} \
               ... on ProjectV2SingleSelectField {{ id name dataType options {{ id name }} }} \
               ... on ProjectV2IterationField {{ id name dataType configuration {{ \
                 iterations {{ id title startDate duration }} \
                 completedIterations {{ id title startDate duration }} }} }} \
             }} }} }} }} }} {RATE} }}"
    )
}

const FIELD_VALUES: &str = "fieldValues(first: 30) { nodes { __typename \
    ... on ProjectV2ItemFieldSingleSelectValue { name optionId field { ... on ProjectV2SingleSelectField { id name } } } \
    ... on ProjectV2ItemFieldIterationValue { title iterationId startDate duration field { ... on ProjectV2IterationField { id name } } } \
    ... on ProjectV2ItemFieldNumberValue { number field { ... on ProjectV2Field { id name } } } \
    ... on ProjectV2ItemFieldDateValue { date field { ... on ProjectV2Field { id name } } } \
    ... on ProjectV2ItemFieldTextValue { text field { ... on ProjectV2Field { id name } } } } }";

/// One page of the items of a project: the light part (fields, assignees, labels). Comments and
/// the timeline come later and only for what changed ([`item_details`]).
pub fn project_items() -> String {
    format!(
        "query($id: ID!, $after: String) {{ node(id: $id) {{ ... on ProjectV2 {{ \
           items(first: 50, after: $after) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ \
             id type updatedAt isArchived {FIELD_VALUES} \
             content {{ __typename \
               ... on DraftIssue {{ id title body createdAt updatedAt creator {{ login }} \
                 assignees(first: 10) {{ nodes {{ login }} }} }} \
               ... on Issue {{ id number title body state stateReason createdAt updatedAt url \
                 author {{ login }} repository {{ nameWithOwner }} \
                 assignees(first: 10) {{ nodes {{ login }} }} \
                 labels(first: 20) {{ nodes {{ name }} }} \
                 milestone {{ title dueOn }} }} \
               ... on PullRequest {{ number repository {{ nameWithOwner }} }} \
             }} }} }} }} }} {RATE} }}"
    )
}

const ISSUE_EXTRAS: &str = "issueType { name } \
    parent { number title state repository { nameWithOwner } } \
    subIssues(first: 50) { nodes { number title state repository { nameWithOwner } } }";

/// The heavy part for issues that are new or changed: comments, the timeline (pull requests
/// connected or cross-referenced) and closing references. `extras` adds issue type, parent and
/// sub-issues – left out for servers that do not have them yet.
pub fn issue_details(extras: bool) -> String {
    let extras = if extras { ISSUE_EXTRAS } else { "" };
    format!(
        "query($ids: [ID!]!) {{ nodes(ids: $ids) {{ __typename ... on Issue {{ id \
           comments(last: 30) {{ nodes {{ author {{ login }} body createdAt }} }} \
           timelineItems(first: 60, itemTypes: [CONNECTED_EVENT, CROSS_REFERENCED_EVENT]) {{ nodes {{ __typename \
             ... on ConnectedEvent {{ subject {{ __typename ... on PullRequest {{ number repository {{ nameWithOwner }} }} }} }} \
             ... on CrossReferencedEvent {{ willCloseTarget source {{ __typename ... on PullRequest {{ number repository {{ nameWithOwner }} }} }} }} }} }} \
           closedByPullRequestsReferences(first: 10, includeClosedPrs: true) {{ nodes {{ number repository {{ nameWithOwner }} }} }} \
           {extras} }} }} {RATE} }}"
    )
}
