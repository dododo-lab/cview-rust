// gen-clip — 측정용 H.264 영상(1920×1080, 30fps, 10초)을 만든다. ffmpeg 없이 AVAssetWriter(하드웨어 인코더)로.
// 선명화가 일을 하도록 가는 줄·바둑판·움직이는 원을 그린다. 사용: gen-clip <출력.mp4> [폭] [높이]
import AVFoundation
import CoreGraphics

let out = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "clip.mp4")
let W = CommandLine.arguments.count > 2 ? Int(CommandLine.arguments[2]) ?? 1920 : 1920
let H = CommandLine.arguments.count > 3 ? Int(CommandLine.arguments[3]) ?? 1080 : 1080
let fps: Int32 = 30, total = 300
try? FileManager.default.removeItem(at: out)
let writer = try! AVAssetWriter(outputURL: out, fileType: .mp4)
let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
    AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: W, AVVideoHeightKey: H,
    AVVideoCompressionPropertiesKey: [AVVideoAverageBitRateKey: 8_000_000]
])
let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
    kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA, kCVPixelBufferWidthKey as String: W, kCVPixelBufferHeightKey as String: H
])
writer.add(input)
writer.startWriting()
writer.startSession(atSourceTime: .zero)
for f in 0..<total {
    while !input.isReadyForMoreMediaData { usleep(1000) }
    var pb: CVPixelBuffer?
    CVPixelBufferPoolCreatePixelBuffer(nil, adaptor.pixelBufferPool!, &pb)
    guard let buf = pb else { continue }
    CVPixelBufferLockBaseAddress(buf, [])
    let ctx = CGContext(data: CVPixelBufferGetBaseAddress(buf), width: W, height: H, bitsPerComponent: 8,
                        bytesPerRow: CVPixelBufferGetBytesPerRow(buf), space: CGColorSpaceCreateDeviceRGB(),
                        bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)!
    let t = CGFloat(f) / CGFloat(fps)
    ctx.setFillColor(CGColor(red: 0.12, green: 0.14, blue: 0.2, alpha: 1)); ctx.fill(CGRect(x: 0, y: 0, width: W, height: H))
    let cell = 24, shift = Int(t * 60) % (cell * 2)
    for y in stride(from: 0, to: H, by: cell) {
        for x in stride(from: -cell * 2, to: W, by: cell) where ((x + y) / cell) % 2 == 0 {
            ctx.setFillColor(CGColor(red: 0.7, green: 0.72, blue: 0.75, alpha: 1))
            ctx.fill(CGRect(x: x + shift, y: y, width: cell, height: cell))
        }
    }
    ctx.setStrokeColor(CGColor(red: 1, green: 0.9, blue: 0.2, alpha: 1)); ctx.setLineWidth(1.5)
    for i in 0..<60 {
        let a = CGFloat(i) / 60 * .pi * 2 + t * 0.6
        ctx.move(to: CGPoint(x: W / 2, y: H / 2))
        ctx.addLine(to: CGPoint(x: CGFloat(W / 2) + cos(a) * 520, y: CGFloat(H / 2) + sin(a) * 520))
    }
    ctx.strokePath()
    ctx.setFillColor(CGColor(red: 0.95, green: 0.3, blue: 0.35, alpha: 1))
    ctx.fillEllipse(in: CGRect(x: CGFloat(W - 200) * (0.5 + 0.5 * sin(t * 1.7)), y: CGFloat(H - 200) * (0.5 + 0.5 * cos(t * 1.3)), width: 200, height: 200))
    CVPixelBufferUnlockBaseAddress(buf, [])
    adaptor.append(buf, withPresentationTime: CMTime(value: CMTimeValue(f), timescale: fps))
}
input.markAsFinished()
let done = DispatchSemaphore(value: 0)
writer.finishWriting { done.signal() }
done.wait()
print(writer.status == .completed ? "ok \(out.path)" : "failed \(String(describing: writer.error))")
