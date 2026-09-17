//! Background fetch service: pulls TLE catalogs from all sources on a timer.

use crate::data::fetch::{self, SOURCES};
use crate::data::model::{Sat, SatGroup};
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Messages from the fetch service to the UI thread.
#[derive(Debug)]
pub enum FetchMsg {
    /// A source finished: (source name, sats or error).
    SourceDone {
        source: &'static str,
        result: Result<Vec<Sat>, String>,
    },
}

/// Shared fetch status for the UI.
#[derive(Debug, Clone, Default)]
pub struct FetchStatus {
    pub sources_done: usize,
    pub sources_total: usize,
    pub total_sats: usize,
    pub last_error: Option<String>,
}

/// Spawn the background fetch loop. Returns the receiver for UI messages.
pub fn spawn(
    runtime: &tokio::runtime::Runtime,
    status: Arc<RwLock<FetchStatus>>,
    catalog: Arc<RwLock<Vec<Sat>>>,
) -> mpsc::UnboundedReceiver<FetchMsg> {
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = runtime.handle().clone();

    status.write().sources_total = SOURCES.len();

    // Map source name to its default group.
    let groups: &[(&str, SatGroup)] = &[
        ("Celestrak-Station", SatGroup::Station),
        ("Celestrak-Weather", SatGroup::Weather),
        ("Celestrak-GPS", SatGroup::Navigation),
        ("Celestrak-Science", SatGroup::Science),
        ("Celestrak-Active", SatGroup::Other),
        ("Celestrak-Geo", SatGroup::Communications),
    ];

    for src in SOURCES {
        let tx = tx.clone();
        let status = Arc::clone(&status);
        let catalog = Arc::clone(&catalog);
        let group = groups
            .iter()
            .find(|(n, _)| *n == src.name)
            .map(|(_, g)| *g)
            .unwrap_or(SatGroup::Other);

        handle.spawn(async move {
            let result = fetch::celestrak::fetch_source(src, group)
                .await
                .map_err(|e| format!("{}: {e:#}", src.name));

            let _count = result.as_ref().map(|s| s.len()).unwrap_or(0);

            match &result {
                Ok(sats) => {
                    catalog.write().extend(sats.iter().cloned());
                }
                Err(e) => {
                    status.write().last_error = Some(e.clone());
                }
            }

            let mut st = status.write();
            st.sources_done += 1;
            st.total_sats = catalog.read().len();
            drop(st);

            let _ = tx.send(FetchMsg::SourceDone {
                source: src.name,
                result: result.map_err(|e| e),
            });
        });
    }

    rx
}
