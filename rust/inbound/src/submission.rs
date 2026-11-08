use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::watch;

pub const SUBMISSION_TIMEOUT: Duration = Duration::from_secs(600);
const CANCELLED: &str =
    "Report submission canceled. Delivery could not be confirmed; check before sending again.";
type Key = (String, String);
struct Entry {
    cancel: watch::Sender<bool>,
    started: bool,
}
#[derive(Clone, Default)]
pub(crate) struct Submissions(Arc<Mutex<HashMap<Key, Entry>>>);
struct Guard {
    submissions: Submissions,
    key: Key,
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.submissions.0.lock().unwrap().remove(&self.key);
    }
}

impl Submissions {
    pub fn register(&self, owner: &str, id: &str) -> Result<(), String> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("Invalid report request ID.".into());
        }
        let mut entries = self.0.lock().unwrap();
        if entries.contains_key(&(owner.into(), id.into()))
            || entries.keys().filter(|(window, _)| window == owner).count() >= 16
        {
            return Err("A report request is already pending.".into());
        }
        entries.insert(
            (owner.into(), id.into()),
            Entry {
                cancel: watch::channel(false).0,
                started: false,
            },
        );
        Ok(())
    }
    pub fn cancel(&self, owner: &str, id: &str) {
        let mut entries = self.0.lock().unwrap();
        let key = (owner.into(), id.into());
        if let Some(entry) = entries.get(&key) {
            entry.cancel.send_replace(true);
            if !entry.started {
                entries.remove(&key);
            }
        }
    }
    pub fn cancel_owner(&self, owner: &str) {
        let mut entries = self.0.lock().unwrap();
        entries.retain(|(window, _), entry| {
            if window != owner {
                return true;
            }
            entry.cancel.send_replace(true);
            entry.started
        });
    }
    pub async fn run<T>(
        &self,
        owner: &str,
        id: &str,
        future: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        let key = (owner.into(), id.into());
        let mut cancelled = {
            let mut entries = self.0.lock().unwrap();
            let entry = entries.get_mut(&key).ok_or_else(|| CANCELLED.to_owned())?;
            if entry.started {
                return Err("This report is already being sent.".into());
            }
            entry.started = true;
            entry.cancel.subscribe()
        };
        let _guard = Guard {
            submissions: self.clone(),
            key,
        };
        if *cancelled.borrow() {
            return Err(CANCELLED.into());
        }
        tokio::select! {
            biased;
            _ = cancelled.changed() => Err(CANCELLED.into()),
            result = bounded(future) => result,
        }
    }
}

pub(crate) async fn bounded<T>(
    future: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::time::timeout(SUBMISSION_TIMEOUT, future).await
        .map_err(|_| "Report submission timed out. Delivery could not be confirmed; check before sending again.".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cancellation_is_owned_and_releases_the_request() {
        let submissions = Submissions::default();
        submissions.register("main", "report-1").unwrap();
        let pending = submissions.run(
            "main",
            "report-1",
            std::future::pending::<Result<(), String>>(),
        );
        tokio::pin!(pending);
        tokio::select! { result = &mut pending => panic!("unexpected completion: {result:?}"), _ = tokio::task::yield_now() => {} }
        submissions.cancel("other", "report-1");
        assert!(!submissions.0.lock().unwrap().is_empty());
        submissions.cancel("main", "report-1");
        assert!(pending
            .await
            .unwrap_err()
            .contains("Delivery could not be confirmed"));
        assert!(submissions.0.lock().unwrap().is_empty());
    }
    #[tokio::test(start_paused = true)]
    async fn deadline_ends_a_stalled_submission_without_resending() {
        let started = tokio::time::Instant::now();
        let result = bounded(std::future::pending::<Result<(), String>>()).await;
        assert!(result
            .unwrap_err()
            .contains("Delivery could not be confirmed"));
        assert_eq!(started.elapsed(), SUBMISSION_TIMEOUT);
    }
    #[tokio::test]
    async fn cancellation_before_submission_does_not_send() {
        let submissions = Submissions::default();
        submissions.register("main", "report-1").unwrap();
        submissions.cancel("main", "report-1");
        assert!(submissions
            .run("main", "report-1", async {
                panic!("transport must not run");
                #[allow(unreachable_code)]
                Ok::<_, String>(())
            })
            .await
            .is_err());
    }
}
