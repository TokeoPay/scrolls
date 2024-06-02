use gasket::framework::*;

use firestore_db_and_auth::{documents, errors, Credentials, JWKSet, ServiceSession};
use serde::Deserialize;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::debug;

use firestore_db_and_auth::jwt::download_google_jwks;

use crate::framework::*;

pub struct Worker {
    session: ServiceSession,
}

#[async_trait::async_trait(?Send)]
impl gasket::framework::Worker<Stage> for Worker {
    async fn bootstrap(stage: &Stage) -> Result<Self, WorkerError> {
        let credential_file = PathBuf::from_str(stage.config.credentials_file.as_str()).unwrap();

        let cred = Credentials::from_file(credential_file.to_str().unwrap())
            .await
            .map_err(|err| {
                println!("Error {:?}", err);
                debug!("Bad credentials file: {}", stage.config.credentials_file);
                WorkerError::Panic
            })?;

        // Only download the public keys once, and cache them.
        let jwkset = from_cache_file(
            credential_file.with_file_name("cached_jwks.jwks").as_path(),
            &cred,
        )
        .await
        .map_err(|err| {
            debug!("JWK from_cache_file error: {:?}", err);
            WorkerError::Panic
        })?;
        cred.add_jwks_public_keys(&jwkset).await;
        cred.verify().await.map_err(|err| {
            debug!("Verification Err: {:?}", err);
            WorkerError::Panic
        })?;

        let session = ServiceSession::new(cred).await.unwrap();

        Ok(Self { session })
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
                for command in commands {
                    match command {
                        model::CRDTCommand::SetAdd(key, child, value) => documents::write(
                            &self.session,
                            format!("/Address/{}/Utxo/", key).as_str(),
                            child,
                            &value,
                            documents::WriteOptions {
                                ..Default::default()
                            },
                        )
                        .await
                        .map(|_| ())
                        .map_err(|err| {
                            println!("{:?}", err);
                            debug!(key, value, "SetAdd Error");
                            WorkerError::Retry
                        })?,
                        model::CRDTCommand::SetRemove(key, child, value) => documents::delete(
                            &self.session,
                            format!("/Address/{}/Utxo/{}", key, child.unwrap()).as_str(),
                            false,
                        )
                        .await
                        .map(|_| ())
                        .map_err(|err| {
                            println!("{:?}", err);
                            debug!(key, value, "SetAdd Error");
                            WorkerError::Retry
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
#[stage(name = "firebase", unit = "ChainEvent", worker = "Worker")]
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
    //TODO: Any config options need to be put here
    // For example: Base Collection Name?
    credentials_file: String,
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

/// Download the two public key JWKS files if necessary and cache the content at the given file path.
/// Only use this option in cloud functions if the given file path is persistent.
/// You can use [`Credentials::add_jwks_public_keys`] to manually add more public keys later on.
pub async fn from_cache_file(
    cache_file: &std::path::Path,
    c: &Credentials,
) -> errors::Result<JWKSet> {
    use std::fs::File;
    use std::io::BufReader;

    Ok(if cache_file.exists() {
        let f = BufReader::new(File::open(cache_file)?);
        let jwks_set: JWKSet = serde_json::from_reader(f)?;
        jwks_set
    } else {
        // If not present, download the two jwks (specific service account + google system account),
        // merge them into one set of keys and store them in the cache file.
        let jwk_set_1 = download_google_jwks(&c.client_email).await?;
        let jwk_set_2 = download_google_jwks("securetoken@system.gserviceaccount.com").await?;

        let mut jwks = JWKSet::new(&jwk_set_1.0)?;
        jwks.keys.append(&mut JWKSet::new(&jwk_set_2.0)?.keys);
        let f = File::create(cache_file)?;
        serde_json::to_writer_pretty(f, &jwks)?;
        jwks
    })
}
