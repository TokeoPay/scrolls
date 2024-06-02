use gasket::framework::*;
use rusqlite::{Connection, Params, Result};

use serde::Deserialize;
// use std::path::PathBuf;
// use std::str::FromStr;
// use tracing::debug;

// use firestore_db_and_auth::jwt::download_google_jwks;

use crate::framework::*;

pub struct Worker {
    conn: Connection,
    table_name: String,
}

fn sqlite_exec<P: Params>(conn: &Connection, sql: String, params: P) -> Result<(), WorkerError> {
    conn.execute(sql.as_str(), params)
        .map(|_| ())
        .map_err(|err| {
            print!("{:?}", err);
            WorkerError::Panic
        })
}
#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(stage: &Stage) -> Result<Self, WorkerError> {
        let conn = Connection::open(&stage.config.db_path).map_err(|err| {
            println!("{:?}", err);
            WorkerError::Panic
        })?;

        sqlite_exec(
            &conn,
            format!(
                "CREATE table if not exists {} (
                  id     INTEGER primary key,
                  key    TEXT NOT NULL,
                  child  TEXT,
                  data   TEXT,
                  CONSTRAINT unq UNIQUE(key, child, data)
              )",
                stage.config.table_name
            ),
            (),
        )?;

        sqlite_exec(
            &conn,
            format!(
                "CREATE index if not exists key_idx ON {} (key)",
                stage.config.table_name
            ),
            (),
        )?;
        sqlite_exec(
            &conn,
            format!(
                "CREATE index if not exists key_child_idx ON {} (key, child)",
                stage.config.table_name
            ),
            (),
        )?;

        Ok(Self {
            conn,
            table_name: stage.config.table_name.clone(),
        })
    }

    async fn schedule(
        &mut self,
        stage: &mut Stage,
    ) -> Result<WorkSchedule<ChainEvent>, WorkerError> {
        let msg = stage.input.recv().await.or_panic()?;
        Ok(WorkSchedule::Unit(msg.payload))
    }

    async fn execute(&mut self, unit: &ChainEvent, stage: &mut Stage) -> Result<(), WorkerError> {
        let point = unit.point().clone();
        let record = unit.record().cloned();

        if record.is_none() {
            return Ok(());
        }

        let record = record.unwrap();

        match record {
            Record::CRDTCommand(commands) => {
                let tx = self.conn.transaction().map_err(|err| {
                    print!("{:?}", err);
                    WorkerError::Restart
                })?;

                for command in commands {
                    match command {
                        model::CRDTCommand::SetAdd(key, child, value) => tx
                            .execute(
                                format!(
                                    "INSERT OR IGNORE INTO {} (key, child, data) VALUES (?1, ?2, ?3)",
                                    self.table_name
                                )
                                .as_str(),
                                (&key, &child, &value),
                            )
                            .map(|_| ())
                            .map_err(|err| {
                                print!("{:?}", err);
                                WorkerError::Restart
                            })?,
                        model::CRDTCommand::SetRemove(key, child, value) => match child {
                            // If we have a child then we can use this as a direct key into the record to remove.
                            Some(c) => tx.execute(
                                format!(
                                    "DELETE FROM {} WHERE key = ?1 and child = ?2",
                                    self.table_name
                                )
                                .as_str(),
                                (key, c),
                            ),
                            // Otherwise we need to use the Data. This is probably slower but I'm not using it.
                            None => tx.execute(
                                format!(
                                    "DELETE FROM {} WHERE key = ?1 and data = ?2",
                                    self.table_name
                                )
                                .as_str(),
                                (key, value),
                            ),
                        }
                        .map(|_| ())
                        .map_err(|err| {
                            print!("{:?}", err);
                            WorkerError::Restart
                        })?,
                        model::CRDTCommand::SortedSetAdd(_, _, _) => todo!(),
                        model::CRDTCommand::SortedSetRemove(_, _, _) => todo!(),
                        model::CRDTCommand::TwoPhaseSetAdd(_, _) => todo!(),
                        model::CRDTCommand::TwoPhaseSetRemove(_, _) => todo!(),
                        model::CRDTCommand::GrowOnlySetAdd(_, _) => todo!(),
                        model::CRDTCommand::LastWriteWins(_, _, _) => todo!(),
                        model::CRDTCommand::AnyWriteWins(_, _) => todo!(),
                        model::CRDTCommand::PNCounter(_, _) => todo!(),
                        model::CRDTCommand::HashCounter(_, _, _) => todo!(),
                        model::CRDTCommand::HashSetValue(_, _, _) => todo!(),
                        model::CRDTCommand::HashUnsetKey(_, _) => todo!(),
                    }
                }

                tx.commit().map_err(|err| {
                    print!("{:?}", err);
                    WorkerError::Retry
                })?;
            }
            Record::RawBlockPayload(_) => todo!(),
            Record::EnrichedBlockPayload(_, _) => todo!(),
        }

        stage.ops_count.inc(1);
        stage.latest_block.set(point.slot_or_default() as i64);
        stage.cursor.add_breadcrumb(point.clone());
        Ok(())
    }
}

#[derive(Stage)]
#[stage(name = "sqlite", unit = "ChainEvent", worker = "Worker")]
pub struct Stage {
    config: Config,
    cursor: Cursor,

    pub input: StorageInputPort,

    #[metric]
    ops_count: gasket::metrics::Counter,

    #[metric]
    latest_block: gasket::metrics::Gauge,
}

#[derive(Default, Deserialize)]
pub struct Config {
    db_path: String,
    table_name: String,
}

impl Config {
    pub fn bootstrapper(self, ctx: &Context) -> Result<Stage, Error> {
        let stage = Stage {
            config: self,
            cursor: ctx.cursor.clone(),
            ops_count: Default::default(),
            latest_block: Default::default(),
            input: Default::default(),
        };

        Ok(stage)
    }
}
