//! 통합 테스트 공용 보조.
#![allow(dead_code)]

use cview_persistence::time::{adding, from_unix, Date};
use cview_persistence::DataStore;

pub fn store() -> DataStore {
    DataStore::open_in_memory().expect("인메모리 스토어")
}

pub fn ts(secs: f64) -> Date {
    from_unix(secs)
}

pub fn add(date: &Date, secs: f64) -> Date {
    adding(date, secs)
}

pub fn now() -> Date {
    cview_persistence::time::now()
}

pub fn approx(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}
