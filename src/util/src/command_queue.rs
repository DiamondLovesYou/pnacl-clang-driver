
use std;
use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Debug, Formatter};
use std::fs::{copy};
use std::ops::{Deref, DerefMut};
use std::path::{PathBuf};
use std::process;
use std::rc::Rc;
use std::sync::{Once, };
use std::sync::atomic::{AtomicBool, Ordering, };

use tempdir::TempDir;

use super::{ToolInvocation, process_invocation_args,
            boolean_env};

static STOP_BEFORE_NEXT_JOB: AtomicBool = AtomicBool::new(false);
static CTRL_C_HANDLER: Once = Once::new();

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum InputArgsTransformResult {
  Normal,
  Skip,
}
impl Default for InputArgsTransformResult {
  fn default() -> Self {
    InputArgsTransformResult::Normal
  }
}

/// if Some(..), its value will be the argument used. The output will be
/// written to a random temp folder && added to the next command's
/// arguments.
/// ie Some("-o")
#[derive(Debug)]
pub struct CommandTool<T>(T);
impl<T> Deref for CommandTool<T> {
  type Target = T;
  fn deref(&self) -> &T { &self.0 }
}
impl<T> DerefMut for CommandTool<T> {
  fn deref_mut(&mut self) -> &mut T { &mut self.0 }
}
pub struct ExternalCommand(process::Command,
                           Option<Cow<'static, str>>,
                           Option<Box<dyn FnOnce(&mut process::Command, &[PathBuf]) -> InputArgsTransformResult>>);
impl Debug for ExternalCommand {
  fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
    match self.2 {
      Some(_) => write!(fmt, "ExternalCommand({:?}, {:?}, Some(..))", self.0, self.1),
      None => write!(fmt, "ExternalCommand({:?}, {:?}, None)", self.0, self.1),
    }
  }
}
pub struct FunctionCommand<T>(Option<Box<dyn FnOnce(&mut &mut T) -> Result<(), CommandQueueError>>>);
impl<T> Debug for FunctionCommand<T> {
  fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
    match self {
      &FunctionCommand(Some(..)) => {
        write!(fmt, "FunctionCommand(Some(..))")
      },
      &FunctionCommand(None) => {
        write!(fmt, "FunctionCommand(None)")
      },
    }
  }
}
pub struct FunctionCommandWithState<T>(Option<Box<dyn FnOnce(&mut &mut T, &mut RunState) -> Result<(), CommandQueueError>>>);
impl<T> Debug for FunctionCommandWithState<T> {
  fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
    match self {
      &FunctionCommandWithState(Some(..)) => {
        write!(fmt, "FunctionCommandWithState(Some(..))")
      },
      &FunctionCommandWithState(None) => {
        write!(fmt, "FunctionCommandWithState(None)")
      },
    }
  }
}

#[derive(Debug)]
pub struct ConcreteCommand {
  pub name: Option<Cow<'static, str>>,
  /// should we print the command we just tried to run if it exits with a non-zero status?
  pub cant_fail: bool,
  pub tmp_dirs: Vec<Rc<TempDir>>,
  pub intermediate_name: Option<PathBuf>,
  pub prev_outputs: bool,
  pub output_override: bool,
  pub copy_output_to: Option<PathBuf>,
}

impl ConcreteCommand {
  pub fn copy_output_to(&self, out: PathBuf) -> Result<(), Box<dyn Error>> {
    if let Some(copy_to) = self.copy_output_to.as_ref() {
      copy(out, copy_to)?;
    }

    Ok(())
  }
}

#[derive(Debug)]
pub struct Command<T>
  where T: Debug,
{
  pub cmd: T,
  pub concrete: ConcreteCommand,
}
impl<T> Deref for Command<T>
  where T: Debug,
{
  type Target = ConcreteCommand;
  fn deref(&self) -> &Self::Target {
    &self.concrete
  }
}
impl<T> DerefMut for Command<T>
  where T: Debug,
{
  fn deref_mut(&mut self) -> &mut ConcreteCommand {
    &mut self.concrete
  }
}
impl<T> Command<T>
  where T: Debug,
{ }
impl<T, U> ICommand<U> for Command<CommandTool<T>>
  where T: ToolInvocation + 'static,
{
  fn run(&mut self, _: &mut &mut U,
         state: &mut RunState) -> Result<(), CommandQueueError> {
    info!("on command: {:?} => {:?}", self.name, self.cmd);

    let mut out = state.output(&self.intermediate_name);

    if self.prev_outputs {
      for prev in state.prev_outputs.drain(..) {
        self.cmd.add_tool_input(prev)?;
      }
    }

    let mut queue = if self.output_override {
      self.cmd.override_output(out.to_path_buf());
      state.prev_outputs.push(out.to_path_buf());
      CommandQueue::new(Some(out.to_path_buf()))
    } else {
      let o = self.cmd.get_output()
        .map(|v| v.to_path_buf() );
      if let Some(o) = o.as_ref() {
        out = o.to_path_buf();
        state.prev_outputs.push(out.clone());
      }
      CommandQueue::new(o)
    };

    info!("output: {}", out.display());

    self.cmd.enqueue_commands(&mut queue)?;
    queue.run_all(&mut self.cmd)?;

    self.copy_output_to(out)?;

    Ok(())
  }
  fn concrete(&mut self) -> &mut ConcreteCommand { &mut self.concrete }
}
impl<T> ICommand<T> for Command<FunctionCommand<T>>
  where T: ToolInvocation,
{
  fn run(&mut self, invoc: &mut &mut T,
         _state: &mut RunState) -> Result<(), CommandQueueError> {
    info!("on command: {:?} => {:?}", self.name, self.cmd);

    let f = self.cmd.0.take().unwrap();
    Ok((f)(invoc,)?)
  }
  fn concrete(&mut self) -> &mut ConcreteCommand { &mut self.concrete }
}
impl<T> ICommand<T> for Command<FunctionCommandWithState<T>>
  where T: ToolInvocation,
{
  fn run(&mut self, invoc: &mut &mut T,
         state: &mut RunState) -> Result<(), CommandQueueError> {
    info!("on command: {:?} => {:?}", self.name, self.cmd);

    let f = self.cmd.0.take().unwrap();
    Ok((f)(invoc, state)?)
  }
  fn concrete(&mut self) -> &mut ConcreteCommand { &mut self.concrete }
}
impl<U> ICommand<U> for Command<ExternalCommand> {
  fn run(&mut self, _: &mut &mut U,
         state: &mut RunState) -> Result<(), CommandQueueError> {
    let cant_fail = self.cant_fail;

    let out = state.output(&self.intermediate_name);

    if self.prev_outputs {
      if let Some(transform) = self.cmd.2.take() {
        let action = (transform)(&mut self.cmd.0, state.prev_outputs.as_ref());
        state.prev_outputs.clear();
        match action {
          InputArgsTransformResult::Skip => {
            if let Some(copy_to) = self.copy_output_to.as_ref() {
              state.prev_outputs.push(copy_to.clone());
            } else {
              // a temp dir is used otherwise, so we can't push anything to the outputs.
            }
            return Ok(());
          },
          InputArgsTransformResult::Normal => {},
        }

        info!("on command: {:?} => {:?}", self.name, self.cmd);
      } else {
        info!("on command: {:?} => {:?}", self.name, self.cmd);

        for prev in state.prev_outputs.drain(..) {
          self.cmd.0.arg(prev);
        }
      }
    }

    info!("output: {}", out.display());


    if let Some(ref out_arg) = self.cmd.1 {
      if self.output_override {
        state.prev_outputs.push(out.clone());
        self.cmd.0.arg(&out_arg[..]);
        self.cmd.0.arg(out.as_path());
      }

      let mut child = self.cmd.0.spawn()?;
      let result = child.wait()?;

      if !cant_fail && !result.success() {
        error!("command failed!");
        return Err(CommandQueueError::ProcessError(result.code()));
      }
    } else {
      let mut child = self.cmd.0.spawn()?;
      let result = child.wait()?;

      if !cant_fail && !result.success() {
        error!("command failed!");
        return Err(CommandQueueError::ProcessError(result.code()));
      }
    }

    self.copy_output_to(out)?;

    Ok(())
  }
  fn concrete(&mut self) -> &mut ConcreteCommand { &mut self.concrete }
}

pub trait ICommand<T>: Debug {
  fn run(&mut self, invoc: &mut &mut T,
         state: &mut RunState) -> Result<(), CommandQueueError>;
  fn concrete(&mut self) -> &mut ConcreteCommand;
}

#[derive(Debug)]
pub struct RunState<'q> {
  pub idx: usize,
  pub final_output: Option<&'q PathBuf>,
  pub prev_outputs: Vec<PathBuf>,
  pub intermediate: Option<TempDir>,
  pub is_last: bool,
  pub dry_run: bool,
}
impl<'q> RunState<'q> {
  fn new(final_output: Option<&'q PathBuf>) -> Result<RunState<'q>, Box<dyn Error>> {
    Ok(RunState {
      idx: 0,
      final_output,
      prev_outputs: Vec::new(),
      intermediate: Some(TempDir::new("wasm-driver-cmd-queue-intermediates")?),
      is_last: false,
      dry_run: false,
    })
  }

  pub fn output(&self, intermediate_name: &Option<PathBuf>) -> PathBuf {
    if self.is_last && self.final_output.is_some() {
      self.final_output.as_ref().unwrap().to_path_buf()
    } else if let &Some(ref name) = intermediate_name {
      self.intermediate.as_ref()
        .unwrap()
        .path()
        .join(name)
    } else {
      self.intermediate.as_ref()
        .unwrap()
        .path()
        .join(format!("{}", self.idx))
    }
  }
  pub fn is_dry_run(&self) -> bool { self.dry_run }
}
impl<'q> Drop for RunState<'q> {
  fn drop(&mut self) {
    if boolean_env("WASM_TOOLCHAIN_SAVE_TMPS") {
      let tmp = self.intermediate
        .take()
        .unwrap()
        .into_path();
      println!("Saving tmps in `{}`.", tmp.display());
    }
  }
}

#[derive(Debug)]
pub enum CommandQueueError {
  Error(Box<dyn Error>),
  ProcessError(Option<i32>),
}
impl From<String> for CommandQueueError {
  fn from(v: String) -> CommandQueueError {
    CommandQueueError::Error(From::from(v))
  }
}
impl From<Box<dyn Error>> for CommandQueueError {
  fn from(v: Box<dyn Error>) -> CommandQueueError {
    CommandQueueError::Error(v)
  }
}
impl From<std::io::Error> for CommandQueueError {
  fn from(v: std::io::Error) -> Self {
    CommandQueueError::Error(From::from(v))
  }
}
#[derive(Debug)]
pub struct CommandQueue<T> {
  pub final_output: Option<PathBuf>,

  queue: Vec<Box<dyn ICommand<T>>>,
  verbose: bool,
  dry_run: bool,
}

impl<T> CommandQueue<T>
  where T: ToolInvocation + 'static,
{
  pub fn new(final_output: Option<PathBuf>) -> CommandQueue<T> {
    CTRL_C_HANDLER.call_once(|| {
      use ctrlc::set_handler;
      let r = set_handler(|| {
        if STOP_BEFORE_NEXT_JOB.load(Ordering::SeqCst) {
          // exit now.
          ::std::process::exit(1);
        }
        STOP_BEFORE_NEXT_JOB.store(true, Ordering::SeqCst);
      });
      if r.is_err() {
        warn!("Couldn't set ctrl-c handler");
      }
    });

    CommandQueue {
      final_output,

      queue: Default::default(),
      verbose: false,
      dry_run: false,
    }
  }
  pub fn set_verbose(&mut self, v: bool) {
    self.verbose = v;
  }
  pub fn set_dry_run(&mut self, v: bool) {
    self.dry_run = v;
  }

  pub fn enqueue_external<U>(&mut self, name: Option<&'static str>,
                             mut cmd: process::Command,
                             output_arg: Option<&'static str>,
                             cant_fail: bool,
                             tmp_dirs: Option<Vec<U>>)
    -> &mut ConcreteCommand
    where U: Into<Rc<TempDir>>,
  {
    use std::process::{Stdio};

    cmd.stdout(Stdio::inherit())
      .stderr(Stdio::inherit())
      .stdin(Stdio::inherit());

    let kind =
      ExternalCommand(cmd, output_arg.map(|v| From::from(v) ), None);
    let concrete = ConcreteCommand {
      name: name.map(|v| v.into() ),
      cant_fail,
      tmp_dirs: tmp_dirs
        .map(|dirs| {
          dirs.into_iter()
            .map(|dir| dir.into() )
            .collect::<Vec<_>>()
        })
        .unwrap_or_default(),
      intermediate_name: None,
      prev_outputs: true,
      output_override: true,
      copy_output_to: None,
    };
    let command = Command {
      cmd: kind,
      concrete,
    };
    let command = box command;

    self.queue.push(command);
    self.queue.last_mut().unwrap().concrete()
  }

  pub fn enqueue_simple_external<U>(&mut self,
                                    name: Option<U>,
                                    mut cmd: process::Command,
                                    output_arg: Option<Cow<'static, str>>)
    -> &mut ConcreteCommand
    where U: Into<Cow<'static, str>>,
  {
    use std::process::{Stdio};

    cmd.stdout(Stdio::inherit())
      .stderr(Stdio::inherit())
      .stdin(Stdio::inherit());

    let kind = ExternalCommand(cmd, output_arg, None);
    let concrete = ConcreteCommand {
      name: name.map(|v| v.into() ),
      cant_fail: false,
      tmp_dirs: Default::default(),
      intermediate_name: None,
      prev_outputs: true,
      output_override: true,
      copy_output_to: None,
    };
    let command = Command {
      cmd: kind,
      concrete,
    };
    let command = box command;

    self.queue.push(command);
    self.queue.last_mut().unwrap().concrete()
  }

  pub fn enqueue_external_with_input_transform<F, U, V>(&mut self,
                                                        name: Option<U>,
                                                        mut cmd: process::Command,
                                                        output_arg: Option<V>,
                                                        f: F)
    -> &mut ConcreteCommand
    where F: FnOnce(&mut process::Command, &[PathBuf]) -> InputArgsTransformResult + 'static,
          U: Into<Cow<'static, str>>,
          V: Into<Cow<'static, str>>,
  {
    use std::process::{Stdio};

    cmd.stdout(Stdio::inherit())
      .stderr(Stdio::inherit())
      .stdin(Stdio::inherit());

    let f = box f as Box<_>;

    let kind = ExternalCommand(cmd, output_arg.map(|v| v.into() ), Some(f));
    let concrete = ConcreteCommand {
      name: name.map(|v| v.into() ),
      cant_fail: false,
      tmp_dirs: Default::default(),
      intermediate_name: None,
      prev_outputs: true,
      output_override: true,
      copy_output_to: None,
    };
    let command = Command {
      cmd: kind,
      concrete,
    };
    let command = box command;

    self.queue.push(command);
    self.queue.last_mut().unwrap().concrete()
  }

  pub fn enqueue_tool<U, V>(&mut self,
                            name: Option<&'static str>,
                            mut invocation: U, args: Vec<String>,
                            cant_fail: bool,
                            tmp_dirs: Option<Vec<V>>)
    -> Result<&mut ConcreteCommand, Box<dyn Error>>
    where U: ToolInvocation + 'static,
          V: Into<Rc<TempDir>>,
  {
    process_invocation_args(&mut invocation, args, true)?;

    let concrete = ConcreteCommand {
      name: name.map(|v| v.into() ),
      cant_fail,
      tmp_dirs: tmp_dirs
        .map(|dirs| {
          dirs.into_iter()
            .map(|dir| dir.into() )
            .collect::<Vec<_>>()
        })
        .unwrap_or_default(),
      intermediate_name: None,
      prev_outputs: true,
      output_override: true,
      copy_output_to: None,
    };
    let command = Command {
      cmd: CommandTool(invocation),
      concrete,
    };
    let command = box command;

    self.queue.push(command);

    Ok(self.queue.last_mut().unwrap().concrete())
  }
  pub fn enqueue_simple_tool<U>(&mut self,
                                name: Option<&'static str>,
                                invoc: U)
    -> &mut ConcreteCommand
    where U: ToolInvocation + 'static,
  {
    let concrete = ConcreteCommand {
      name: name.map(|v| v.into() ),
      cant_fail: false,
      tmp_dirs: vec![],
      intermediate_name: None,
      prev_outputs: true,
      output_override: true,
      copy_output_to: None,
    };
    let command = Command {
      cmd: CommandTool(invoc),
      concrete,
    };
    let command = box command;

    self.queue.push(command);

    self.queue.last_mut().unwrap().concrete()
  }
  pub fn enqueue_function<U, F>(&mut self,
                                name: Option<U>,
                                f: F)
    -> &mut ConcreteCommand
    where U: Into<Cow<'static, str>>,
          F: FnOnce(&mut &mut T) -> Result<(), CommandQueueError> + 'static,
  {
    let f_box = box f as Box<_>;
    let kind = FunctionCommand(Some(f_box));
    let concrete = ConcreteCommand {
      name: name.map(|v| v.into() ),
      cant_fail: false,
      tmp_dirs: Default::default(),
      intermediate_name: None,
      prev_outputs: false,
      output_override: false,
      copy_output_to: None,
    };
    let command = Command {
      cmd: kind,
      concrete,
    };
    let command = box command;

    self.queue.push(command);
    self.queue.last_mut().unwrap().concrete()
  }
  pub fn enqueue_state_function<U, F>(&mut self,
                                      name: Option<U>,
                                      f: F)
    -> &mut ConcreteCommand
    where U: Into<Cow<'static, str>>,
          F: FnOnce(&mut &mut T, &mut RunState) -> Result<(), CommandQueueError> + 'static,
  {
    let f_box = box f as Box<_>;
    let kind = FunctionCommandWithState(Some(f_box));
    let concrete = ConcreteCommand {
      name: name.map(|v| v.into() ),
      cant_fail: false,
      tmp_dirs: Default::default(),
      intermediate_name: None,
      prev_outputs: false,
      output_override: false,
      copy_output_to: None,
    };
    let command = Command {
      cmd: kind,
      concrete,
    };
    let command = box command;

    self.queue.push(command);
    self.queue.last_mut().unwrap().concrete()
  }

  pub fn enqueue_custom(&mut self, runner: Box<dyn ICommand<T>>)
    -> &mut ConcreteCommand
  {
    self.queue.push(runner);
    self.queue.last_mut()
      .unwrap()
      .concrete()
  }

  pub fn run_all(&mut self, mut invoc: &mut T) -> Result<(), CommandQueueError> {
    let cmd_len = self.queue.len();
    let iter =
      self.queue
        .drain(..)
        .enumerate()
        .map(|(idx, v)| {
          (idx == cmd_len - 1, idx, v)
        });

    let mut state =
      RunState::new(self.final_output.as_ref())?;
    for (is_last, idx, mut cmd) in iter {
      if STOP_BEFORE_NEXT_JOB.load(Ordering::SeqCst) {
        return Err(CommandQueueError::ProcessError(Some(1)));
      }
      state.dry_run = self.dry_run;
      state.is_last = is_last;
      state.idx = idx;

      cmd.run(&mut invoc, &mut state)?;
    }

    Ok(())
  }
}