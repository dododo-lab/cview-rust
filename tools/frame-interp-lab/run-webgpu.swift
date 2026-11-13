// webgpu-bench.html 을 앱과 같은 WKWebView 에서 띄워 결과(JSON)를 출력한다.
// 사용: swiftc -O run-webgpu.swift -o run-webgpu && ./run-webgpu webgpu-bench.html "w=1920&h=1080&r=48"
import AppKit
import WebKit

let args = CommandLine.arguments
let page = URL(fileURLWithPath: args.count > 1 ? args[1] : "webgpu-bench.html")
let query = args.count > 2 ? args[2] : ""
let app = NSApplication.shared
let wv = WKWebView(frame: NSRect(x: 0, y: 0, width: 640, height: 360), configuration: WKWebViewConfiguration())
let win = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 360), styleMask: [.titled], backing: .buffered, defer: false)
win.contentView = wv
win.orderFrontRegardless()  // 가려진 창은 WebKit 이 타이머를 늦춘다

func poll() {
    wv.evaluateJavaScript("window.__result || null") { v, _ in
        if let s = v as? String {
            if let data = s.data(using: .utf8), var obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
                if let png = obj.removeValue(forKey: "png") as? String, let out = ProcessInfo.processInfo.environment["DUMP"],
                   let comma = png.firstIndex(of: ","), let bytes = Data(base64Encoded: String(png[png.index(after: comma)...])) {
                    try? bytes.write(to: URL(fileURLWithPath: out))
                }
                let d = try! JSONSerialization.data(withJSONObject: obj, options: [.sortedKeys])
                print(String(data: d, encoding: .utf8)!)
            } else { print(s) }
            exit(0)
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25, execute: poll)
    }
}
var comps = URLComponents(url: page, resolvingAgainstBaseURL: false)!
comps.query = query
wv.loadFileURL(comps.url!, allowingReadAccessTo: page.deletingLastPathComponent())
DispatchQueue.main.asyncAfter(deadline: .now() + 0.5, execute: poll)
DispatchQueue.main.asyncAfter(deadline: .now() + 120) { print("timeout"); exit(1) }
app.run()
