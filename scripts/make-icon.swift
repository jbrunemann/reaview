// Draws Reaview's app icon and writes assets/AppIcon.png (1024×1024).
// Run: swift scripts/make-icon.swift
import AppKit

let size: CGFloat = 1024
let image = NSImage(size: NSSize(width: size, height: size))
image.lockFocus()
let ctx = NSGraphicsContext.current!.cgContext

// macOS icon grid: an 824pt rounded-square body centred on the 1024 canvas.
let body = CGRect(x: 100, y: 100, width: 824, height: 824)
let bodyPath = CGPath(roundedRect: body, cornerWidth: 185, cornerHeight: 185, transform: nil)

// Soft drop shadow under the body.
ctx.saveGState()
ctx.setShadow(offset: CGSize(width: 0, height: -12), blur: 28, color: NSColor.black.withAlphaComponent(0.35).cgColor)
ctx.addPath(bodyPath)
ctx.setFillColor(NSColor.black.cgColor)
ctx.fillPath()
ctx.restoreGState()

// Body: deep indigo to violet gradient.
ctx.saveGState()
ctx.addPath(bodyPath)
ctx.clip()
let space = CGColorSpace(name: CGColorSpace.sRGB)!
let bg = CGGradient(colorsSpace: space, colors: [
    NSColor(srgbRed: 0.33, green: 0.20, blue: 0.85, alpha: 1).cgColor,
    NSColor(srgbRed: 0.10, green: 0.42, blue: 0.95, alpha: 1).cgColor,
] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(bg, start: CGPoint(x: 0, y: 100), end: CGPoint(x: 0, y: 924), options: [])
// Subtle top sheen.
let sheen = CGGradient(colorsSpace: space, colors: [
    NSColor.white.withAlphaComponent(0.18).cgColor,
    NSColor.white.withAlphaComponent(0).cgColor,
] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(sheen, start: CGPoint(x: 0, y: 924), end: CGPoint(x: 0, y: 560), options: [])
ctx.restoreGState()

// Photos stacked behind: hints at a folder of images to flip through.
func card(_ rect: CGRect, angle: CGFloat, alpha: CGFloat) {
    ctx.saveGState()
    ctx.translateBy(x: rect.midX, y: rect.midY)
    ctx.rotate(by: angle * .pi / 180)
    let r = CGRect(x: -rect.width / 2, y: -rect.height / 2, width: rect.width, height: rect.height)
    ctx.setShadow(offset: CGSize(width: 0, height: -8), blur: 20, color: NSColor.black.withAlphaComponent(0.3).cgColor)
    ctx.addPath(CGPath(roundedRect: r, cornerWidth: 36, cornerHeight: 36, transform: nil))
    ctx.setFillColor(NSColor.white.withAlphaComponent(alpha).cgColor)
    ctx.fillPath()
    ctx.restoreGState()
}
let photo = CGRect(x: 262, y: 300, width: 500, height: 420)
card(photo.offsetBy(dx: -40, dy: 14), angle: 9, alpha: 0.45)
card(photo.offsetBy(dx: 40, dy: 14), angle: -9, alpha: 0.65)
card(photo, angle: 0, alpha: 1)

// The front photo: sky, sun and mountains inside a white border.
let inner = photo.insetBy(dx: 30, dy: 30)
ctx.saveGState()
ctx.addPath(CGPath(roundedRect: inner, cornerWidth: 16, cornerHeight: 16, transform: nil))
ctx.clip()
let sky = CGGradient(colorsSpace: space, colors: [
    NSColor(srgbRed: 1.00, green: 0.62, blue: 0.38, alpha: 1).cgColor,
    NSColor(srgbRed: 0.42, green: 0.70, blue: 1.00, alpha: 1).cgColor,
] as CFArray, locations: [0, 1])!
ctx.drawLinearGradient(sky, start: CGPoint(x: 0, y: inner.minY), end: CGPoint(x: 0, y: inner.maxY), options: [])
ctx.setFillColor(NSColor(srgbRed: 1.0, green: 0.93, blue: 0.55, alpha: 1).cgColor)
ctx.fillEllipse(in: CGRect(x: inner.maxX - 150, y: inner.maxY - 140, width: 92, height: 92))
func mountain(_ points: [CGPoint], _ color: NSColor) {
    ctx.beginPath()
    ctx.move(to: points[0])
    points.dropFirst().forEach { ctx.addLine(to: $0) }
    ctx.closePath()
    ctx.setFillColor(color.cgColor)
    ctx.fillPath()
}
mountain([CGPoint(x: inner.minX, y: inner.minY), CGPoint(x: inner.minX + 150, y: inner.minY + 190),
          CGPoint(x: inner.minX + 300, y: inner.minY)], NSColor(srgbRed: 0.24, green: 0.36, blue: 0.62, alpha: 1))
mountain([CGPoint(x: inner.minX + 120, y: inner.minY), CGPoint(x: inner.minX + 300, y: inner.minY + 240),
          CGPoint(x: inner.maxX, y: inner.minY + 40), CGPoint(x: inner.maxX, y: inner.minY)],
         NSColor(srgbRed: 0.15, green: 0.24, blue: 0.48, alpha: 1))
ctx.restoreGState()

// Previous / next chevrons on either side.
func chevron(_ x: CGFloat, pointsLeft: Bool) {
    let dx: CGFloat = pointsLeft ? 44 : -44
    ctx.beginPath()
    ctx.move(to: CGPoint(x: x + dx, y: 590))
    ctx.addLine(to: CGPoint(x: x, y: 510))
    ctx.addLine(to: CGPoint(x: x + dx, y: 430))
    ctx.setStrokeColor(NSColor.white.withAlphaComponent(0.92).cgColor)
    ctx.setLineWidth(34)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)
    ctx.strokePath()
}
chevron(150, pointsLeft: true)
chevron(874, pointsLeft: false)

image.unlockFocus()

let rep = NSBitmapImageRep(data: image.tiffRepresentation!)!
let png = rep.representation(using: .png, properties: [:])!
let out = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "assets/AppIcon.png")
try! png.write(to: out)
print("Wrote \(out.path)")
