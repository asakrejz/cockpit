use super::*;
use cockpit_core::{Action, Actor, AgentOptions, demo};

mod jobs;
mod prerequisites;
mod protocol;
mod watcher;

const T: u64 = 1_700_000_000_000;

fn start(ws: &mut Workspace, plan_first: bool) {
    ws.apply(
        Action::StartAgent {
            ticket: "BAU-142".into(),
            options: AgentOptions {
                plan_first,
                ..AgentOptions::default()
            },
        },
        Actor::User,
        T,
    )
    .unwrap();
}

fn info(mode: Mode) -> Info {
    Info {
        mode,
        paused: false,
        question_open: false,
        hints_sent: 0,
    }
}

fn running(m: Mode) -> HashMap<String, Info> {
    HashMap::from([("BAU-142".to_string(), info(m))])
}
