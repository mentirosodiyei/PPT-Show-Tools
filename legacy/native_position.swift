import Foundation
@main struct PositionTests {
 static func main() {
  precondition(NativeSlidePosition.isLast("Diapositiva 33 de 33",total:33))
  precondition(NativeSlidePosition.isLast("Slide 25 of 25",total:25))
  precondition(!NativeSlidePosition.isLast("Diapositiva 3 de 25",total:25))
  precondition(!NativeSlidePosition.isLast("Diapositiva 25 de 25",total:38))
  precondition(!NativeSlidePosition.isLast("Miniatura Diapositiva 25 de 25",total:25))
  precondition(!NativeSlidePosition.isLast("",total:25))
  precondition(!NativeSlidePosition.isLast("Diapositiva 0 de 0",total:0))
  precondition(NativeSlidePosition.parse("Diapositiva 3 de 25")?.total==25)
  precondition(NativeSlidePosition.parse("Diapositiva 3 de 25")?.current==3)
  precondition(NativeSlidePosition.parse("Miniatura Diapositiva 3 de 25")==nil)
  precondition(NativeSlidePosition.parse("Diapositiva 26 de 25")==nil)
  print("11 position guard checks passed")
 }
}
