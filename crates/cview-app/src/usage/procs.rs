//! 앱 프로세스 묶음 — 앱 본체와 그 도우미(macOS WebKit WebContent·GPU·Networking, Windows WebView2 렌더러·GPU 등)를
//! 찾고 CPU·메모리를 잰다.
//!
//! - macOS: WebKit 도우미는 launchd 의 자식이라 부모로 찾을 수 없다. 활성 상태 보기처럼 '책임 프로세스'
//!   (`responsibility_get_pid_responsible_for_pid`)가 이 앱인 프로세스를 모은다. 함수가 없으면(비공개 API) 예전
//!   디버그 로그 방식(이름에 WebKit · 앱보다 늦게 시작)으로 물러난다. 메모리는 물리 사용량(`ri_phys_footprint`).
//! - Windows: WebView2 는 앱의 자식(브라우저) → 그 자식(렌더러·GPU…)이라 부모 사슬로 찾는다(PID 재사용을 걸러 고리마다 시작 시각을 본다). 메모리는 전용 바이트.
//! - 표본마다 찾아 둔 몇 개만 다시 읽고, 전체 목록은 [`REDISCOVER`] 마다 한 번 훑는다
//!   (전체 목록을 매번 읽으면 그것만으로 CPU 를 눈에 띄게 썼다 — perf_log 표본과 같은 이유).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// 도우미를 다시 찾는 주기 — 웹뷰가 프로세스를 새로 띄울 수 있다.
pub const REDISCOVER: Duration = Duration::from_secs(5);

/// 프로세스 종류(화면의 '어디에 쓰나' 묶음).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcKind {
    /// 앱 본체(Rust·Tauri).
    App,
    /// 웹 화면(WebKit WebContent · WebView2 렌더러).
    Web,
    /// GPU 프로세스(그리기·영상 디코딩).
    Gpu,
    /// 네트워크 프로세스.
    Network,
    /// 그 밖의 도우미(WebView2 브라우저·유틸리티 등).
    Other,
}

/// 이름(과 명령줄)으로 종류를 가른다. 순수 함수.
pub fn classify(name: &str, cmd: &str, is_app: bool) -> ProcKind {
    if is_app {
        return ProcKind::App;
    }
    let n = name.to_ascii_lowercase();
    if n.contains("webkit.webcontent") {
        return ProcKind::Web;
    }
    if n.contains("webkit.gpu") {
        return ProcKind::Gpu;
    }
    // VideoToolbox 디코더 서비스 — 하드웨어 디코드 프레임 메모리가 여기 잡힌다(예전엔 '그 밖'에 섞였다).
    if n.contains("vtdecoderxpcservice") {
        return ProcKind::Gpu;
    }
    if n.contains("webkit.networking") {
        return ProcKind::Network;
    }
    if n.starts_with("msedgewebview2") {
        let c = cmd.to_ascii_lowercase();
        if c.contains("--type=renderer") {
            return ProcKind::Web;
        }
        if c.contains("--type=gpu-process") {
            return ProcKind::Gpu;
        }
        if c.contains("network.mojom.networkservice") {
            return ProcKind::Network;
        }
    }
    ProcKind::Other
}

/// 종류별 합계(화면의 내역).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupUsage {
    pub kind: ProcKind,
    /// 코어 하나 = 100.
    pub cpu: f32,
    pub mem: u64,
    pub count: u16,
}

/// 한 번 잰 결과.
#[derive(Debug, Clone, Default)]
pub struct ProcReading {
    pub cpu: f32,
    pub mem: u64,
    pub groups: Vec<GroupUsage>,
    pub pids: Vec<u32>,
}

/// 종류별로 묶는다 — 앱·웹·GPU·네트워크·기타 순, 빈 종류는 뺀다. 순수 함수.
pub fn group(rows: &[(ProcKind, f32, u64)]) -> Vec<GroupUsage> {
    let mut map: HashMap<ProcKind, GroupUsage> = HashMap::new();
    for &(kind, cpu, mem) in rows {
        let g = map.entry(kind).or_insert(GroupUsage {
            kind,
            cpu: 0.0,
            mem: 0,
            count: 0,
        });
        g.cpu += cpu;
        g.mem += mem;
        g.count = g.count.saturating_add(1);
    }
    let mut out: Vec<GroupUsage> = map.into_values().collect();
    out.sort_by_key(|g| g.kind);
    out
}

pub struct ProcessSet {
    sys: System,
    me: Pid,
    my_start: u64,
    /// 찾아 둔 프로세스와 종류.
    watched: Vec<(Pid, ProcKind)>,
    discovered_at: Option<Instant>,
    /// 프로세스마다 직전 표본의 누적 CPU 시간(ms).
    last_cpu_ms: std::collections::HashMap<u32, u64>,
    #[cfg(target_os = "macos")]
    mac: mac::Discovery,
}

/// 이번 표본 CPU% — 그 사이 누적 CPU 시간이 그대로면 0. macOS sysinfo 는 CPU 시간이 늘지 않은 표본에서 값을 갱신하지
/// 않고 앞 값을 남겨, 재생을 멈춘 뒤 쉬는 디코더·GPU 도우미가 마지막으로 바쁘던 값(예: 48.9%)에 굳어 보였다. 순수 함수.
fn sampled_cpu(previous_ms: Option<u64>, now_ms: u64, reported: f32) -> f32 {
    if previous_ms == Some(now_ms) {
        0.0
    } else {
        reported.max(0.0)
    }
}

impl Default for ProcessSet {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessSet {
    pub fn new() -> Self {
        Self::for_pid(std::process::id())
    }

    /// 다른 CView 프로세스(`pid`)와 그 도우미 묶음 — 측정 도구(examples/usage_probe.rs)가 실행 중인 앱을 밖에서 잰다.
    pub fn for_pid(pid: u32) -> Self {
        let me = Pid::from_u32(pid);
        Self {
            sys: System::new(),
            me,
            my_start: 0,
            watched: vec![(me, ProcKind::App)],
            discovered_at: None,
            last_cpu_ms: Default::default(),
            #[cfg(target_os = "macos")]
            mac: mac::Discovery::default(),
        }
    }

    /// 다음 표본에서 도우미를 다시 찾게 한다(측정을 다시 켰을 때).
    pub fn invalidate(&mut self) {
        self.discovered_at = None;
    }

    /// 찾아 둔 프로세스 ID(앱 포함).
    pub fn pids(&self) -> impl Iterator<Item = u32> + '_ {
        self.watched.iter().map(|(p, _)| p.as_u32())
    }

    /// 방금 도우미를 다시 찾았나(GPU 클라이언트도 이때 다시 찾는다). 아니면 `false`.
    pub fn refresh(&mut self) -> (ProcReading, bool) {
        let rediscovered = self.discovered_at.is_none_or(|t| t.elapsed() >= REDISCOVER);
        if rediscovered {
            self.discover();
            self.discovered_at = Some(Instant::now());
        }
        let pids: Vec<Pid> = self.watched.iter().map(|(p, _)| *p).collect();
        let kind = ProcessRefreshKind::nothing().with_cpu().with_memory();
        self.sys
            .refresh_processes_specifics(ProcessesToUpdate::Some(&pids), true, kind);
        let mut rows = Vec::with_capacity(pids.len());
        let mut alive = Vec::with_capacity(pids.len());
        for (pid, kind) in &self.watched {
            let Some(p) = self.sys.process(*pid) else {
                continue;
            };
            let total_ms = p.accumulated_cpu_time();
            let cpu = sampled_cpu(
                self.last_cpu_ms.insert(pid.as_u32(), total_ms),
                total_ms,
                p.cpu_usage(),
            );
            let mem = footprint(pid.as_u32()).unwrap_or_else(|| private_bytes(p));
            rows.push((*kind, cpu, mem));
            alive.push(pid.as_u32());
        }
        let reading = ProcReading {
            cpu: rows.iter().map(|r| r.1).sum(),
            mem: rows.iter().map(|r| r.2).sum(),
            groups: group(&rows),
            pids: alive,
        };
        (reading, rediscovered)
    }

    fn discover(&mut self) {
        if self.my_start == 0 {
            self.sys.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[self.me]),
                false,
                ProcessRefreshKind::nothing(),
            );
            self.my_start = self
                .sys
                .process(self.me)
                .map(|p| p.start_time())
                .unwrap_or(0);
        }
        #[cfg(target_os = "macos")]
        let found = self.mac.discover(&mut self.sys, self.me, self.my_start);
        #[cfg(not(target_os = "macos"))]
        let found = descendants(&mut self.sys, self.me);
        // 새로 찾은 것만 이름(Windows 는 명령줄도)을 읽어 종류를 정한다.
        let new: Vec<Pid> = found
            .iter()
            .copied()
            .filter(|p| !self.watched.iter().any(|(w, _)| w == p))
            .collect();
        if !new.is_empty() {
            // 명령줄은 WebView2 도우미 종류(--type=…)를 가를 때만 — macOS 는 이름으로 충분하다.
            #[cfg(windows)]
            let kind = ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::OnlyIfNotSet);
            #[cfg(not(windows))]
            let kind = ProcessRefreshKind::nothing();
            self.sys
                .refresh_processes_specifics(ProcessesToUpdate::Some(&new), false, kind);
        }
        let mut watched = Vec::with_capacity(found.len() + 1);
        watched.push((self.me, ProcKind::App));
        for pid in found {
            if pid == self.me {
                continue;
            }
            if let Some((_, k)) = self.watched.iter().find(|(w, _)| *w == pid) {
                watched.push((pid, *k));
                continue;
            }
            let Some(p) = self.sys.process(pid) else {
                continue;
            };
            let cmd = p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            watched.push((pid, classify(&p.name().to_string_lossy(), &cmd, false)));
        }
        // 사라진 프로세스는 sysinfo 목록에서도 지운다(표본 refresh 가 remove_dead 로 지운다).
        self.watched = watched;
    }
}

/// Windows 등 — 부모 사슬이 앱에 닿는 프로세스(손자 포함).
#[cfg(not(target_os = "macos"))]
fn descendants(sys: &mut System, me: Pid) -> Vec<Pid> {
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let info: HashMap<u32, (Option<u32>, u64)> = sys
        .processes()
        .iter()
        .map(|(pid, p)| {
            (
                pid.as_u32(),
                (p.parent().map(|pp| pp.as_u32()), p.start_time()),
            )
        })
        .collect();
    descendants_of(&info, me.as_u32())
        .into_iter()
        .map(Pid::from_u32)
        .collect()
}

/// PID → (부모 PID, 시작 시각 초) 표에서 `me` 의 자손(자기 포함). 순수 함수.
///
/// 부모 PID 만 따라가면 Windows 의 빠른 PID 재사용에 속는다 — 이미 끝난 부모(예: explorer 의 부모 userinit)의 PID 를
/// 앱이 받으면 explorer 아래 프로세스가 모두 앱 묶음이 됐다. 그래서 고리마다 자식이 부모보다 늦게(같은 초 포함) 시작했을
/// 때만 잇고, 시작 시각을 모르는(0) 프로세스는 넣지 않는다.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn descendants_of(info: &HashMap<u32, (Option<u32>, u64)>, me: u32) -> Vec<u32> {
    let linked = |child: u32| -> Option<u32> {
        let &(parent, start) = info.get(&child)?;
        let parent = parent?;
        let &(_, parent_start) = info.get(&parent)?;
        (start != 0 && parent_start != 0 && start >= parent_start).then_some(parent)
    };
    let mut out = vec![me];
    for &pid in info.keys() {
        if pid == me {
            continue;
        }
        let mut cur = pid;
        // 고리가 생겨도 끝나게 깊이를 묶는다.
        for _ in 0..16 {
            match linked(cur) {
                Some(pp) if pp == me => {
                    out.push(pid);
                    break;
                }
                Some(pp) if pp != cur => cur = pp,
                _ => break,
            }
        }
    }
    out
}

/// macOS 밖 메모리 — Windows 는 전용 바이트(PrivateUsage, 작업 관리자 '커밋 크기'). 작업 집합을 더하면 WebView2
/// 프로세스들이 함께 매핑한 공유 페이지(DLL 등)가 프로세스 수만큼 겹쳐 들어갔다.
fn private_bytes(p: &sysinfo::Process) -> u64 {
    if cfg!(windows) {
        p.virtual_memory()
    } else {
        p.memory()
    }
}

/// macOS 물리 사용량(활성 상태 보기 '메모리'). 실패하면 `None`(호출한 쪽이 RSS 로).
#[cfg(target_os = "macos")]
fn footprint(pid: u32) -> Option<u64> {
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    // SAFETY: info 는 rusage_info_v2 크기의 쓰기 가능한 버퍼이고 flavor 가 V2 다.
    let r = unsafe {
        libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast::<libc::rusage_info_t>(),
        )
    };
    (r == 0 && info.ri_phys_footprint > 0).then_some(info.ri_phys_footprint)
}

#[cfg(not(target_os = "macos"))]
fn footprint(_pid: u32) -> Option<u64> {
    None
}

#[cfg(target_os = "macos")]
mod mac {
    use std::collections::{HashMap, HashSet};
    use std::sync::OnceLock;

    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    type ResponsibleFn = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;

    /// 비공개 함수라 링크하지 않고 실행 중에 찾는다 — 없으면 시작이 막히지 않고 예전 방식으로.
    fn responsible_fn() -> Option<ResponsibleFn> {
        static F: OnceLock<Option<ResponsibleFn>> = OnceLock::new();
        *F.get_or_init(|| {
            // SAFETY: RTLD_DEFAULT 에서 C 이름으로 찾는다. 찾으면 pid_t → pid_t 함수다.
            let p = unsafe {
                libc::dlsym(
                    libc::RTLD_DEFAULT,
                    c"responsibility_get_pid_responsible_for_pid".as_ptr(),
                )
            };
            (!p.is_null())
                .then(|| unsafe { std::mem::transmute::<*mut libc::c_void, ResponsibleFn>(p) })
        })
    }

    /// 지금 있는 모든 PID.
    fn all_pids() -> Vec<libc::pid_t> {
        // SAFETY: 빈 버퍼로 개수를 묻고, 넉넉히 잡은 버퍼에 채운다(돌려준 개수만 쓴다).
        unsafe {
            let n = libc::proc_listallpids(std::ptr::null_mut(), 0);
            if n <= 0 {
                return Vec::new();
            }
            let mut buf: Vec<libc::pid_t> = vec![0; n as usize + 64];
            let bytes = (buf.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
            let got = libc::proc_listallpids(buf.as_mut_ptr().cast(), bytes);
            if got <= 0 {
                return Vec::new();
            }
            buf.truncate((got as usize).min(buf.len()));
            buf
        }
    }

    /// 책임 프로세스를 물은 결과를 PID 별로 기억한다 — 이미 본 PID 는 다시 묻지 않는다(살아 있는 동안 바뀌지 않는다).
    #[derive(Default)]
    pub struct Discovery {
        owner: HashMap<libc::pid_t, bool>,
    }

    impl Discovery {
        pub fn discover(&mut self, sys: &mut System, me: Pid, my_start: u64) -> Vec<Pid> {
            let me_raw = me.as_u32() as libc::pid_t;
            let Some(f) = responsible_fn() else {
                return fallback(sys, me, my_start);
            };
            let pids = all_pids();
            let live: HashSet<libc::pid_t> = pids.iter().copied().collect();
            self.owner.retain(|p, _| live.contains(p));
            let mut out = vec![me];
            for pid in pids {
                if pid <= 0 || pid == me_raw {
                    continue;
                }
                // SAFETY: 찾은 함수에 PID 하나를 넘긴다(없는 PID 면 -1 이나 자기 자신을 돌려준다).
                let mine = *self
                    .owner
                    .entry(pid)
                    .or_insert_with(|| unsafe { f(pid) } == me_raw);
                if mine {
                    out.push(Pid::from_u32(pid as u32));
                }
            }
            out
        }
    }

    /// 예전 방식 — 이름에 WebKit 이 들어가고 앱 시작 뒤(2초 여유)에 시작한 프로세스.
    fn fallback(sys: &mut System, me: Pid, my_start: u64) -> Vec<Pid> {
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        sys.processes()
            .iter()
            .filter(|(pid, p)| {
                **pid == me
                    || (p.name().to_string_lossy().contains("WebKit")
                        && p.start_time() + 2 >= my_start)
            })
            .map(|(pid, _)| *pid)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn idle_process_cpu_is_zero_not_the_last_busy_value() {
        use super::sampled_cpu;
        assert_eq!(sampled_cpu(None, 2485, 48.9), 48.9, "첫 표본은 그대로");
        assert_eq!(
            sampled_cpu(Some(2485), 2485, 48.9),
            0.0,
            "CPU 시간이 그대로면 0"
        );
        assert_eq!(sampled_cpu(Some(2485), 2515, 3.5), 3.5);
        assert_eq!(sampled_cpu(Some(1), 2, -1.0), 0.0);
    }

    use super::*;

    #[test]
    fn classifies_webkit_and_webview2() {
        assert_eq!(classify("cview-app", "", true), ProcKind::App);
        assert_eq!(
            classify("com.apple.WebKit.WebContent", "", false),
            ProcKind::Web
        );
        assert_eq!(classify("com.apple.WebKit.GPU", "", false), ProcKind::Gpu);
        assert_eq!(
            classify("VTDecoderXPCService", "", false),
            ProcKind::Gpu,
            "VideoToolbox 디코더는 GPU 쪽"
        );
        assert_eq!(
            classify("com.apple.WebKit.Networking", "", false),
            ProcKind::Network
        );
        assert_eq!(
            classify(
                "msedgewebview2.exe",
                "msedgewebview2.exe --type=renderer --lang=ko",
                false
            ),
            ProcKind::Web
        );
        assert_eq!(
            classify(
                "msedgewebview2.exe",
                "x --type=gpu-process --gpu-preferences=",
                false
            ),
            ProcKind::Gpu
        );
        assert_eq!(
            classify(
                "msedgewebview2.exe",
                "x --type=utility --utility-sub-type=network.mojom.NetworkService",
                false
            ),
            ProcKind::Network
        );
        // 브라우저 프로세스(--type 없음)·모르는 도우미는 기타.
        assert_eq!(
            classify(
                "msedgewebview2.exe",
                "msedgewebview2.exe --embedded-browser-webview=1",
                false
            ),
            ProcKind::Other
        );
        assert_eq!(classify("ffmpeg", "", false), ProcKind::Other);
    }

    #[test]
    fn descendants_ignore_reused_parent_pids() {
        let mut info: HashMap<u32, (Option<u32>, u64)> = HashMap::new();
        info.insert(100, (Some(1), 1_000)); // 앱
        info.insert(200, (Some(100), 1_005)); // WebView2 브라우저
        info.insert(201, (Some(200), 1_006)); // 렌더러(손자)
        info.insert(202, (Some(200), 1_005)); // 같은 초에 시작한 GPU
                                              // explorer: 부모 PID 100 은 예전에 끝난 userinit 의 것 — 앱보다 먼저 시작했으니 자식이 아니다.
        info.insert(300, (Some(100), 900));
        info.insert(301, (Some(300), 950)); // explorer 가 띄운 앱
        info.insert(400, (Some(100), 0)); // 시작 시각을 모름
        info.insert(500, (Some(999), 1_100)); // 부모가 목록에 없음
        let mut got = descendants_of(&info, 100);
        got.sort_unstable();
        assert_eq!(got, vec![100, 200, 201, 202]);
    }

    #[test]
    fn groups_sum_and_sort() {
        let g = group(&[
            (ProcKind::Web, 10.0, 100),
            (ProcKind::App, 5.0, 50),
            (ProcKind::Web, 2.5, 20),
            (ProcKind::Gpu, 1.0, 10),
        ]);
        let kinds: Vec<ProcKind> = g.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, vec![ProcKind::App, ProcKind::Web, ProcKind::Gpu]);
        assert_eq!(g[1].cpu, 12.5);
        assert_eq!(g[1].mem, 120);
        assert_eq!(g[1].count, 2);
    }

    #[test]
    fn measures_this_process() {
        let mut set = ProcessSet::new();
        let (first, rediscovered) = set.refresh();
        assert!(rediscovered);
        assert!(first.pids.contains(&std::process::id()));
        assert!(first.mem > 0, "자기 프로세스 메모리는 0 이 아니다");
        assert_eq!(first.groups.first().map(|g| g.kind), Some(ProcKind::App));
        let (_, again) = set.refresh();
        assert!(!again, "5초 안에는 다시 찾지 않는다");
    }
}
