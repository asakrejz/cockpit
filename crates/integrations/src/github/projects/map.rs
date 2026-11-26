//! Project items → inbox entries. Pure functions, without network.

use cockpit_core::project::{DEFAULT_TEAM_FIELD, draft_key, issue_key, status_of};
use cockpit_core::{
    DetailField, DetailValue, Details, FieldKind, FieldValue, Item, Kind, Person, Priority,
    ProjectItem, ReasonKind, Scope, Source, Sprint, SprintState, TeamRef, TicketStatus,
};
use cockpit_i18n::msg;

use super::dto::{Content, FieldValueNode, ItemNode};
use crate::{text, time};

mod heavy;

pub use heavy::{Heavy, apply_details, apply_heavy, heavy_of};

/// What the mapping needs besides the item.
pub struct MapCtx<'a> {
    /// Login of the signed-in user.
    pub me: &'a str,
    pub now_s: i64,
    /// Name of the project field that holds the team (default `Team`).
    pub team_field: &'a str,
}

impl Default for MapCtx<'_> {
    fn default() -> Self {
        MapCtx {
            me: "",
            now_s: 0,
            team_field: DEFAULT_TEAM_FIELD,
        }
    }
}

const DUE_FIELDS: &[&str] = &["due date", "due", "deadline", "target date", "end date"];

fn person_of(login: &str, me: &str) -> Person {
    if !me.is_empty() && login.eq_ignore_ascii_case(me) {
        Person::me()
    } else {
        text::person(login)
    }
}

/// The name for a plain name field (reporter, co-assignees): [`cockpit_core::ME_MARK`] for me.
fn name_of(login: &str, me: &str) -> String {
    let person = person_of(login, me);
    if person.me {
        cockpit_core::ME_MARK.to_string()
    } else {
        person.name
    }
}

/// A number the way people write it: `3`, `2.5`.
fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn day_to_secs(day: &str) -> Option<i64> {
    time::parse_iso(&format!("{}T00:00:00Z", day.get(..10)?))
}

/// `2026-11-16` for seconds since 1970 (UTC).
fn day_of(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

/// Where an iteration stands: not started, running or over.
pub fn iteration_state(start_day: &str, duration_days: u32, now_s: i64) -> Option<SprintState> {
    let start = day_to_secs(start_day)?;
    let end = start + i64::from(duration_days.max(1)) * 86_400;
    Some(if now_s < start {
        SprintState::Future
    } else if now_s >= end {
        SprintState::Closed
    } else {
        SprintState::Active
    })
}

/// The priority from the name of an option of a field called `Priority`: `High`, `P1`, `Urgent` …
pub fn priority_of(name: &str) -> Priority {
    match name.trim().to_lowercase().as_str() {
        "critical" | "urgent" | "highest" | "blocker" | "p0" => Priority::Highest,
        "high" | "important" | "p1" => Priority::High,
        "low" | "minor" | "p3" => Priority::Low,
        "lowest" | "trivial" | "p4" => Priority::Lowest,
        _ => Priority::Medium,
    }
}

/// One value of a project field, ready to show. `None` for kinds Cockpit does not show.
fn field_value(v: &FieldValueNode) -> Option<FieldValue> {
    let field = v.field.as_ref()?;
    let (field_id, name) = (field.id.clone()?, field.name.clone()?);
    let (kind, text, option_id) = match v.typename.as_str() {
        "ProjectV2ItemFieldSingleSelectValue" => (
            FieldKind::SingleSelect,
            v.name.clone()?,
            v.option_id.clone(),
        ),
        "ProjectV2ItemFieldIterationValue" => (
            FieldKind::Iteration,
            v.title.clone()?,
            v.iteration_id.clone(),
        ),
        "ProjectV2ItemFieldNumberValue" => (FieldKind::Number, number_text(v.number?), None),
        "ProjectV2ItemFieldDateValue" => (FieldKind::Date, v.date.clone()?, None),
        "ProjectV2ItemFieldTextValue" => (FieldKind::Text, v.text.clone()?, None),
        _ => return None,
    };
    (!text.trim().is_empty()).then_some(FieldValue {
        field_id,
        name,
        kind,
        text,
        option_id,
    })
}

/// A project item as an inbox entry. Pull requests in the project and items the credential
/// cannot see (`content` missing) give `None`: pull requests are synced as pull requests.
/// Comments, linked pull requests, parent and sub-issues come later ([`apply_details`]).
pub fn map_item(
    node: &ItemNode,
    meta: &cockpit_core::ProjectMeta,
    ctx: &MapCtx<'_>,
) -> Option<Item> {
    let c: &Content = node.content.as_ref()?;
    let draft = match c.typename.as_str() {
        "Issue" => false,
        "DraftIssue" => true,
        _ => return None,
    };
    let project = meta.reference().to_string();
    let repo = c
        .repository
        .as_ref()
        .map(|r| r.name_with_owner.clone())
        .filter(|r| !r.is_empty());
    let number = c.number.filter(|n| *n > 0);
    let (key, id, repo_opt) = if draft {
        (
            draft_key(&project, &node.id),
            cockpit_core::project::draft_id(&node.id),
            None,
        )
    } else {
        let (repo, number) = (repo?, number?);
        (
            issue_key(&repo, number),
            cockpit_core::project::issue_id(&repo, number),
            Some(repo),
        )
    };

    // Project fields.
    let status_def = meta.status_def();
    let mut status_option: Option<String> = None;
    let mut fields: Vec<FieldValue> = Vec::new();
    let mut team: Option<TeamRef> = None;
    let mut priority = Priority::default();
    let mut due: Option<String> = None;
    let mut sprint: Option<Sprint> = None;
    for v in node.field_values.iter().flat_map(|c| c.items()) {
        let Some(fv) = field_value(v) else { continue };
        if fv.name.eq_ignore_ascii_case("Title") {
            continue;
        }
        if status_def.is_some_and(|d| d.id == fv.field_id) {
            status_option = fv.option_id.clone();
            continue;
        }
        if fv.name.eq_ignore_ascii_case(ctx.team_field.trim()) {
            team = Some(TeamRef {
                id: fv.option_id.clone().unwrap_or_else(|| fv.text.clone()),
                name: fv.text.clone(),
            });
        }
        match fv.kind {
            FieldKind::SingleSelect if fv.name.eq_ignore_ascii_case("Priority") => {
                priority = priority_of(&fv.text);
            }
            FieldKind::Date if DUE_FIELDS.contains(&fv.name.to_lowercase().as_str()) => {
                due = Some(fv.text.chars().take(10).collect());
            }
            FieldKind::Iteration => {
                let start = v.start_date.clone().unwrap_or_default();
                let days = v.duration.unwrap_or(0);
                if let Some(state) = iteration_state(&start, days, ctx.now_s) {
                    let end = day_to_secs(&start)
                        .map(|s| day_of(s + i64::from(days.max(1)) * 86_400))
                        .unwrap_or_default();
                    sprint = Some(Sprint {
                        name: fv.text.clone(),
                        state,
                        start,
                        end,
                    });
                }
            }
            _ => {}
        }
        fields.push(fv);
    }

    let closed = c.state.as_deref() == Some("CLOSED");
    let options = status_def.map(|d| d.options.as_slice()).unwrap_or(&[]);
    let status = match (&status_option, closed) {
        (Some(_), _) => status_of(options, status_option.as_deref()),
        (None, true) => TicketStatus::Done,
        (None, false) => TicketStatus::Open,
    };

    // People.
    let logins: Vec<String> = c
        .assignees
        .iter()
        .flat_map(|a| a.items())
        .map(|l| l.login.clone())
        .filter(|l| !l.is_empty())
        .collect();
    let mine = logins.iter().any(|l| l.eq_ignore_ascii_case(ctx.me));
    let owner = match logins.first() {
        Some(_) if mine => Person::me(),
        Some(l) => text::person(l),
        None => Person::nobody(),
    };
    let owner_name = logins.first().map(|l| name_of(l, ctx.me));
    let co_assignees: Vec<String> = logins
        .iter()
        .map(|l| name_of(l, ctx.me))
        .filter(|n| Some(n) != owner_name.as_ref())
        .collect();
    let (reason, reason_msg, scope) = if mine {
        (ReasonKind::Assigned, msg!(ReasonAssigned), Scope::Mine)
    } else {
        (ReasonKind::Comment, msg!(ReasonInProject), Scope::Team)
    };

    let labels: Vec<String> = c
        .labels
        .iter()
        .flat_map(|l| l.items())
        .map(|l| l.name.clone())
        .filter(|n| !n.is_empty())
        .collect();
    let milestone = c
        .milestone
        .as_ref()
        .map(|m| m.title.clone())
        .filter(|t| !t.is_empty());
    let reporter = c
        .author
        .as_ref()
        .or(c.creator.as_ref())
        .map(|a| a.login.as_str())
        .filter(|l| !l.is_empty())
        .map(|l| name_of(l, ctx.me));
    // The later of the item's and the issue's last change (ISO strings sort like time).
    let updated = [node.updated_at.as_str(), c.updated_at.as_str()]
        .into_iter()
        .max()
        .unwrap_or_default()
        .to_string();

    // What the detail pane lists besides the description.
    let mut details = Details::default();
    let mut put = |id: &str, name: &str, value: DetailValue| {
        details.fields.push(DetailField {
            id: id.into(),
            name: name.into(),
            value,
        });
    };
    put("project", "", DetailValue::Text(meta.title.clone()));
    if logins.len() > 1 {
        put(
            "assignee",
            "",
            DetailValue::List(logins.iter().map(|l| name_of(l, ctx.me)).collect()),
        );
    }
    if let Some(r) = &reporter {
        put("reporter", "", DetailValue::Text(r.clone()));
    }
    if !labels.is_empty() {
        put("labels", "", DetailValue::List(labels.clone()));
    }
    if let Some(m) = &milestone {
        put("milestone", "", DetailValue::Text(m.clone()));
    }
    if let Some(d) = &due {
        put("duedate", "", DetailValue::Date(d.clone()));
    }
    for f in &fields {
        let value = match f.kind {
            FieldKind::Date => DetailValue::Date(f.text.chars().take(10).collect()),
            _ => DetailValue::Text(f.text.clone()),
        };
        put(&format!("ghp:{}", f.field_id), &f.name, value);
    }
    if !c.created_at.is_empty() {
        put(
            "created",
            "",
            DetailValue::Date(c.created_at.chars().take(10).collect()),
        );
    }

    let url = if draft {
        Some(meta.url.clone()).filter(|u| !u.is_empty())
    } else {
        c.url.clone().filter(|u| !u.is_empty())
    };

    Some(Item {
        id,
        kind: Kind::Ticket,
        source: Source::GitHubProject,
        key,
        title: c.title.clone(),
        description: c.body.clone(),
        description_rich: None,
        reason,
        reason_msg: Some(reason_msg),
        reason_text: String::new(),
        time: String::new(),
        scope,
        owner,
        status,
        priority,
        branch: None,
        pr: None,
        agent_note: None,
        agent_note_msg: None,
        comments: Vec::new(),
        read: false,
        due,
        repo: repo_opt,
        labels,
        sprint,
        issue_type: None,
        reporter,
        watching: false,
        team,
        updated_ms: time::parse_iso(&updated).map(|s| s.max(0) as u64 * 1000),
        url,
        details: Some(details),
        project: Some(ProjectItem {
            project_id: meta.id.clone(),
            project,
            project_title: meta.title.clone(),
            item_id: node.id.clone(),
            content_id: c.id.clone(),
            draft,
            closed,
            number: if draft { None } else { number },
            assignees: logins,
            milestone: milestone.clone(),
            status_option,
            fields,
            linked_prs: Vec::new(),
            parent: None,
            sub_issues: Vec::new(),
        }),
        co_assignees,
        milestone,
    })
}
