//! 같은 GET 합치기(리더/대기자)와 stale 재검증 게이트.

use std::sync::{Arc, Weak};

use cview_core::error::ApiError;

use super::{
    AgedResponse, ChzzkApiClient, ClientState, EndpointSnapshot, GetWaitEntry, Inner, LeaderFinish,
    RequestOptions, ResponseBody, RevalidationRun, SharedGet, StaleRevalidation,
    STALE_REVALIDATION_CONCURRENCY, STALE_REVALIDATION_QUEUE_LIMIT,
};
use crate::cache::Lookup;
use crate::wait::{GetWait, WaitTicket};

impl ChzzkApiClient {
    pub(super) async fn shared_get<T: ResponseBody>(
        &self,
        endpoint: Arc<EndpointSnapshot>,
        shared: SharedGet,
    ) -> Result<AgedResponse<T>, ApiError> {
        let (ticket, is_leader) = {
            let mut st = self.lock_state();
            if let Some(existing) = st.get_waits.get(&shared.key) {
                (existing.wait.reserve(), false)
            } else {
                let weak: Weak<Inner> = Arc::downgrade(&self.inner);
                let abandon_key = shared.key.clone();
                let wait = GetWait::new(Some(Box::new(move |w: &GetWait| {
                    if let Some(inner) = weak.upgrade() {
                        ChzzkApiClient { inner }.abandon_get_wait(&abandon_key, w);
                    }
                })));
                let ticket = wait.reserve();
                let client = self.clone();
                let task_wait = wait.clone();
                let task_shared = shared.clone();
                // 스폰은 락 안에서 한다 — 작업이 끝나 기록을 지우는 것이 등록보다 앞설 수 없다.
                let task = tokio::spawn(async move {
                    let _finish = LeaderFinish {
                        client: client.clone(),
                        key: task_shared.key.clone(),
                        wait: task_wait.clone(),
                    };
                    let options = RequestOptions {
                        bypass_cache: task_shared.bypass_cache,
                        store_in_cache: task_shared.store_in_cache,
                        joined: true,
                        purpose: task_shared.purpose,
                    };
                    match Self::boxed_request::<T>(client.clone(), endpoint, options).await {
                        Ok(value) => task_wait.succeed(Arc::new(value), None),
                        Err(error) => {
                            // bypass 리더·취소·인증·4xx·디코드 오류는 이전 값으로 덮지 않는다.
                            let fallback = if !task_shared.bypass_cache
                                && Self::allows_stale_on_error(&error)
                            {
                                client.stale_cached::<T>(&task_shared.key, task_shared.stale_window)
                            } else {
                                None
                            };
                            match fallback {
                                Some(cached) => task_wait.succeed(Arc::new(cached), Some(error)),
                                None => task_wait.fail(error),
                            }
                        }
                    }
                });
                st.running_leaders += 1;
                st.get_waits
                    .insert(shared.key.clone(), GetWaitEntry { wait, task });
                (ticket, true)
            }
        };
        let _ = is_leader;
        self.await_shared_get::<T>(ticket, &shared).await
    }

    async fn await_shared_get<T: ResponseBody>(
        &self,
        ticket: WaitTicket,
        shared: &SharedGet,
    ) -> Result<AgedResponse<T>, ApiError> {
        // bypassCache 호출자는 오류 뒤 이전 값을 받지 않는다.
        let any = ticket.value(!shared.bypass_cache).await?;
        let typed = any
            .downcast::<AgedResponse<T>>()
            .map_err(|_| ApiError::DecodingFailed("공유 응답 타입이 다릅니다".into()))?;
        // 기다리는 사이 로그인 세대가 바뀌었으면 이전 계정의 값이다.
        self.check_auth_context(shared.context).await?;
        Ok((*typed).clone())
    }

    /// 공유 작업이 끝나면 부른다. 신원이 같을 때만 지운다.
    pub(super) fn finish_get_wait(&self, key: &str, wait: &GetWait) {
        let mut st = self.lock_state();
        st.running_leaders = st.running_leaders.saturating_sub(1);
        if st.get_waits.get(key).is_some_and(|e| e.wait.ptr_eq(wait)) {
            st.get_waits.remove(key);
        }
    }

    /// 기다리는 호출자가 모두 떠난 공유 GET 을 멈춘다.
    fn abandon_get_wait(&self, key: &str, wait: &GetWait) {
        let task = {
            let mut st = self.lock_state();
            match st.get_waits.get(key) {
                Some(entry) if entry.wait.ptr_eq(wait) && wait.is_abandoned() => {
                    st.get_waits.remove(key).map(|e| e.task)
                }
                _ => None,
            }
        };
        if let Some(t) = task {
            t.abort();
        }
    }

    fn stale_cached<T: ResponseBody>(&self, key: &str, stale: f64) -> Option<AgedResponse<T>> {
        match self.inner.cache.lookup::<T>(key, 0.0, stale, None) {
            Lookup::Stale { value, fetched_at } => Some(AgedResponse {
                value,
                fetched_at,
                is_stale: true,
            }),
            _ => None,
        }
    }

    // MARK: - stale 재검증 게이트

    pub(super) fn schedule_stale_revalidation(&self, key: String, run: RevalidationRun) {
        let mut st = self.lock_state();
        // 같은 키를 이미 받는 중이면 그 결과가 캐시를 갱신한다.
        if st.get_waits.contains_key(&key) || st.revalidation_keys.contains(&key) {
            return;
        }
        let job = StaleRevalidation { key, run };
        if st.revalidations_running < STALE_REVALIDATION_CONCURRENCY {
            self.start_stale_revalidation(&mut st, job);
        } else if st.revalidation_queue.len() < STALE_REVALIDATION_QUEUE_LIMIT {
            st.revalidation_keys.insert(job.key.clone());
            st.revalidation_queue.push_back(job);
        } else {
            st.revalidations_dropped += 1;
        }
    }

    fn start_stale_revalidation(&self, st: &mut ClientState, job: StaleRevalidation) {
        st.revalidations_running += 1;
        st.revalidations_started += 1;
        st.revalidation_keys.insert(job.key.clone());
        let client = self.clone();
        tokio::spawn(async move {
            let StaleRevalidation { key, run } = job;
            run(client.clone()).await;
            client.finish_stale_revalidation(&key);
        });
    }

    fn finish_stale_revalidation(&self, key: &str) {
        let mut st = self.lock_state();
        st.revalidations_running = st.revalidations_running.saturating_sub(1);
        st.revalidation_keys.remove(key);
        while st.revalidations_running < STALE_REVALIDATION_CONCURRENCY {
            let Some(next) = st.revalidation_queue.pop_front() else {
                break;
            };
            if st.get_waits.contains_key(&next.key) {
                st.revalidation_keys.remove(&next.key);
                continue;
            }
            self.start_stale_revalidation(&mut st, next);
        }
    }
}
