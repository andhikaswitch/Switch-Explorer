//! Single instance (SYSTEM-INTEGRATION.md §4). The first process owns the pipe; later launches
//! forward their paths to it and exit, so a folder opened from anywhere lands in the running
//! window as a new tab.

use std::path::PathBuf;
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use sx_ipc::{IpcError, Message, Server, ServerHandle};
use sx_ui::OpenRequest;

use crate::args::Args;

/// How long a later launch waits for the running instance to take its request.
const FORWARD_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug)]
pub enum Role {
    /// This process shows the window; with a server when the pipe could be claimed.
    Primary(Option<Server>),
    /// The running instance took the request; exit.
    Forwarded,
}

pub fn claim(args: &Args) -> Role {
    // A benchmark or safe-mode run always gets its own fresh instance.
    if args.bench_open.is_some() || args.safe_mode {
        return Role::Primary(None);
    }
    match Server::bind() {
        Ok(server) => Role::Primary(Some(server)),
        Err(IpcError::AlreadyRunning) => {
            let absolute = |p: &PathBuf| std::path::absolute(p).unwrap_or_else(|_| p.clone());
            let request = Message::OpenPaths {
                paths: args.paths.iter().map(absolute).collect(),
                select: args.select.as_ref().map(absolute),
                new_window: false,
                home: args.home,
            };
            match sx_ipc::send(&request, FORWARD_TIMEOUT) {
                Ok(Message::Ack) => Role::Forwarded,
                Ok(reply) => {
                    tracing::warn!(
                        ?reply,
                        "running instance did not take the request; opening a new one"
                    );
                    Role::Primary(None)
                }
                Err(err) => {
                    tracing::warn!(error = %err, "running instance did not answer; opening a new one");
                    Role::Primary(None)
                }
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "single instance unavailable");
            Role::Primary(None)
        }
    }
}

/// Paths a single forwarded request may open; more than this is not a launch from Explorer or
/// a shortcut.
const MAX_FORWARDED_PATHS: usize = 64;

/// Serves forwarded requests into the workspace's channel. The handle stops the server.
pub fn serve(server: Server, requests: UnboundedSender<OpenRequest>) -> Option<ServerHandle> {
    // Ack only what reached the window: a refused request makes the sender open its own window.
    let deliver = move |request: OpenRequest| match requests.unbounded_send(request) {
        Ok(()) => Message::Ack,
        Err(_) => Message::Unknown,
    };
    let started = server.serve(move |message| match message {
        Message::OpenPaths {
            paths,
            select,
            home,
            ..
        } => {
            // The sender made its paths absolute; anything relative would resolve against our
            // working directory instead of the sender's.
            let paths = paths
                .into_iter()
                .filter(|p| p.is_absolute())
                .take(MAX_FORWARDED_PATHS)
                .collect();
            deliver(OpenRequest {
                paths,
                select: select.filter(|p| p.is_absolute()),
                home,
            })
        }
        Message::Activate => deliver(OpenRequest::default()),
        Message::Ping => Message::Pong,
        Message::Hello { .. } => Message::Ack,
        _ => Message::Unknown,
    });
    match started {
        Ok(handle) => Some(handle),
        Err(err) => {
            tracing::warn!(error = %err, "single instance server failed to start");
            None
        }
    }
}
