// metal-bench — 플레이어(player.html)의 GPU 경로를 화면에 아무것도 띄우지 않고 재는 숨은 WKWebView 호스트.
//
// · Dock 아이콘·메뉴 막대 없음(.accessory), 앱을 앞으로 가져오지 않는다(activate 를 부르지 않는다).
// · 테두리 없는 창을 화면 밖(-30000,-30000)에 둔다 — 키 창이 될 수 없어 포커스도 뺏지 않는다.
// · 화면 밖 창은 macOS 가 "가려짐"으로 보고 WebKit 이 그리기를 멈추므로, 가려짐 감지를 끈다(비공개 설정, 측정 전용).
// · 페이지는 cvbench:// 사용자 스킴으로 연다 — 앱(tauri://)처럼 WKURLSchemeHandler 로 내준다.
//   영상은 범위 요청을 받는 127.0.0.1 서버에서(serve.py), CORS 로 받아 WebGL·WebGPU 가 읽을 수 있게.
//
// 사용: bench <root> <page 경로?쿼리> <영상 URL> <뷰 수> <폭pt> <높이pt> <초> <모드 JS>
//   모드 JS 는 영상이 재생되기 시작하면 각 뷰에서 한 번 실행된다(예: "setSharpen(true,1)").
//   영상 URL 이 .m3u8 이면 앱처럼 hls.js(MSE)로 재생한다. 플레이어의 pictureEnhance 메시지는
//   "player <뷰> pictureEnhance ..." 로 찍힌다(백엔드·오류·검은 화면 검사 결과).
//   1초마다 "tick <뷰> frames=<누적 프레임> backend=<webgpu|webgl|none> secure=<bool> gpu=<bool>" 를 찍는다.
import AppKit
import WebKit

let args = CommandLine.arguments
guard args.count >= 9 else {
    FileHandle.standardError.write("usage: bench <root> <page> <videoURL> <views> <w> <h> <seconds> <modeJS>\n".data(using: .utf8)!)
    exit(2)
}
let root = URL(fileURLWithPath: args[1])
let page = args[2]
let videoURL = args[3]
let views = max(1, Int(args[4]) ?? 1)
let width = CGFloat(Double(args[5]) ?? 1280)
let height = CGFloat(Double(args[6]) ?? 720)
let seconds = Double(args[7]) ?? 15
let modeJS = args[8]

final class FileScheme: NSObject, WKURLSchemeHandler {
    func webView(_ webView: WKWebView, start task: WKURLSchemeTask) {
        guard let url = task.request.url else { return }
        let file = root.appendingPathComponent(url.path)
        guard let data = try? Data(contentsOf: file) else {
            task.didFailWithError(NSError(domain: "bench", code: 404))
            return
        }
        let type = file.pathExtension == "html" ? "text/html" : (file.pathExtension == "js" ? "text/javascript" : "application/octet-stream")
        let resp = HTTPURLResponse(url: url, statusCode: 200, httpVersion: "HTTP/1.1",
                                   headerFields: ["Content-Type": type, "Content-Length": String(data.count)])!
        task.didReceive(resp)
        task.didReceive(data)
        task.didFinish()
    }
    func webView(_ webView: WKWebView, stop task: WKURLSchemeTask) {}
}

final class Bridge: NSObject, WKScriptMessageHandler {
    let index: Int
    init(_ index: Int) { self.index = index }
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        guard m.name == "bench", let d = m.body as? [String: Any] else { return }
        let f = d["frames"] ?? 0, b = d["backend"] ?? "none", s = d["secure"] ?? false, g = d["gpu"] ?? false
        print("tick \(index) frames=\(f) backend=\(b) secure=\(s) gpu=\(g)")
        fflush(stdout)
    }
}
// 플레이어의 postToSwift — 화질 경로 메시지(pictureEnhance: 백엔드·오류·검사 결과)만 찍는다.
final class Sink: NSObject, WKScriptMessageHandler {
    let index: Int
    init(_ index: Int) { self.index = index }
    func userContentController(_ c: WKUserContentController, didReceive m: WKScriptMessage) {
        guard let d = m.body as? [String: Any], (d["type"] as? String) == "pictureEnhance" else { return }
        let data = (d["data"] as? [String: Any]) ?? [:]
        let text = data.keys.sorted().map { "\($0)=\(data[$0]!)" }.joined(separator: " ")
        print("player \(index) pictureEnhance \(text)")
        fflush(stdout)
    }
}

// 영상 재생·프레임 세기·모드 적용. player.html 의 전역(video·setSharpen·setPictureEnhancement·_upscaler)을 쓴다.
func setupScript() -> String {
    let src = videoURL.replacingOccurrences(of: "'", with: "\\'")
    return """
    (function(){
      var frames = 0, applied = false;
      function count(){ video.requestVideoFrameCallback(function(){ frames++; count(); }); }
      video.crossOrigin = 'anonymous'; video.muted = true; video.loop = true;
      video.addEventListener('playing', function(){ if (applied) return; applied = true; try { \(modeJS) } catch (e) {} });
      var src = '\(src)';
      // .m3u8 은 앱과 같이 hls.js(MSE)로 — 파일 재생(AVPlayer)과 MSE 는 WebKit 안에서 프레임 경로가 다르다.
      if (/\\.m3u8(\\?|$)/.test(src) && typeof Hls !== 'undefined' && Hls.isSupported()) {
        var h = new Hls(); h.loadSource(src); h.attachMedia(video);
        h.on(Hls.Events.MANIFEST_PARSED, function(){ video.play().catch(function(){}); });
      } else { video.src = src; video.play().catch(function(){}); }
      count();
      setInterval(function(){
        window.webkit.messageHandlers.bench.postMessage({ frames: frames,
          backend: (typeof _upscaler !== 'undefined' && _upscaler && _upscaler.active) ? (_upscaler.backend || 'webgl') : 'none',
          secure: !!window.isSecureContext, gpu: !!navigator.gpu });
      }, 1000);
    })();
    """
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)

let cols = Int(ceil(sqrt(Double(views))))
let rows = Int(ceil(Double(views) / Double(cols)))
let window = NSWindow(contentRect: NSRect(x: -30000, y: -30000, width: width * CGFloat(cols), height: height * CGFloat(rows)),
                      styleMask: [.borderless], backing: .buffered, defer: false)
window.isReleasedWhenClosed = false
let container = NSView(frame: window.contentRect(forFrameRect: window.frame))
window.contentView = container

var webViews: [WKWebView] = []
let scheme = FileScheme()
for i in 0..<views {
    let cfg = WKWebViewConfiguration()
    cfg.setURLSchemeHandler(scheme, forURLScheme: "cvbench")
    cfg.userContentController.add(Bridge(i), name: "bench")
    cfg.userContentController.add(Sink(i), name: "player")
    cfg.mediaTypesRequiringUserActionForPlayback = []
    let r = NSRect(x: CGFloat(i % cols) * width, y: CGFloat(i / cols) * height, width: width, height: height)
    let wv = WKWebView(frame: r, configuration: cfg)
    let occlusion = NSSelectorFromString("_setWindowOcclusionDetectionEnabled:")
    // KVC(setValue:forKey:)는 게터가 없어 예외를 던진다 — 세터를 직접 부른다.
    if wv.responds(to: occlusion) {
        typealias Setter = @convention(c) (AnyObject, Selector, Bool) -> Void
        unsafeBitCast(wv.method(for: occlusion), to: Setter.self)(wv, occlusion, false)
    }
    container.addSubview(wv)
    webViews.append(wv)
}

final class Nav: NSObject, WKNavigationDelegate {
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        webView.evaluateJavaScript(setupScript()) { _, err in
            if let err = err { print("setup error \(err)"); fflush(stdout) }
        }
    }
}
let nav = Nav()
window.orderFrontRegardless() // 화면 밖이라 보이지 않는다. 앱 활성화는 하지 않는다.
for wv in webViews {
    wv.navigationDelegate = nav
    wv.load(URLRequest(url: URL(string: "cvbench://localhost/" + page)!))
}
print("pid \(ProcessInfo.processInfo.processIdentifier)")
fflush(stdout)
DispatchQueue.main.asyncAfter(deadline: .now() + seconds) { exit(0) }
app.run()
