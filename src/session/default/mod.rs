use std::sync::Arc;

use super::SessionManager;
use super::{Auth, SessionEntry, Unauth};
use crate::prelude::*;
use chashmap::CHashMap;
use tokio::time::sleep;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    expires: i64,
    user_id: SessionEntry,
}
#[derive(Default, Clone)]
pub struct DefaultManager(Arc<CHashMap<String, Entry>>);

#[async_trait]
impl SessionManager for DefaultManager {
    async fn create_auth(&self, session_id: &str, user_id: i32, time: Duration) -> Result {
        let entry = Entry {
            expires: time.as_secs() as i64,
            user_id: Auth(user_id),
        };
        self.0.insert(session_id.to_owned(), entry);
        Ok(())
    }
    async fn create_unauth(&self, session_id: &str, time: Duration) -> Result {
        let entry = Entry {
            expires: time.as_secs() as i64,
            user_id: Unauth,
        };
        self.0.insert(session_id.to_owned(), entry);
        Ok(())
    }

    async fn destroy(&self, session_id: &str) -> Option<SessionEntry> {
        Some(self.0.remove(session_id)?.user_id)
    }

    async fn get(&self, session_id: &str) -> Option<SessionEntry> {
        let session = self.0.get(session_id)?;
        Some(session.user_id)
    }

    async fn destroy_all(&self) -> Result {
        self.0.clear();
        Ok(())
    }

    async fn destroy_by_user(&self, user_id: i32) -> Result {
        let manager = self.clone();
        // this may be an expensive operation so we spawn blocking
        // in case there are too many sessions open
        tokio::task::spawn_blocking(move || async move {
            manager
                .0
                .retain(|_, value| value.expires > now() || value.user_id == Auth(user_id))
        });
        Ok(())
    }

    async fn init(self) {
        tokio::spawn(async move {
            loop {
                let time = now();
                self.0.retain(|_, entry| entry.expires > time);
                sleep(Duration::from_secs(24 * 60 * 60)).await;
            }
        });
    }
}
