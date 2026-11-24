import AppKit
// 사용: appvis hide|unhide|state <pid> — 접근성 권한 없이 다른 앱을 숨기고 되돌린다.
let a = CommandLine.arguments
guard a.count >= 3, let pid = Int32(a[2]), let app = NSRunningApplication(processIdentifier: pid) else { print("usage"); exit(2) }
switch a[1] {
case "hide": print(app.hide())
case "unhide": print(app.unhide())
default: print(app.isHidden ? "hidden" : "visible")
}
RunLoop.current.run(until: Date().addingTimeInterval(0.3))
