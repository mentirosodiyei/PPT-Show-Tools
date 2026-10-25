import AppKit
let output = CommandLine.arguments[1]
let image = NSImage(size: NSSize(width:1024,height:1024))
image.lockFocus()
let background = NSBezierPath(roundedRect:NSRect(x:32,y:32,width:960,height:960),xRadius:210,yRadius:210)
NSGradient(starting:NSColor(calibratedRed:0.12,green:0.22,blue:0.38,alpha:1),ending:NSColor(calibratedRed:0.03,green:0.09,blue:0.19,alpha:1))!.draw(in:background,angle:65)
func card(_ x:CGFloat,_ y:CGFloat,_ color:NSColor) {
 color.setFill(); NSBezierPath(roundedRect:NSRect(x:x,y:y,width:520,height:340),xRadius:38,yRadius:38).fill()
}
card(170,485,NSColor(calibratedRed:0.40,green:0.62,blue:0.85,alpha:1))
card(230,420,NSColor(calibratedRed:0.72,green:0.84,blue:0.96,alpha:1))
card(290,355,.white)
NSColor(calibratedRed:0.94,green:0.35,blue:0.15,alpha:1).setFill()
NSBezierPath(roundedRect:NSRect(x:330,y:535,width:440,height:115),xRadius:18,yRadius:18).fill()
let attributes:[NSAttributedString.Key:Any] = [.font:NSFont.systemFont(ofSize:64,weight:.heavy),.foregroundColor:NSColor.white]
("PPT" as NSString).draw(at:NSPoint(x:480,y:553),withAttributes:attributes)
NSColor(calibratedRed:0.16,green:0.32,blue:0.51,alpha:1).setFill()
for y in [CGFloat(470),CGFloat(415)] { NSBezierPath(roundedRect:NSRect(x:340,y:y,width:y == 470 ? 380 : 265,height:22),xRadius:11,yRadius:11).fill() }
let arrow = NSBezierPath(); arrow.move(to:NSPoint(x:250,y:345));arrow.line(to:NSPoint(x:250,y:255));arrow.curve(to:NSPoint(x:300,y:205),controlPoint1:NSPoint(x:250,y:215),controlPoint2:NSPoint(x:265,y:205));arrow.line(to:NSPoint(x:635,y:205));arrow.lineWidth=32;arrow.lineCapStyle = .round
NSColor(calibratedRed:0.98,green:0.60,blue:0.22,alpha:1).setStroke(); arrow.stroke()
let tip=NSBezierPath();tip.move(to:NSPoint(x:595,y:260));tip.line(to:NSPoint(x:665,y:205));tip.line(to:NSPoint(x:595,y:150));tip.lineWidth=32;tip.lineCapStyle = .round;tip.lineJoinStyle = .round;tip.stroke()
image.unlockFocus()
let rep=NSBitmapImageRep(data:image.tiffRepresentation!)!
try rep.representation(using:.png,properties:[:])!.write(to:URL(fileURLWithPath:output))
