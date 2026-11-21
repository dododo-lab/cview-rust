//! macOS GPU 사용률 — IOKit 레지스트리에서 읽는다(Apple 실리콘).
//!
//! - 전체: `IOAccelerator` 서비스의 `PerformanceStatistics` › `Device Utilization %`·`In use system memory`.
//! - 앱: 가속기의 자식 사용자 클라이언트(`AGXDeviceUserClient`) 중 `IOUserClientCreator`("pid N, 이름")가 앱 묶음인 것의
//!   `AppUsage[].accumulatedGPUTime`(ns 누계) 차이 ÷ 지난 시간. 활성 상태 보기의 'GPU %'와 같은 근원이다.
//! - 클라이언트 목록(자식 160개 남짓)은 프로세스 묶음을 다시 찾을 때만 훑고, 표본마다 찾아 둔 몇 개만 읽는다.
//! - 값이 없으면(Intel GPU·다른 드라이버·속성 이름 변경) 해당 항목만 `None` — 화면이 '지원 안 됨'으로 보인다.

use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_void, CStr};
use std::time::Instant;

use super::GpuReading;

type KernReturn = i32;
type MachPort = u32;
type IoObject = MachPort;
type CFTypeRef = *const c_void;
type CFIndex = isize;
type CFTypeID = usize;

const KERN_SUCCESS: KernReturn = 0;
const UTF8: u32 = 0x0800_0100;
const CF_NUMBER_SINT64: CFIndex = 4;
const CF_NUMBER_FLOAT64: CFIndex = 6;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingServices(
        main: MachPort,
        matching: *const c_void,
        it: *mut IoObject,
    ) -> KernReturn;
    fn IOIteratorNext(it: IoObject) -> IoObject;
    fn IOObjectRelease(o: IoObject) -> KernReturn;
    fn IORegistryEntryGetChildIterator(
        e: IoObject,
        plane: *const c_char,
        it: *mut IoObject,
    ) -> KernReturn;
    fn IORegistryEntryCreateCFProperty(
        e: IoObject,
        key: CFTypeRef,
        alloc: CFTypeRef,
        options: u32,
    ) -> CFTypeRef;
    fn IORegistryEntryGetRegistryEntryID(e: IoObject, id: *mut u64) -> KernReturn;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(alloc: CFTypeRef, s: *const c_char, enc: u32) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
    fn CFDictionaryGetTypeID() -> CFTypeID;
    fn CFArrayGetTypeID() -> CFTypeID;
    fn CFNumberGetTypeID() -> CFTypeID;
    fn CFStringGetTypeID() -> CFTypeID;
    fn CFDictionaryGetValue(d: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFArrayGetCount(a: CFTypeRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(a: CFTypeRef, i: CFIndex) -> CFTypeRef;
    fn CFNumberGetValue(n: CFTypeRef, ty: CFIndex, out: *mut c_void) -> u8;
    fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: CFIndex, enc: u32) -> u8;
}

/// 만든(+1) CF 객체 — 놓을 때 CFRelease.
struct Owned(CFTypeRef);

impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: Create 계열이 돌려준 +1 참조를 한 번만 놓는다.
            unsafe { CFRelease(self.0) };
        }
    }
}

fn cf_str(s: &CStr) -> Owned {
    // SAFETY: NUL 로 끝나는 UTF-8 문자열에서 CFString 을 만든다.
    Owned(unsafe { CFStringCreateWithCString(std::ptr::null(), s.as_ptr(), UTF8) })
}

/// 레지스트리 항목의 속성(+1). 없으면 `None`.
fn property(entry: IoObject, key: &Owned) -> Option<Owned> {
    if key.0.is_null() {
        return None;
    }
    // SAFETY: 유효한 항목·키, 기본 할당자, 옵션 0.
    let v = unsafe { IORegistryEntryCreateCFProperty(entry, key.0, std::ptr::null(), 0) };
    (!v.is_null()).then_some(Owned(v))
}

fn is_type(v: CFTypeRef, id: CFTypeID) -> bool {
    // SAFETY: v 는 살아 있는 CF 객체다.
    !v.is_null() && unsafe { CFGetTypeID(v) } == id
}

fn number(v: CFTypeRef) -> Option<f64> {
    // SAFETY: CFNumber 인지 확인한 뒤 같은 크기의 버퍼로 꺼낸다.
    unsafe {
        if !is_type(v, CFNumberGetTypeID()) {
            return None;
        }
        let mut i: i64 = 0;
        if CFNumberGetValue(v, CF_NUMBER_SINT64, (&mut i as *mut i64).cast()) != 0 {
            return Some(i as f64);
        }
        let mut f: f64 = 0.0;
        (CFNumberGetValue(v, CF_NUMBER_FLOAT64, (&mut f as *mut f64).cast()) != 0).then_some(f)
    }
}

fn dict_number(dict: CFTypeRef, key: &Owned) -> Option<f64> {
    // SAFETY: 사전인지 확인하고 빌린 값을 읽는다.
    unsafe {
        if !is_type(dict, CFDictionaryGetTypeID()) || key.0.is_null() {
            return None;
        }
        number(CFDictionaryGetValue(dict, key.0))
    }
}

fn string(v: CFTypeRef) -> Option<String> {
    // SAFETY: CFString 인지 확인하고 넉넉한 버퍼로 UTF-8 을 꺼낸다.
    unsafe {
        if !is_type(v, CFStringGetTypeID()) {
            return None;
        }
        let mut buf = [0 as c_char; 256];
        if CFStringGetCString(v, buf.as_mut_ptr(), buf.len() as CFIndex, UTF8) == 0 {
            return None;
        }
        Some(CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }
}

/// "pid 408, WindowServer" → 408. 순수 함수.
pub fn creator_pid(s: &str) -> Option<u32> {
    s.trim()
        .strip_prefix("pid ")?
        .split(',')
        .next()?
        .trim()
        .parse()
        .ok()
}

/// 레지스트리 항목 손잡이 — 놓을 때 IOObjectRelease.
struct Entry(IoObject);

impl Drop for Entry {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: 반복자·조회가 넘긴 참조를 한 번만 놓는다.
            unsafe { IOObjectRelease(self.0) };
        }
    }
}

struct Keys {
    perf: Owned,
    util: Owned,
    in_use: Owned,
    creator: Owned,
    app_usage: Owned,
    gpu_time: Owned,
    model: Owned,
}

pub struct GpuProbe {
    accel: Entry,
    keys: Keys,
    /// 앱 묶음의 사용자 클라이언트(레지스트리 ID, 손잡이).
    clients: Vec<(u64, Entry)>,
    /// 클라이언트마다 지난 GPU 시간 누계(ns).
    last_ns: HashMap<u64, u64>,
    last_at: Option<Instant>,
    /// 앱 GPU 시간을 읽을 수 있는가(클라이언트에 AppUsage 가 있었나). 처음 훑기 전엔 `None`.
    app_supported: Option<bool>,
    /// 마지막 표본에서 전체 GPU 사용률을 읽었나 — 지원 여부를 묻느라 IOKit 을 다시 부르지 않게.
    system_ok: bool,
    pub name: Option<String>,
}

impl GpuProbe {
    /// 가속기를 찾는다. 없으면 `None`(GPU 값은 모두 '지원 안 됨').
    pub fn new() -> Option<Self> {
        // SAFETY: 클래스 이름으로 일치 사전을 만들고(GetMatchingServices 가 소비) 반복자에서 첫 항목을 가진다.
        let accel = unsafe {
            let matching = IOServiceMatching(c"IOAccelerator".as_ptr());
            if matching.is_null() {
                return None;
            }
            let mut it: IoObject = 0;
            if IOServiceGetMatchingServices(0, matching, &mut it) != KERN_SUCCESS || it == 0 {
                return None;
            }
            let it = Entry(it);
            let first = IOIteratorNext(it.0);
            if first == 0 {
                return None;
            }
            Entry(first)
        };
        let keys = Keys {
            perf: cf_str(c"PerformanceStatistics"),
            util: cf_str(c"Device Utilization %"),
            in_use: cf_str(c"In use system memory"),
            creator: cf_str(c"IOUserClientCreator"),
            app_usage: cf_str(c"AppUsage"),
            gpu_time: cf_str(c"accumulatedGPUTime"),
            model: cf_str(c"model"),
        };
        let name = property(accel.0, &keys.model).and_then(|v| string(v.0));
        Some(Self {
            accel,
            keys,
            clients: Vec::new(),
            last_ns: HashMap::new(),
            last_at: None,
            app_supported: None,
            system_ok: false,
            name,
        })
    }

    /// 전체 GPU 사용률·사용 메모리를 읽을 수 있는가.
    pub fn system_supported(&self) -> bool {
        self.system_ok
    }

    /// 앱 GPU 를 읽을 수 있는가(처음 훑기 전엔 `None`).
    pub fn app_supported(&self) -> Option<bool> {
        self.app_supported
    }

    fn system(&self) -> (Option<f32>, Option<u64>) {
        let Some(stats) = property(self.accel.0, &self.keys.perf) else {
            return (None, None);
        };
        let util = dict_number(stats.0, &self.keys.util).map(|v| v.clamp(0.0, 100.0) as f32);
        let mem = dict_number(stats.0, &self.keys.in_use)
            .filter(|v| *v >= 0.0)
            .map(|v| v as u64);
        (util, mem)
    }

    /// 앱 묶음(`pids`)의 클라이언트를 다시 찾는다 — 프로세스 묶음을 다시 찾을 때.
    pub fn rescan(&mut self, pids: &HashSet<u32>) {
        let mut found: Vec<(u64, Entry)> = Vec::new();
        let mut any_usage = false;
        // SAFETY: 가속기의 IOService 자식을 차례로 돌며 손잡이는 Entry 가 놓는다.
        unsafe {
            let mut it: IoObject = 0;
            if IORegistryEntryGetChildIterator(self.accel.0, c"IOService".as_ptr(), &mut it)
                != KERN_SUCCESS
            {
                return;
            }
            let it = Entry(it);
            loop {
                let child = IOIteratorNext(it.0);
                if child == 0 {
                    break;
                }
                let child = Entry(child);
                let Some(creator) = property(child.0, &self.keys.creator) else {
                    continue;
                };
                if !any_usage {
                    any_usage = property(child.0, &self.keys.app_usage).is_some();
                }
                let Some(pid) = string(creator.0).as_deref().and_then(creator_pid) else {
                    continue;
                };
                if !pids.contains(&pid) {
                    continue;
                }
                let mut id = 0u64;
                if IORegistryEntryGetRegistryEntryID(child.0, &mut id) != KERN_SUCCESS {
                    continue;
                }
                found.push((id, child));
            }
        }
        // 한 번이라도 AppUsage 를 봤으면 지원으로 본다(앱 클라이언트가 아직 없을 수 있다).
        self.app_supported = Some(self.app_supported == Some(true) || any_usage);
        self.last_ns
            .retain(|id, _| found.iter().any(|(f, _)| f == id));
        self.clients = found;
    }

    fn client_ns(&self, entry: IoObject) -> Option<u64> {
        let usage = property(entry, &self.keys.app_usage)?;
        // SAFETY: 배열인지 확인하고 빌린 원소(사전)를 읽는다.
        unsafe {
            if !is_type(usage.0, CFArrayGetTypeID()) {
                return None;
            }
            let n = CFArrayGetCount(usage.0);
            let mut total = 0u64;
            for i in 0..n {
                let item = CFArrayGetValueAtIndex(usage.0, i);
                if let Some(ns) = dict_number(item, &self.keys.gpu_time) {
                    total = total.saturating_add(ns.max(0.0) as u64);
                }
            }
            Some(total)
        }
    }

    pub fn sample(&mut self) -> GpuReading {
        let (sys, mem) = self.system();
        self.system_ok = sys.is_some();
        let now = Instant::now();
        let mut delta = 0u64;
        let mut seen = 0usize;
        for (id, entry) in &self.clients {
            let Some(ns) = self.client_ns(entry.0) else {
                continue;
            };
            seen += 1;
            // 처음 보는 클라이언트는 기준만 잡는다 — 만든 지 오래된 누계가 한 번에 튀지 않게.
            if let Some(prev) = self.last_ns.insert(*id, ns) {
                delta = delta.saturating_add(ns.saturating_sub(prev));
            }
        }
        let app = match (self.app_supported, self.last_at) {
            (Some(true), Some(prev)) => {
                let wall = now.duration_since(prev).as_nanos().max(1) as f64;
                Some(((delta as f64 / wall) * 100.0).clamp(0.0, 100.0) as f32)
            }
            // 클라이언트가 없으면(아직 그리기 전) 0 — 지원은 되므로 '지원 안 됨'이 아니다.
            (Some(true), None) if seen == 0 => Some(0.0),
            _ => None,
        };
        self.last_at = Some(now);
        GpuReading {
            app,
            system: sys,
            memory: mem,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_creator() {
        assert_eq!(creator_pid("pid 408, WindowServer"), Some(408));
        assert_eq!(creator_pid(" pid 12,"), Some(12));
        assert_eq!(creator_pid("pid x, y"), None);
        assert_eq!(creator_pid("WindowServer"), None);
    }

    #[test]
    fn reads_real_accelerator_when_present() {
        // CI 가상 머신 등 가속기가 없으면 None — 그 경우도 실패가 아니다.
        let Some(mut probe) = GpuProbe::new() else {
            return;
        };
        let me: HashSet<u32> = [std::process::id()].into_iter().collect();
        probe.rescan(&me);
        let first = probe.sample();
        let second = probe.sample();
        if let Some(u) = second.system {
            assert!((0.0..=100.0).contains(&u));
        }
        if let (Some(a), Some(b)) = (first.app, second.app) {
            assert!((0.0..=100.0).contains(&a) && (0.0..=100.0).contains(&b));
        }
    }
}
