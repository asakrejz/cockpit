//! Repos and branches: what the key can reach in a repo, the list of repos to choose from, the
//! contribution guidelines, and creating and deleting a branch.

use super::super::dto::{
    BranchInfo, ContentFile, GitRefResponse, InstallationRepos, Installations, Repo, RepoListEntry,
};
use super::{GitHubClient, MAX_REPOS, RepoAccess, RepoInfo, RepoList};
use crate::Error;
use crate::outbound::branch_request;

impl GitHubClient {
    /// Creates `name` as a ref from the default branch of the repo.
    pub async fn branch_create(&self, repo: &str, name: &str) -> Result<(), Error> {
        let r: Repo = self.read_json(self.get(&format!("/repos/{repo}"))).await?;
        let r#ref: GitRefResponse = self
            .read_json(self.get(&format!("/repos/{repo}/git/ref/heads/{}", r.default_branch)))
            .await?;
        self.send(&branch_request(repo, name, &r#ref.object.sha))
            .await
            .map(|_| ())
    }

    /// Deletes the branch `name`. **Only on the user's click**, after
    /// `cockpit_core::branch::deletable` allowed it. Never touches anything else.
    pub async fn branch_delete(&self, repo: &str, name: &str) -> Result<(), Error> {
        self.send(&crate::outbound::branch_delete_request(repo, name))
            .await
            .map(|_| ())
    }

    /// Is the branch protected on GitHub? Read-only.
    pub async fn branch_protected(&self, repo: &str, name: &str) -> Result<bool, Error> {
        let info: BranchInfo = self
            .read_json(self.get(&format!("/repos/{repo}/branches/{name}")))
            .await?;
        Ok(info.protected)
    }

    /// Read-only look at a repo: does it exist, may the key holder see it and push to it?
    /// Changes nothing.
    pub async fn repo_access(&self, repo: &str) -> Result<RepoAccess, Error> {
        let r: Repo = self.read_json(self.get(&format!("/repos/{repo}"))).await?;
        Ok(RepoAccess {
            default_branch: r.default_branch,
            archived: r.archived,
            can_push: r.permissions.map(|p| p.push),
        })
    }

    /// The contribution guidelines of a repo: the first of `CONTRIBUTING.md`,
    /// `.github/CONTRIBUTING.md` and `docs/CONTRIBUTING.md` that exists (`GET
    /// /repos/{repo}/contents/{path}`). Read-only. `None` if there is none; files over 200 kB
    /// are skipped. Returns the path and the text.
    pub async fn contributing(&self, repo: &str) -> Result<Option<(String, String)>, Error> {
        for path in cockpit_core::branch::GUIDELINE_FILES {
            let file: Result<ContentFile, Error> = self
                .read_json(self.get(&format!("/repos/{repo}/contents/{path}")))
                .await;
            let file = match file {
                Ok(f) => f,
                // No such file: try the next place.
                // A folder with that name answers with a list, not a file.
                Err(Error::NotFound | Error::Response(_)) => continue,
                Err(e) => return Err(e),
            };
            if file.kind != "file" || file.encoding != "base64" || file.size > 200_000 {
                continue;
            }
            use base64::Engine as _;
            let clean: String = file
                .content
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(clean) else {
                continue;
            };
            return Ok(Some((
                path.to_string(),
                String::from_utf8_lossy(&bytes).into_owned(),
            )));
        }
        Ok(None)
    }

    /// The repos a personal access token can reach (`GET /user/repos`): own, as collaborator
    /// and through organizations. Read-only; at most [`MAX_REPOS`], sorted by name.
    pub async fn repo_list(&self) -> Result<RepoList, Error> {
        let mut list = RepoList::default();
        for page in 1..=(MAX_REPOS / 100 + 1) {
            let part: Vec<RepoListEntry> = self
                .read_json(self.get("/user/repos").query(&[
                    ("affiliation", "owner,collaborator,organization_member"),
                    ("sort", "full_name"),
                    ("per_page", "100"),
                    ("page", &page.to_string()),
                ]))
                .await?;
            let full = part.len() == 100;
            list.repos.extend(part.into_iter().map(RepoInfo::from));
            if !full {
                break;
            }
        }
        list_finish(&mut list);
        Ok(list)
    }

    /// The installations of the app the user can use (`GET /user/installations`): the name of
    /// the app in its address (if GitHub tells it) per installation. Read-only.
    pub async fn installations(&self) -> Result<Vec<Option<String>>, Error> {
        let installations: Installations = self
            .read_json(
                self.get("/user/installations")
                    .query(&[("per_page", "100")]),
            )
            .await?;
        Ok(installations
            .installations
            .into_iter()
            .map(|i| i.app_slug)
            .collect())
    }

    /// The repos a GitHub App sign-in can reach: all installations of the app for the user
    /// (`GET /user/installations`) and their repos. Read-only.
    pub async fn installation_repo_list(&self) -> Result<RepoList, Error> {
        let installations: Installations = self
            .read_json(
                self.get("/user/installations")
                    .query(&[("per_page", "100")]),
            )
            .await?;
        let mut list = RepoList::default();
        for i in installations.installations {
            for page in 1..=(MAX_REPOS / 100 + 1) {
                let part: InstallationRepos = self
                    .read_json(
                        self.get(&format!("/user/installations/{}/repositories", i.id))
                            .query(&[("per_page", "100"), ("page", &page.to_string())]),
                    )
                    .await?;
                let full = part.repositories.len() == 100;
                list.repos
                    .extend(part.repositories.into_iter().map(RepoInfo::from));
                if !full {
                    break;
                }
            }
        }
        list_finish(&mut list);
        Ok(list)
    }
}

/// Sorts by name, drops duplicates and applies the limit.
fn list_finish(list: &mut RepoList) {
    list.repos.sort_by_key(|r| r.repo.to_lowercase());
    list.repos.dedup_by(|a, b| a.repo == b.repo);
    if list.repos.len() > MAX_REPOS {
        list.repos.truncate(MAX_REPOS);
        list.truncated = true;
    }
}
