use gasket::{messaging::RecvPort, runtime::Tether};
use serde::Deserialize;

use crate::framework::{errors::Error, *};

pub mod firebase;
pub mod redis;
pub mod sqlite;

pub enum Bootstrapper {
    Redis(redis::Stage),
    Sqlite(sqlite::Stage),
    Firebase(firebase::Stage),
}

impl StageBootstrapper for Bootstrapper {
    fn connect_output(&mut self, _: OutputAdapter) {
        panic!("attempted to use storage stage as sender");
    }

    fn connect_input(&mut self, adapter: InputAdapter) {
        match self {
            Bootstrapper::Redis(p) => p.input.connect(adapter),
            Bootstrapper::Sqlite(p) => p.input.connect(adapter),
            Bootstrapper::Firebase(p) => p.input.connect(adapter),
        }
    }

    fn spawn(self, policy: gasket::runtime::Policy) -> Tether {
        match self {
            Bootstrapper::Redis(s) => gasket::runtime::spawn_stage(s, policy),
            Bootstrapper::Sqlite(s) => gasket::runtime::spawn_stage(s, policy),
            Bootstrapper::Firebase(s) => gasket::runtime::spawn_stage(s, policy),
        }
    }
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum Config {
    Redis(redis::Config),
    Sqlite(sqlite::Config),
    Firebase(firebase::Config),
}

impl Config {
    pub fn bootstrapper(self, ctx: &Context) -> Result<Bootstrapper, Error> {
        match self {
            Config::Redis(c) => Ok(Bootstrapper::Redis(c.bootstrapper(ctx)?)),
            Config::Sqlite(c) => Ok(Bootstrapper::Sqlite(c.bootstrapper(ctx)?)),
            Config::Firebase(c) => Ok(Bootstrapper::Firebase(c.bootstrapper(ctx)?)),
        }
    }
}
