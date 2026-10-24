//! Command-line parsing, pure and on OS strings so paths need not be UTF-8.

use std::{ffi::OsString, path::PathBuf};

use crate::agents::Agent;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("name an agent ({}) or give a command after `--`", Agent::names())]
    MissingAgent,
    #[error("unknown agent `{0}`; known agents are {names}", names = Agent::names())]
    UnknownAgent(String),
    #[error("unknown option `{}`", .0.to_string_lossy())]
    UnknownOption(OsString),
    #[error("`{0}` needs a value")]
    MissingValue(&'static str),
    #[error("`{0}` must be valid UTF-8")]
    NotUtf8(&'static str),
    #[error("give the agent command after `--`")]
    EmptyCommand,
    #[error("`connect` needs a chat's endpoint id")]
    MissingEndpoint,
    #[error("`{0}` is not a chat endpoint id")]
    InvalidEndpoint(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Parsed {
    Run(Options),
    /// Carry a client connection to a running chat over stdin and stdout.
    Connect(herdr_chat_protocol::EndpointId),
    /// List the agents installed here, one name per line.
    Agents,
    Help,
    Version,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Options {
    /// Reach a built-in agent through its ACP adapter even where `herdr-acp`
    /// can speak to the agent itself.
    pub adapter: bool,
    pub agent: AgentChoice,
    /// Listen for Herdr UI and other clients.
    pub attach: bool,
    pub cwd: Option<PathBuf>,
    /// Run without the in-pane chat view.
    pub headless: bool,
    /// The agent's session to continue instead of starting a new one.
    pub load: Option<String>,
    /// The display name for a custom command.
    pub name: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AgentChoice {
    Builtin {
        agent: Agent,
        args: Vec<OsString>,
    },
    Command {
        program: OsString,
        args: Vec<OsString>,
    },
}

pub(crate) const HELP: &str = "\
Runs an ACP agent in this terminal and shares the conversation with Herdr UI.

Usage:
  herdr-acp [OPTIONS] AGENT [-- AGENT_ARGS...]
  herdr-acp [OPTIONS] -- COMMAND [ARGS...]
  herdr-acp connect ID   Attach stdin and stdout to the running chat ID
                         (Herdr UI runs this over SSH for remote chats)
  herdr-acp agents       List the agents installed here, one per line

Agents:
  claude     claude itself, or with --acp claude-agent-acp
             (or npx @agentclientprotocol/claude-agent-acp)
  codex      codex app-server, or with --acp codex-acp
             (or npx @agentclientprotocol/codex-acp)
  copilot    copilot --acp
  gemini     gemini --acp
  opencode   opencode acp
  pi         pi --mode rpc, or with --acp pi-acp (or npx pi-acp)

Options:
  --acp          Use the agent's ACP adapter, not the agent itself
                 (also HERDR_ACP_ADAPTER=1)
  --cwd DIR      Run the session in DIR instead of the current directory
  --load ID      Continue the agent's session ID instead of starting a new one
  --name NAME    The name to show for a custom command
  --headless     Draw nothing; clients such as Herdr UI drive the session
  --no-attach    Do not listen for Herdr UI
  -h, --help     Show this help
  -V, --version  Show the version

Keys:
  Enter          Send the prompt, or queue it while the agent works
  Alt+Enter      New line
  Esc            Cancel the running turn
  /              Complete the agent's commands: Tab takes one, Up/Down choose
  Up/Down        Recall earlier prompts from an empty prompt
  Shift+Tab      Next session mode
  Ctrl+O         Settings: model, thinking level, mode (Left/Right change)
  PgUp/PgDn      Scroll
  Ctrl+D         Quit when the prompt is empty
";

pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Parsed, Error> {
    let mut args = args.into_iter().peekable();
    if args.peek().and_then(|arg| arg.to_str()) == Some("agents") {
        args.next();
        return match args.next() {
            Some(extra) => Err(Error::UnknownOption(extra)),
            None => Ok(Parsed::Agents),
        };
    }
    if args.peek().and_then(|arg| arg.to_str()) == Some("connect") {
        args.next();
        let id = utf8(args.next(), "connect").map_err(|_| Error::MissingEndpoint)?;
        if let Some(extra) = args.next() {
            return Err(Error::UnknownOption(extra));
        }
        return id
            .parse()
            .map(Parsed::Connect)
            .map_err(|_| Error::InvalidEndpoint(id));
    }
    let mut options = Options {
        adapter: false,
        agent: AgentChoice::Command {
            program: OsString::new(),
            args: Vec::new(),
        },
        attach: true,
        cwd: None,
        headless: false,
        load: None,
        name: None,
    };
    let mut agent = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => return Ok(Parsed::Help),
            Some("-V" | "--version") => return Ok(Parsed::Version),
            Some("--acp") => options.adapter = true,
            Some("--headless") => options.headless = true,
            Some("--no-attach") => options.attach = false,
            Some("--cwd") => {
                options.cwd = Some(args.next().ok_or(Error::MissingValue("--cwd"))?.into());
            }
            Some("--load") => options.load = Some(utf8(args.next(), "--load")?),
            Some("--name") => options.name = Some(utf8(args.next(), "--name")?),
            Some("--") => {
                let rest: Vec<OsString> = args.by_ref().collect();
                options.agent = match agent {
                    Some(agent) => AgentChoice::Builtin { agent, args: rest },
                    None => {
                        let mut rest = rest.into_iter();
                        let program = rest.next().ok_or(Error::EmptyCommand)?;
                        AgentChoice::Command {
                            program,
                            args: rest.collect(),
                        }
                    }
                };
                return Ok(Parsed::Run(options));
            }
            Some(name) if !name.starts_with('-') && agent.is_none() => {
                agent = Some(
                    name.parse::<Agent>()
                        .map_err(|_| Error::UnknownAgent(name.to_owned()))?,
                );
            }
            _ => return Err(Error::UnknownOption(arg)),
        }
    }
    let agent = agent.ok_or(Error::MissingAgent)?;
    options.agent = AgentChoice::Builtin {
        agent,
        args: Vec::new(),
    };
    Ok(Parsed::Run(options))
}

fn utf8(value: Option<OsString>, option: &'static str) -> Result<String, Error> {
    value
        .ok_or(Error::MissingValue(option))?
        .into_string()
        .map_err(|_| Error::NotUtf8(option))
}

#[cfg(test)]
mod tests;
