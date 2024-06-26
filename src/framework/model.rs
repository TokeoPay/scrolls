use std::{collections::HashMap, fmt::Debug};

use pallas::ledger::traverse::{Era, MultiEraOutput, MultiEraTx, OutputRef};
use serde::Deserialize;

use crate::crosscut::policies::{AppliesPolicy, RuntimePolicy};

use super::errors::Error;

#[derive(Default, Debug, Clone)]
pub struct BlockContext {
    utxos: HashMap<String, (Era, Vec<u8>)>,
}
impl BlockContext {
    pub fn import_ref_output(&mut self, key: &OutputRef, era: Era, cbor: Vec<u8>) {
        self.utxos.insert(key.to_string(), (era, cbor));
    }

    pub fn find_utxo(&self, key: &OutputRef) -> Result<MultiEraOutput, Error> {
        let (era, cbor) = self
            .utxos
            .get(&key.to_string())
            .ok_or_else(|| Error::missing_utxo(key))?;

        MultiEraOutput::decode(*era, cbor).map_err(Error::cbor)
    }

    pub fn get_all_keys(&self) -> Vec<String> {
        self.utxos.keys().map(|x| x.clone()).collect()
    }

    pub fn find_consumed_txos(
        &self,
        tx: &MultiEraTx,
        policy: &RuntimePolicy,
    ) -> Result<Vec<(OutputRef, MultiEraOutput)>, Error> {
        let items = tx
            .consumes()
            .iter()
            .map(|i| i.output_ref())
            .map(|r| self.find_utxo(&r).map(|u| (r, u)))
            .map(|r| r.apply_policy(policy))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        Ok(items)
    }
}

pub type ChildSet = Option<String>;
pub type Set = String;
pub type Member = String;
pub type Key = String;
pub type Delta = i64;
pub type Timestamp = u64;

#[derive(Clone, Debug, Deserialize)]
pub enum Value {
    String(String),
    BigInt(i128),
    Cbor(Vec<u8>),
    Json(serde_json::Value),
}

impl From<String> for Value {
    fn from(x: String) -> Self {
        Value::String(x)
    }
}

impl From<Vec<u8>> for Value {
    fn from(x: Vec<u8>) -> Self {
        Value::Cbor(x)
    }
}

impl From<serde_json::Value> for Value {
    fn from(x: serde_json::Value) -> Self {
        Value::Json(x)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[non_exhaustive]
pub enum CRDTCommand {
    SetAdd(Set, ChildSet, Member),
    SetRemove(Set, ChildSet, Member),
    SortedSetAdd(Set, Member, Delta),
    SortedSetRemove(Set, Member, Delta),
    TwoPhaseSetAdd(Set, Member),
    TwoPhaseSetRemove(Set, Member),
    GrowOnlySetAdd(Set, Member),
    LastWriteWins(Key, Value, Timestamp),
    AnyWriteWins(Key, Value),
    // TODO make sure Value is a generic not stringly typed
    PNCounter(Key, Delta),
    HashCounter(Key, Member, Delta),
    HashSetValue(Key, Member, Value),
    HashUnsetKey(Key, Member),
}

impl CRDTCommand {
    pub fn set_add(
        prefix: Option<&str>,
        key: &str,
        child: Option<&str>,
        member: String,
    ) -> CRDTCommand {
        let key = match prefix {
            Some(prefix) => format!("{}.{}", prefix, key),
            None => key.to_string(),
        };

        let child = match (prefix, child) {
            (Some(prefix), Some(parent)) => Some(format!("{}.{}", prefix, parent)),
            (None, Some(parent)) => Some(format!("{}", parent)),
            _ => None,
        };

        CRDTCommand::SetAdd(key, child, member)
    }

    pub fn set_remove(
        prefix: Option<&str>,
        key: &str,
        child: Option<&str>,
        member: String,
    ) -> CRDTCommand {
        let key = match prefix {
            Some(prefix) => format!("{}.{}", prefix, key),
            None => key.to_string(),
        };

        let child = match (prefix, child) {
            (Some(prefix), Some(child)) => Some(format!("{}.{}", prefix, child)),
            (None, Some(child)) => Some(format!("{}", child)),
            _ => None,
        };

        CRDTCommand::SetRemove(key, child, member)
    }

    pub fn sorted_set_add(
        prefix: Option<&str>,
        key: &str,
        member: String,
        delta: i64,
    ) -> CRDTCommand {
        let key = match prefix {
            Some(prefix) => format!("{}.{}", prefix, key),
            None => key.to_string(),
        };

        CRDTCommand::SortedSetAdd(key, member, delta)
    }

    pub fn sorted_set_remove(
        prefix: Option<&str>,
        key: &str,
        member: String,
        delta: i64,
    ) -> CRDTCommand {
        let key = match prefix {
            Some(prefix) => format!("{}.{}", prefix, key),
            None => key.to_string(),
        };

        CRDTCommand::SortedSetRemove(key, member, delta)
    }

    pub fn any_write_wins<K, V>(prefix: Option<&str>, key: K, value: V) -> CRDTCommand
    where
        K: ToString,
        V: Into<Value>,
    {
        let key = match prefix {
            Some(prefix) => format!("{}.{}", prefix, key.to_string()),
            None => key.to_string(),
        };

        CRDTCommand::AnyWriteWins(key, value.into())
    }

    pub fn last_write_wins<V>(
        prefix: Option<&str>,
        key: &str,
        value: V,
        ts: Timestamp,
    ) -> CRDTCommand
    where
        V: Into<Value>,
    {
        let key = match prefix {
            Some(prefix) => format!("{}.{}", prefix, key),
            None => key.to_string(),
        };

        CRDTCommand::LastWriteWins(key, value.into(), ts)
    }

    pub fn hash_set_value<V>(
        prefix: Option<&str>,
        key: &str,
        member: String,
        value: V,
    ) -> CRDTCommand
    where
        V: Into<Value>,
    {
        let key = match prefix {
            Some(prefix) => format!("{}.{}", prefix, key.to_string()),
            None => key.to_string(),
        };

        CRDTCommand::HashSetValue(key, member, value.into())
    }

    pub fn hash_del_key(prefix: Option<&str>, key: &str, member: String) -> CRDTCommand {
        let key = match prefix {
            Some(prefix) => format!("{}.{}", prefix, key.to_string()),
            None => key.to_string(),
        };

        CRDTCommand::HashUnsetKey(key, member)
    }

    pub fn hash_counter(
        prefix: Option<&str>,
        key: &str,
        member: String,
        delta: i64,
    ) -> CRDTCommand {
        let key = match prefix {
            Some(prefix) => format!("{}.{}", prefix, key.to_string()),
            None => key.to_string(),
        };

        CRDTCommand::HashCounter(key, member, delta)
    }
}
