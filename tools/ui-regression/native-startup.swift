import AppKit
import WebKit
import Foundation

// 창과 앱 활성화 없이 실제 macOS WKWebView에서 동일한 시작 감시와 모의 모듈로 검증한다.
// 저장소 루트에서: swift tools/ui-regression/native-startup.swift [저장소 경로]
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let root = CommandLine.arguments.dropFirst().first ?? FileManager.default.currentDirectoryPath
let source = try String(contentsOfFile: root + "/app-ui/index.html", encoding: .utf8)
let scriptStart = source.range(of: "<script>")!.upperBound
let scriptEnd = source.range(of: "</script>", range: scriptStart..<source.endIndex)!.lowerBound
let inline = String(source[scriptStart..<scriptEnd])
let html = "<!doctype html><html lang=ko><head><meta charset=utf-8><script>\(inline)</script><script type=module src=\"js/app.js\"></script></head><body><div id=splash>준비하는 중…</div></body></html>"

final class Probe: NSObject, WKURLSchemeHandler, WKNavigationDelegate {
    let mode: String
    var attempts = 0
    var visits = 0
    var blocked = true
    init(_ mode: String) { self.mode = mode }
    func webView(_ webView: WKWebView, didCommit navigation: WKNavigation!) { visits += 1 }
    func webView(_ webView: WKWebView, start urlSchemeTask: WKURLSchemeTask) {
        let url = urlSchemeTask.request.url!
        let data: Data
        var mime = "text/javascript"
        if url.path == "/js/app.js" {
            data = "import { ok } from './app-sheets.js'; if (window.__cviewBoot.ready()) document.body.dataset.boot = ok;".data(using: .utf8)!
        } else if url.path == "/js/app-sheets.js" {
            attempts += 1
            if blocked && (mode == "permanent" || attempts == 1) {
                if mode == "binding" { data = "export const unrelated = true;".data(using: .utf8)! }
                else { mime = "text/css"; data = "body { color: red; }".data(using: .utf8)! }
            } else { data = "export const ok = 'ready';".data(using: .utf8)! }
        } else { mime = "text/html"; data = html.data(using: .utf8)! }
        let response = URLResponse(url: url, mimeType: mime, expectedContentLength: data.count, textEncodingName: "utf-8")
        urlSchemeTask.didReceive(response)
        urlSchemeTask.didReceive(data)
        urlSchemeTask.didFinish()
    }
    func webView(_ webView: WKWebView, stop urlSchemeTask: WKURLSchemeTask) {}
}

var results: [[String: Any]] = []
for mode in ["mime", "binding", "permanent"] {
    let handler = Probe(mode)
    let config = WKWebViewConfiguration()
    config.websiteDataStore = .nonPersistent()
    config.setURLSchemeHandler(handler, forURLScheme: "cview-test")
    let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 800, height: 600), configuration: config)
    web.navigationDelegate = handler
    web.load(URLRequest(url: URL(string: "cview-test://localhost/")!))
    let deadline = Date().addingTimeInterval(20)
    var done = false
    var passed = false
    var manual = false
    var checking = false
    var error: String? = nil
    while !done && Date() < deadline {
        if !checking {
            checking = true
            web.evaluateJavaScript("({ready:document.body?.dataset.boot === 'ready', error:!!document.getElementById('boot-error'), splash:!!document.getElementById('splash')})") { value, failure in
                checking = false
                if let state = value as? [String: Bool] {
                    if mode == "permanent" && state["error"] == true && !manual {
                        guard handler.visits == 2, handler.attempts == 2, state["splash"] == false else { done = true; error = "반복 실패 제한 오류"; return }
                        manual = true
                        handler.blocked = false
                        web.evaluateJavaScript("document.querySelector('#boot-error button').click()", completionHandler: nil)
                    } else if state["ready"] == true {
                        passed = handler.visits == (mode == "permanent" ? 3 : 2)
                        done = true
                    }
                } else if let failure = failure { error = failure.localizedDescription }
            }
        }
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
    }
    results.append(["검사": mode, "통과": passed, "탐색수": handler.visits, "모듈요청수": handler.attempts, "수동복구": manual, "진단": error ?? ""])
    web.stopLoading()
}
let bytes = try JSONSerialization.data(withJSONObject: ["엔진": "macOS 네이티브 WKWebView", "창표시": false, "검사": results], options: [.prettyPrinted, .sortedKeys])
print(String(data: bytes, encoding: .utf8)!)
exit(results.allSatisfy { $0["통과"] as? Bool == true } ? 0 : 1)
