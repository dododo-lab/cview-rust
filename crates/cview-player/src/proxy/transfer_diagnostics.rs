//! `Sources/CViewPlayer/ProxyTransferDiagnostics.swift` 포팅 — 전송 하나의 네이티브 타이밍.
//!
//! URL·쿠키·키·채널명은 남기지 않는다. "processed" 는 로컬 처리(hyper 에 넘김)를 뜻하며
//! 플레이어 전달을 뜻하지 않는다. URLSession 트랜잭션 메트릭(프로토콜·재사용 연결)은 reqwest 에
//! 해당 API 가 없어 비워 둔다.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 전송 진단 레코드.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTransferDiagnostic {
    pub id: Uuid,
    pub generation: u64,
    pub kind: String,
    pub status: u16,
    pub mime_corrected: bool,
    pub header_wait: f64,
    pub body_time: f64,
    pub first_forward_delay: f64,
    pub send_drain_time: f64,
    pub upstream_bytes: i64,
    pub enqueued_bytes: i64,
    pub processed_bytes: i64,
    pub peak_pending_bytes: i64,
    pub outcome: String,
    pub protocols: Vec<String>,
    pub reused_connections: i64,
    pub transactions: i64,
}

impl ProxyTransferDiagnostic {
    pub fn new(generation: u64, kind: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            generation,
            kind: kind.to_string(),
            status: 0,
            mime_corrected: false,
            header_wait: 0.0,
            body_time: 0.0,
            first_forward_delay: 0.0,
            send_drain_time: 0.0,
            upstream_bytes: 0,
            enqueued_bytes: 0,
            processed_bytes: 0,
            peak_pending_bytes: 0,
            outcome: "pending".into(),
            protocols: Vec::new(),
            reused_connections: 0,
            transactions: 0,
        }
    }
}

/// 완료 레코드를 받는 쪽.
pub type PublishFn = Arc<dyn Fn(ProxyTransferDiagnostic) + Send + Sync>;

struct TraceState {
    value: ProxyTransferDiagnostic,
    first_body: Option<f64>,
    last_body: Option<f64>,
    first_forward: Option<f64>,
    last_processed: Option<f64>,
    pending: i64,
    finished: bool,
}

/// 전송 하나의 추적기. 끝났고(`finish`) 대기 바이트가 0 일 때 레코드를 발행한다.
pub struct ProxyTransferTrace {
    state: Mutex<TraceState>,
    publish: PublishFn,
    start: Instant,
}

impl ProxyTransferTrace {
    pub fn new(generation: u64, kind: &str, publish: PublishFn) -> Self {
        Self {
            state: Mutex::new(TraceState {
                value: ProxyTransferDiagnostic::new(generation, kind),
                first_body: None,
                last_body: None,
                first_forward: None,
                last_processed: None,
                pending: 0,
                finished: false,
            }),
            publish,
            start: Instant::now(),
        }
    }

    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn with<R>(&self, f: impl FnOnce(&mut TraceState) -> R) -> R {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut s)
    }

    pub fn response(&self, status: u16, kind: &str, mime_corrected: bool) {
        let now = self.now();
        self.with(|s| {
            s.value.status = status;
            s.value.kind = kind.to_string();
            s.value.mime_corrected = mime_corrected;
            s.value.header_wait = now.max(0.0);
        });
    }

    pub fn received(&self, count: usize) {
        let now = self.now();
        self.with(|s| {
            let first = *s.first_body.get_or_insert(now);
            s.last_body = Some(now);
            s.value.upstream_bytes += count as i64;
            s.value.body_time = (now - first).max(0.0);
        });
    }

    pub fn enqueued(&self, count: usize) {
        let now = self.now();
        self.with(|s| {
            if s.first_forward.is_none() {
                s.first_forward = Some(now);
                s.value.first_forward_delay = (now - s.first_body.unwrap_or(now)).max(0.0);
            }
            s.value.enqueued_bytes += count as i64;
            s.pending += count as i64;
            s.value.peak_pending_bytes = s.value.peak_pending_bytes.max(s.pending);
        });
    }

    pub fn processed(&self, count: usize, succeeded: bool) {
        let now = self.now();
        let result = self.with(|s| {
            s.pending = (s.pending - count as i64).max(0);
            s.last_processed = Some(now);
            if succeeded {
                s.value.processed_bytes += count as i64;
            } else {
                s.value.outcome = "downstreamError".into();
            }
            Self::completed(s)
        });
        if let Some(r) = result {
            (self.publish)(r);
        }
    }

    pub fn finish(&self, outcome: &str) {
        let result = self.with(|s| {
            s.finished = true;
            if s.value.outcome != "downstreamError" {
                s.value.outcome = outcome.to_string();
            }
            Self::completed(s)
        });
        if let Some(r) = result {
            (self.publish)(r);
        }
    }

    /// 아직 넘기지 못한 대기 바이트.
    pub fn pending(&self) -> i64 {
        self.with(|s| s.pending)
    }

    fn completed(s: &mut TraceState) -> Option<ProxyTransferDiagnostic> {
        if !s.finished || s.pending != 0 {
            return None;
        }
        if let (Some(received), Some(processed)) = (s.last_body, s.last_processed) {
            s.value.send_drain_time = (processed - received).max(0.0);
        }
        Some(s.value.clone())
    }
}

/// 최근 256 건 보관소 — 같은 id 는 갱신한다.
#[derive(Default)]
pub struct TransferRecords {
    records: Mutex<Vec<ProxyTransferDiagnostic>>,
}

impl TransferRecords {
    /// 레코드를 넣는다. 로그로 남길 만하면 `true`.
    pub fn insert(&self, record: ProxyTransferDiagnostic) -> bool {
        let mut r = self.records.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = r.iter_mut().find(|x| x.id == record.id) {
            *existing = record;
            return false;
        }
        if r.len() >= 256 {
            r.remove(0);
        }
        let log = record.outcome != "success"
            || r.len() < 3
            || record.id.simple().to_string().ends_with('0');
        r.push(record);
        log
    }

    pub fn snapshot(&self) -> Vec<ProxyTransferDiagnostic> {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn clear(&self) {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}

#[cfg(test)]
mod tests {
    //! `StreamCorrectionProxyTests.terminalMetricsWaitForPendingSendsAndDoNotCountFailuresAsProcessed` 포팅.
    use super::*;

    #[test]
    fn terminal_metrics_wait_for_pending_sends() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let r = records.clone();
        let trace =
            ProxyTransferTrace::new(5, "media", Arc::new(move |rec| r.lock().unwrap().push(rec)));
        trace.received(1_000);
        trace.enqueued(600);
        trace.enqueued(400);
        trace.finish("success");
        assert!(records.lock().unwrap().is_empty());
        trace.processed(600, true);
        assert!(records.lock().unwrap().is_empty());
        trace.processed(400, false);
        let last = records.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.peak_pending_bytes, 1_000);
        assert_eq!(last.processed_bytes, 600);
        assert_eq!(last.outcome, "downstreamError");
        assert_eq!(last.generation, 5);
    }

    #[test]
    fn records_update_in_place() {
        let store = TransferRecords::default();
        let mut rec = ProxyTransferDiagnostic::new(1, "media");
        rec.outcome = "success".into();
        assert!(store.insert(rec.clone()));
        rec.transactions = 2;
        assert!(!store.insert(rec.clone()));
        assert_eq!(store.snapshot().len(), 1);
        assert_eq!(store.snapshot()[0].transactions, 2);
    }
}
