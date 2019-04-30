
use std::borrow::Cow;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::{Command};

use super::{ToolArgs, ToolArg, ToolArgAccessor, CommandQueue,
           ToolInvocation, };

use super::git;
use super::git2;

#[derive(Clone, Debug)]
pub enum RepoRoot {
  Git {
    url: Cow<'static, str>,
    branch: Cow<'static, str>,
  },
  Local {
    path: PathBuf,
    branch: Cow<'static, str>,
  }
}
impl RepoRoot {
  pub fn branch(&self) -> &Cow<'static, str> {
    match self {
      &RepoRoot::Git {
        ref branch, ..
      } | &RepoRoot::Local {
        ref branch, ..
      } => branch,
    }
  }
  pub fn branch_mut(&mut self) -> &mut Cow<'static, str> {
    match self {
      &mut RepoRoot::Git {
        ref mut branch, ..
      } | &mut RepoRoot::Local {
        ref mut branch, ..
      } => branch,
    }
  }
}

#[derive(Clone, Debug)]
pub struct Repo {
  pub name: Cow<'static, str>,
  pub root: RepoRoot,
  /// ignored if root is local.
  pub commit: Option<Cow<'static, str>>,
  pub clobber: bool,
}

impl Repo {
  pub fn new_git<T, U, V>(name: T, url: U, branch: V) -> Self
    where T: Into<Cow<'static, str>>,
          U: Into<Cow<'static, str>>,
          V: Into<Cow<'static, str>>,
  {
    Repo {
      name: name.into(),
      root: RepoRoot::Git {
        url: url.into(),
        branch: branch.into(),
      },
      commit: None,
      clobber: true,
    }
  }
  pub fn new_git_commit<T, U, V, W>(name: T, url: U, branch: V,
                                    commit: W)
    -> Self
    where T: Into<Cow<'static, str>>,
          U: Into<Cow<'static, str>>,
          V: Into<Cow<'static, str>>,
          W: Into<Cow<'static, str>>,
  {
    Repo {
      name: name.into(),
      root: RepoRoot::Git {
        url: url.into(),
        branch: branch.into(),
      },
      commit: Some(commit.into()),
      clobber: true,
    }
  }

  pub fn checkout<T>(&self, dest: T, fat: bool)
    -> Result<(), Box<Error>>
    where T: AsRef<Path>,
  {
    let dest = dest.as_ref();
    if self.clobber || !dest.exists() {
      let (over, url, branch) = match self.root {
        RepoRoot::Local { ref path, ref branch, } => {
          (Some(path), "", branch.as_ref())
        },
        RepoRoot::Git { ref url, ref branch, } => {
          (None, url.as_ref(), branch.as_ref())
        },
      };
      let (cref, checkout) = if let RepoRoot::Local { .. } = self.root {
        (branch, git::checkout_repo as fn(&str, &Path, &str, &str, bool))
      } else if let Some(commit) = self.commit.as_ref() {
        (commit.as_ref(), git::checkout_repo_commit as _)
      } else {
        (branch, git::checkout_repo as _)
      };
      git::checkout_or_override_raw(self.name.as_ref(),
                                    &dest,
                                    over, url,
                                    cref, !fat,
                                    checkout)?;
    }

    Ok(())
  }
  pub fn checkout_thin<T>(&self, dest: T)
    -> Result<(), Box<Error>>
    where T: AsRef<Path>,
  {
    self.checkout(dest, false)
  }
  pub fn checkout_fat<T>(&self, dest: T)
    -> Result<(), Box<Error>>
    where T: AsRef<Path>,
  {
    self.checkout(dest, true)
  }

  pub fn remote_name(&self) -> String {
    format!("remote-{}-branch-{}", self.name, self.root.branch())
  }

  pub fn add_remote_from<T>(&self, checkout: &PathBuf,
                            from: &Repo,
                            queue: &mut CommandQueue<T>)
    -> Result<(), Box<Error>>
    where T: ToolInvocation + 'static,
  {

    let remote_name = from.remote_name();
    {
      let repo = git2::Repository::open(checkout)?;

      if let Ok(_remote) = repo.find_remote(&remote_name) {
        match from.root {
          RepoRoot::Git { ref url, .. } => {
            repo.remote_set_url(&remote_name, url)?;
          },
          RepoRoot::Local { ref path, .. } => {
            repo.remote_set_url(&remote_name,
                                path.to_str().unwrap())?;
          },
        }

        return Ok(());
      };
    }

    let mut cmd = Command::new("git");
    cmd.current_dir(checkout)
      .arg("remote")
      .arg("add")
      .arg("-t")
      .arg(from.root.branch().as_ref())
      .arg(remote_name);

    match from.root {
      RepoRoot::Git { ref url, .. } => {
        cmd.arg(url.as_ref());
      },
      RepoRoot::Local { ref path, .. } => {
        cmd.arg(path);
      },
    }

    queue.enqueue_simple_external(Some("add-remote"),
                                  cmd,
                                  None);

    Ok(())
  }

  pub fn update_remotes<T>(&self, checkout: &PathBuf,
                           queue: &mut CommandQueue<T>)
    where T: ToolInvocation + 'static,
  {
    let mut cmd = Command::new("git");
    cmd.current_dir(checkout)
      .arg("fetch")
      .arg("--all");

    queue.enqueue_simple_external(Some("update-remotes"),
                                  cmd,
                                  None);
  }

  /// `start_at` must be added as a remote.
  pub fn create_or_reset_branch<T, U>(&self,
                                      checkout: &PathBuf,
                                      branch_name: T,
                                      start_at: &Repo,
                                      queue: &mut CommandQueue<U>)
    -> Result<Repo, Box<Error>>
    where T: AsRef<str>,
          U: ToolInvocation + 'static,
  {
    let mut cmd = Command::new("git");

    let start = format!("{}/{}", start_at.remote_name(),
                        start_at.root.branch());

    cmd.current_dir(checkout)
      .arg("checkout")
      .arg("-B")
      .arg(branch_name.as_ref())
      .arg(&start);

    queue.enqueue_simple_external(Some("create-new-branch"),
                                  cmd, None);

    Ok(Repo {
      name: self.name.clone(),
      root: RepoRoot::Local {
        path: checkout.clone(),
        branch: branch_name.as_ref().to_string().into(),
      },
      clobber: false,
      commit: None,
    })
  }
  pub fn merge_branch<T>(&self, checkout: &PathBuf,
                         branch: &Repo,
                         queue: &mut CommandQueue<T>)
    -> Result<(), Box<Error>>
    where T: ToolInvocation + 'static,
  {
    let mut cmd = Command::new("git");

    let remote_name = branch.remote_name();
    let merge_branch = format!("{}/{}",
                               remote_name,
                               branch.root.branch());

    cmd.current_dir(checkout)
      .arg("merge")
      .arg("--no-edit")
      .arg(merge_branch);

    queue.enqueue_simple_external(Some("merge-branch"),
                                  cmd, None);

    Ok(())
  }

  pub fn args<T, Deref>(&self, into: &mut ToolArgs<T>)
    where Deref: ToolArgAccessor<T, Self>,
  {
    let single = format!("^--{}-src=(.*)$",
                         self.name).into();
    let split  = format!("^--{}-src$",
                         self.name).into();

    let o = ToolArg {
      name: self.name.clone(),
      single: Some(single),
      split: Some(split),
      action: Some(|this: &mut T, single, cap| {
        let cdir = std::env::current_dir()?;
        let state = Deref::access(this);
        expand_style!(single_and_split_simple_path(src) => single, cap);
        let src = cdir.join(src);
        state.root = RepoRoot::Local {
          path: src,
          branch: state.root.branch().clone(),
        };
        Ok(())
      }),
      help: None,
    };
    into.to_mut().push(o);

    let single = format!("^--{}-branch=(.*)$",
                         self.name).into();
    let split  = format!("^--{}-branch$",
                         self.name).into();

    let o = ToolArg {
      name: format!("{}-src-branch", self.name).into(),
      single: Some(single),
      split: Some(split),
      action: Some(|this, single, cap| {
        let state = Deref::access(this);
        expand_style!(single_and_split_str(branch) => single, cap);
        *state.root.branch_mut() = Cow::Owned(branch.into());
        Ok(())
      }),
      help: None,
    };
    into.to_mut().push(o);

    let single = format!("^--(no-)clobber-{}-srcs$", self.name)
      .into();

    let o = ToolArg {
      name: format!("clobber-{}-sources", self.name).into(),
      single: Some(single),
      split: None,
      action: Some(|this, _single, cap| {
        let state = Deref::access(this);
        expand_style!(simple_no_flag(b) => single, cap);
        state.clobber = b;
        Ok(())
      }),
      help: None,
    };
    into.to_mut().push(o);
  }
}
