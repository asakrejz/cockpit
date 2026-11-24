//! Actions on tickets of GitHub projects. The backend writes to GitHub first; these are the local
//! effects once GitHub has said yes (and the rules that apply before it is asked at all).

use cockpit_i18n::msg;

use super::*;
use crate::model::{Item, Kind, Person, Priority, ReasonKind, Scope, Source};
use crate::project::{FieldInput, FieldKind, FieldValue, issue_id, issue_key};

impl Workspace {
    /// The ticket of a GitHub project, or why it is none.
    fn project_ticket(&self, id: &str) -> Result<&Item, ActionError> {
        let item = self
            .item(id)
            .ok_or_else(|| ActionError::UnknownItem(id.to_string()))?;
        if item.source != Source::GitHubProject || item.project.is_none() {
            return Err(ActionError::NotATicket);
        }
        Ok(item)
    }

    /// An issue (not a draft) of a GitHub project.
    fn project_issue(&self, id: &str) -> Result<&Item, ActionError> {
        let item = self.project_ticket(id)?;
        if !item.project.as_ref().is_some_and(|p| p.is_issue()) {
            return Err(ActionError::NotAnIssue);
        }
        Ok(item)
    }

    pub(super) fn perform_projects(
        &mut self,
        action: Action,
        at_ms: u64,
    ) -> Result<Vec<Note>, ActionError> {
        match action {
            Action::SetLabel { item, label, on } => {
                let label = label.trim().to_string();
                if label.is_empty() || label.contains(',') || label.chars().any(char::is_control) {
                    return Err(ActionError::InvalidInput);
                }
                let current = self.project_issue(&item)?;
                let has = current
                    .labels
                    .iter()
                    .any(|l| l.eq_ignore_ascii_case(&label));
                if has == on {
                    return Err(ActionError::Unchanged);
                }
                let key = current.key.clone();
                if let Some(i) = self.item_mut(&item) {
                    if on {
                        i.labels.push(label.clone());
                    } else {
                        i.labels.retain(|l| !l.eq_ignore_ascii_case(&label));
                    }
                }
                let text = if on {
                    msg!(CoreActionLogLabelAdded, key = key, label = label)
                } else {
                    msg!(CoreActionLogLabelRemoved, key = key, label = label)
                };
                Ok(vec![Note::coded(Target::Item(item), text)])
            }
            Action::SetIssueClosed { item, closed } => {
                let current = self.project_issue(&item)?;
                let key = current.key.clone();
                if current.project.as_ref().is_some_and(|p| p.closed == closed) {
                    return Err(ActionError::Unchanged);
                }
                if let Some(p) = self.item_mut(&item).and_then(|i| i.project.as_mut()) {
                    p.closed = closed;
                }
                let text = if closed {
                    msg!(CoreActionLogIssueClosed, key = key)
                } else {
                    msg!(CoreActionLogIssueReopened, key = key)
                };
                Ok(vec![Note::coded(Target::Item(item), text)])
            }
            Action::SetProjectField {
                item,
                field_id,
                value,
            } => {
                let current = self.project_ticket(&item)?;
                let key = current.key.clone();
                let project = current.project.clone().unwrap_or_default();
                let meta = self.sync.project(&project.project_id);
                let def = meta
                    .and_then(|m| m.field(&field_id))
                    .ok_or_else(|| ActionError::UnknownField(field_id.clone()))?;
                // The status has its own action, so that its rules (the columns) apply.
                if meta
                    .and_then(|m| m.status_def())
                    .is_some_and(|s| s.id == def.id)
                {
                    return Err(ActionError::InvalidInput);
                }
                let new = match &value {
                    None => None,
                    Some(v) if !v.is_valid() => return Err(ActionError::InvalidInput),
                    Some(v) => {
                        let (kind, text, option_id) = match (v, def.kind) {
                            (FieldInput::Select(id), FieldKind::SingleSelect)
                            | (FieldInput::Iteration(id), FieldKind::Iteration) => {
                                let name = def
                                    .options
                                    .iter()
                                    .find(|o| o.id == *id)
                                    .map(|o| o.name.clone())
                                    .ok_or_else(|| ActionError::UnknownField(id.clone()))?;
                                (def.kind, name, Some(id.clone()))
                            }
                            (FieldInput::Text(t), FieldKind::Text) => {
                                (def.kind, t.trim().to_string(), None)
                            }
                            (FieldInput::Number(n), FieldKind::Number) => {
                                (def.kind, n.trim().to_string(), None)
                            }
                            (FieldInput::Date(d), FieldKind::Date) => {
                                (def.kind, d.trim().to_string(), None)
                            }
                            _ => return Err(ActionError::InvalidInput),
                        };
                        Some(FieldValue {
                            field_id: def.id.clone(),
                            name: def.name.clone(),
                            kind,
                            text,
                            option_id,
                        })
                    }
                };
                let field_name = def.name.clone();
                let old = project.fields.iter().find(|f| f.field_id == field_id);
                if old == new.as_ref() {
                    return Err(ActionError::Unchanged);
                }
                let shown = new.as_ref().map(|f| f.text.clone());
                if let Some(p) = self.item_mut(&item).and_then(|i| i.project.as_mut()) {
                    p.fields.retain(|f| f.field_id != field_id);
                    p.fields.extend(new);
                }
                let text = match shown {
                    Some(v) => msg!(
                        CoreActionLogFieldSet,
                        key = key,
                        field = field_name,
                        value = v
                    ),
                    None => msg!(CoreActionLogFieldCleared, key = key, field = field_name),
                };
                Ok(vec![Note::coded(Target::Item(item), text)])
            }
            Action::CreateIssue {
                project,
                repo,
                title,
                body,
                status,
                created,
            } => {
                let title = title.trim();
                if title.is_empty() || !crate::project::repo_valid(&repo) {
                    return Err(ActionError::InvalidInput);
                }
                let meta = self
                    .sync
                    .project(&project)
                    .ok_or_else(|| ActionError::UnknownField(project.clone()))?
                    .clone();
                // GitHub has to say yes first: without its answer there is nothing to show.
                let created = created.ok_or(ActionError::NeedsBackend)?;
                let id = issue_id(&created.repo, created.number);
                if self.item(&id).is_some() {
                    return Err(ActionError::Unchanged);
                }
                let key = issue_key(&created.repo, created.number);
                let status_option = status.and_then(|s| {
                    meta.status_def()
                        .and_then(|d| crate::project::option_for(&d.options, s))
                        .map(|o| o.id.clone())
                });
                let item = Item {
                    id: id.clone(),
                    kind: Kind::Ticket,
                    source: Source::GitHubProject,
                    key: key.clone(),
                    title: title.to_string(),
                    description: body.trim().to_string(),
                    description_rich: None,
                    reason: ReasonKind::Comment,
                    reason_msg: Some(cockpit_i18n::msg!(ReasonInProject)),
                    reason_text: String::new(),
                    time: String::new(),
                    scope: Scope::Team,
                    owner: Person::new(crate::UNASSIGNED_NAME, "–"),
                    status: status.unwrap_or_default(),
                    priority: Priority::default(),
                    branch: None,
                    pr: None,
                    agent_note: None,
                    agent_note_msg: None,
                    comments: Vec::new(),
                    read: false,
                    due: None,
                    repo: Some(created.repo.clone()),
                    labels: Vec::new(),
                    sprint: None,
                    issue_type: None,
                    reporter: Some(Person::me().name),
                    watching: false,
                    team: None,
                    updated_ms: Some(at_ms),
                    url: Some(created.url.clone()).filter(|u| !u.is_empty()),
                    details: None,
                    project: Some(crate::ProjectItem {
                        project_id: meta.id.clone(),
                        project: meta.reference().to_string(),
                        project_title: meta.title.clone(),
                        item_id: created.item_id.clone(),
                        content_id: created.content_id.clone(),
                        number: Some(created.number),
                        status_option,
                        ..Default::default()
                    }),
                    co_assignees: Vec::new(),
                    milestone: None,
                };
                self.items.insert(0, item);
                self.sync.items.push(id.clone());
                Ok(vec![Note::coded(
                    Target::Item(id),
                    msg!(CoreActionLogIssueCreated, key = key, project = meta.title),
                )])
            }
            Action::ConvertDraft {
                item,
                repo,
                created,
            } => {
                let current = self.project_ticket(&item)?;
                if !current.project.as_ref().is_some_and(|p| p.draft) {
                    return Err(ActionError::NotADraft);
                }
                if !crate::project::repo_valid(&repo) {
                    return Err(ActionError::InvalidInput);
                }
                let created = created.ok_or(ActionError::NeedsBackend)?;
                let new_id = issue_id(&created.repo, created.number);
                let key = issue_key(&created.repo, created.number);
                if self.item(&new_id).is_some() {
                    return Err(ActionError::Unchanged);
                }
                if let Some(i) = self.item_mut(&item) {
                    i.id = new_id.clone();
                    i.key = key.clone();
                    i.repo = Some(created.repo.clone());
                    i.url = Some(created.url.clone()).filter(|u| !u.is_empty());
                    if let Some(p) = i.project.as_mut() {
                        p.draft = false;
                        p.number = Some(created.number);
                        p.content_id = created.content_id.clone();
                        p.item_id = created.item_id.clone();
                    }
                }
                for s in self.sync.items.iter_mut().filter(|s| **s == item) {
                    s.clone_from(&new_id);
                }
                for snooze in self.snoozed.iter_mut().filter(|s| s.item == item) {
                    snooze.item.clone_from(&new_id);
                }
                Ok(vec![Note::coded(
                    Target::Item(new_id),
                    msg!(CoreActionLogDraftConverted, key = key),
                )])
            }
            _ => Err(ActionError::NothingToDo),
        }
    }
}
