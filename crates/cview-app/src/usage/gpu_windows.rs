//! Windows GPU 사용률 — 성능 카운터(PDH). 작업 관리자의 GPU 열과 같은 근원이다.
//!
//! - `\GPU Engine(*)\Utilization Percentage`: 인스턴스 이름이 `pid_1234_luid_0x…_0x…_phys_0_eng_3_engtype_3D` 꼴이다.
//!   엔진(luid·phys·eng)마다 프로세스 값을 더하고 가장 바쁜 엔진 값을 쓴다 — 앱은 앱 묶음 PID 만, 전체는 모두.
//! - `\GPU Process Memory(*)\Dedicated Usage`·`Shared Usage`: 앱 묶음 PID 의 전용 + 공유 메모리.
//! - 카운터가 없으면(WDDM 2.x 이전 드라이버·서비스 꺼짐) `GpuProbe::new` 가 `None` — 화면은 '지원 안 됨'.
//! - 비율 카운터라 첫 수집 값은 버린다. 인스턴스가 많은 PC 에서는 수집이 수 ms 걸릴 수 있어 상위(`usage`)가
//!   걸린 시간을 보고 GPU 수집 간격을 늘린다.

use std::collections::{HashMap, HashSet};

/// `pid_1234_luid_…` → (1234, `luid_…` 나머지). 순수 함수.
pub fn split_instance(name: &str) -> Option<(u32, &str)> {
    let rest = name.strip_prefix("pid_")?;
    let (pid, tail) = rest.split_once('_')?;
    Some((pid.parse().ok()?, tail))
}

/// 엔진 인스턴스 값들 → (앱 %, 전체 %). 엔진마다 더해 가장 큰 값. 순수 함수.
pub fn engine_usage<'a>(
    items: impl IntoIterator<Item = (&'a str, f64)>,
    pids: &HashSet<u32>,
) -> (f32, f32) {
    let mut app: HashMap<&str, f64> = HashMap::new();
    let mut all: HashMap<&str, f64> = HashMap::new();
    for (name, v) in items {
        let Some((pid, engine)) = split_instance(name) else {
            continue;
        };
        if !v.is_finite() || v < 0.0 {
            continue;
        }
        *all.entry(engine).or_default() += v;
        if pids.contains(&pid) {
            *app.entry(engine).or_default() += v;
        }
    }
    let max = |m: &HashMap<&str, f64>| m.values().copied().fold(0.0f64, f64::max).min(100.0) as f32;
    (max(&app), max(&all))
}

/// 메모리 인스턴스 값들 → 앱 묶음 합(바이트). 순수 함수.
pub fn process_memory<'a>(
    items: impl IntoIterator<Item = (&'a str, f64)>,
    pids: &HashSet<u32>,
) -> u64 {
    items
        .into_iter()
        .filter_map(|(name, v)| {
            let (pid, _) = split_instance(name)?;
            (pids.contains(&pid) && v.is_finite() && v > 0.0).then_some(v as u64)
        })
        .sum()
}

#[cfg(windows)]
pub use probe::GpuProbe;

#[cfg(windows)]
mod probe {
    use std::collections::HashSet;

    use windows_sys::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W,
        PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
    };

    use super::super::GpuReading;
    use super::{engine_usage, process_memory};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 넓은 문자 C 문자열 → String.
    ///
    /// # Safety
    /// `p` 는 NUL 로 끝나는 UTF-16 문자열이어야 한다.
    unsafe fn from_wide(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        while *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }

    pub struct GpuProbe {
        query: PDH_HQUERY,
        engine: PDH_HCOUNTER,
        dedicated: Option<PDH_HCOUNTER>,
        shared: Option<PDH_HCOUNTER>,
        /// PDH_FMT_COUNTERVALUE_ITEM_W 배열 + 이름 문자열을 받을 버퍼(8바이트 정렬).
        buf: Vec<u64>,
        primed: bool,
        /// 연달아 실패한 수집 횟수 — 넘으면 '지원 안 됨'으로 알린다(카운터 서비스 꺼짐 등).
        failures: u32,
        pub name: Option<String>,
    }

    /// 이만큼 연달아 실패하면 지원 안 됨으로 본다.
    const FAILURES_UNSUPPORTED: u32 = 3;

    impl Drop for GpuProbe {
        fn drop(&mut self) {
            // SAFETY: 연 질의를 한 번 닫는다(카운터도 함께 닫힌다).
            unsafe { PdhCloseQuery(self.query) };
        }
    }

    impl GpuProbe {
        pub fn new() -> Option<Self> {
            let mut query: PDH_HQUERY = std::ptr::null_mut();
            // SAFETY: 실시간 데이터 원본(null)으로 질의를 연다.
            if unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut query) } != 0 {
                return None;
            }
            let add = |path: &str| -> Option<PDH_HCOUNTER> {
                let w = wide(path);
                let mut c: PDH_HCOUNTER = std::ptr::null_mut();
                // SAFETY: 열린 질의에 영어 카운터 경로(지역화와 무관)를 더한다.
                (unsafe { PdhAddEnglishCounterW(query, w.as_ptr(), 0, &mut c) } == 0).then_some(c)
            };
            let Some(engine) = add("\\GPU Engine(*)\\Utilization Percentage") else {
                // SAFETY: 위에서 연 질의.
                unsafe { PdhCloseQuery(query) };
                return None;
            };
            let dedicated = add("\\GPU Process Memory(*)\\Dedicated Usage");
            let shared = add("\\GPU Process Memory(*)\\Shared Usage");
            let mut probe = Self {
                query,
                engine,
                dedicated,
                shared,
                buf: Vec::new(),
                primed: false,
                failures: 0,
                name: None,
            };
            // 비율 카운터의 첫 수집 — 다음 표본부터 값이 나온다.
            // SAFETY: 열린 질의.
            probe.primed = unsafe { PdhCollectQueryData(probe.query) } == 0;
            Some(probe)
        }

        /// 최근 수집이 잘 되고 있나.
        pub fn healthy(&self) -> bool {
            self.failures < FAILURES_UNSUPPORTED
        }

        pub fn memory_supported(&self) -> bool {
            self.dedicated.is_some() || self.shared.is_some()
        }

        /// 카운터 배열(인스턴스 이름, 값). 실패하면 빈 목록.
        fn read(&mut self, counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
            let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            // 첫 호출은 크기만, 사이에 인스턴스가 늘면 PDH_MORE_DATA 가 다시 와서 몇 번 더 묻는다.
            for _ in 0..3 {
                let mut size: u32 = (self.buf.len() * 8) as u32;
                let mut count: u32 = 0;
                let ptr = if self.buf.is_empty() {
                    std::ptr::null_mut()
                } else {
                    self.buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>()
                };
                // SAFETY: size 는 buf 의 바이트 수, ptr 는 그만큼 쓸 수 있는 8바이트 정렬 버퍼(또는 null·0).
                let st = unsafe {
                    PdhGetFormattedCounterArrayW(
                        counter,
                        PDH_FMT_DOUBLE,
                        &mut size,
                        &mut count,
                        ptr,
                    )
                };
                if st == PDH_MORE_DATA {
                    self.buf.resize((size as usize).div_ceil(8) + 64, 0);
                    continue;
                }
                if st != 0 {
                    return Vec::new();
                }
                let n = (count as usize).min(self.buf.len() * 8 / item.max(1));
                // SAFETY: 성공한 호출이 buf 앞에 count 개 원소를 채웠다(이름 포인터는 buf 뒤쪽을 가리킨다).
                let items = unsafe {
                    std::slice::from_raw_parts(
                        self.buf.as_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>(),
                        n,
                    )
                };
                return items
                    .iter()
                    .filter(|it| {
                        matches!(
                            it.FmtValue.CStatus,
                            PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA
                        )
                    })
                    // SAFETY: PDH_FMT_DOUBLE 로 받았으니 doubleValue 가 유효하고, szName 은 NUL 로 끝난다.
                    .map(|it| unsafe { (from_wide(it.szName), it.FmtValue.Anonymous.doubleValue) })
                    .collect();
            }
            Vec::new()
        }

        pub fn sample(&mut self, pids: &HashSet<u32>) -> GpuReading {
            // SAFETY: 열린 질의.
            let ok = unsafe { PdhCollectQueryData(self.query) } == 0;
            if !ok || !self.primed {
                self.primed = ok;
                if !ok {
                    self.failures = self.failures.saturating_add(1);
                }
                return GpuReading::default();
            }
            let engines = self.read(self.engine);
            if engines.is_empty() {
                // 읽지 못했다 — 0% 로 보이지 않게 값 없음으로.
                self.failures = self.failures.saturating_add(1);
                return GpuReading::default();
            }
            self.failures = 0;
            let (app, system) = engine_usage(engines.iter().map(|(n, v)| (n.as_str(), *v)), pids);
            // 메모리 — 읽은 줄이 있는 카운터만 더한다(못 읽으면 0 MB 가 아니라 값 없음).
            let mut memory = None;
            for c in [self.dedicated, self.shared].into_iter().flatten() {
                let rows = self.read(c);
                if rows.is_empty() {
                    continue;
                }
                let sum = process_memory(rows.iter().map(|(n, v)| (n.as_str(), *v)), pids);
                memory = Some(memory.unwrap_or(0) + sum);
            }
            GpuReading {
                app: Some(app),
                system: Some(system),
                memory,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const E3D: &str = "luid_0x00000000_0x0000D1E3_phys_0_eng_0_engtype_3D";
    const EVD: &str = "luid_0x00000000_0x0000D1E3_phys_0_eng_3_engtype_VideoDecode";

    #[test]
    fn splits_instance_names() {
        assert_eq!(
            split_instance(&format!("pid_1234_{E3D}")),
            Some((1234, E3D))
        );
        assert_eq!(split_instance("_Total"), None);
        assert_eq!(split_instance("pid_x_luid"), None);
    }

    #[test]
    fn engine_usage_sums_per_engine_and_takes_busiest() {
        let mine: HashSet<u32> = [10, 11].into_iter().collect();
        let rows = [
            (format!("pid_10_{E3D}"), 12.0),
            (format!("pid_11_{E3D}"), 8.0),
            (format!("pid_10_{EVD}"), 30.0),
            (format!("pid_99_{E3D}"), 50.0),
            ("garbage".to_string(), 99.0),
            (format!("pid_11_{EVD}"), f64::NAN),
        ];
        let (app, all) = engine_usage(rows.iter().map(|(n, v)| (n.as_str(), *v)), &mine);
        // 앱: 3D 20 · 디코드 30 → 30. 전체: 3D 70 · 디코드 30 → 70.
        assert_eq!(app, 30.0);
        assert_eq!(all, 70.0);
        // 합이 100 을 넘으면 100.
        let busy = [
            (format!("pid_10_{E3D}"), 80.0),
            (format!("pid_11_{E3D}"), 60.0),
        ];
        let (app, _) = engine_usage(busy.iter().map(|(n, v)| (n.as_str(), *v)), &mine);
        assert_eq!(app, 100.0);
    }

    #[test]
    fn process_memory_sums_only_app() {
        let mine: HashSet<u32> = [10].into_iter().collect();
        let rows = [
            ("pid_10_luid_0x0_0x1_phys_0", 1_000.0),
            ("pid_10_luid_0x0_0x2_phys_0", 500.0),
            ("pid_20_luid_0x0_0x1_phys_0", 9_000.0),
        ];
        assert_eq!(
            process_memory(rows.iter().map(|(n, v)| (*n, *v)), &mine),
            1_500
        );
    }
}
